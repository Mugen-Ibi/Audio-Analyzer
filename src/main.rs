use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use eframe::egui;
use egui_plot::{Line, LineStyle, Plot, PlotPoints};
use ringbuf::{traits::{Consumer, Producer, Split}, HeapRb};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

// --- 第一原理に基づく定数定義 ---
const FFT_SIZE: usize = 2048;
const RINGBUF_SIZE: usize = FFT_SIZE * 8;
// 指数移動平均（EMA）の平滑化係数。
// 瞬時のFFTノイズを抑えつつ、物理的な変化に追従するための定数。
const EMA_ALPHA: f32 = 0.15; 

/// アプリケーションのメイン状態
/// Insight 2クラスの分析を行うため、入力は (Reference, Measurement) のデュアルチャネルとなる
struct AnalyzerApp<C: Consumer<Item = (f32, f32)>> {
    audio_consumer: C,
    
    // 時間領域バッファ
    ref_buffer: Vec<f32>,
    meas_buffer: Vec<f32>,
    
    // 窓関数（事前計算によるゼロコスト化）
    window: Vec<f32>,

    // クロススペクトル算出用 平滑化バッファ
    s_xx: Vec<f32>,
    s_yy: Vec<f32>,
    s_xy: Vec<Complex<f32>>,
    
    // 描画用データ
    tf_mag_data: Vec<[f64; 2]>,
    tf_phase_data: Vec<[f64; 2]>,
    coherence_data: Vec<[f64; 2]>,
    
    fft: Arc<dyn Fft<f32>>,
    actual_sample_rate: u32,
}

impl<C: Consumer<Item = (f32, f32)>> AnalyzerApp<C> {
    fn new(audio_consumer: C, actual_sample_rate: u32) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()))
            .collect();

        let half_size = FFT_SIZE / 2;

        Self {
            audio_consumer,
            ref_buffer: Vec::with_capacity(FFT_SIZE),
            meas_buffer: Vec::with_capacity(FFT_SIZE),
            window,
            s_xx: vec![0.0; half_size],
            s_yy: vec![0.0; half_size],
            s_xy: vec![Complex { re: 0.0, im: 0.0 }; half_size],
            tf_mag_data: Vec::with_capacity(half_size),
            tf_phase_data: Vec::with_capacity(half_size),
            coherence_data: Vec::with_capacity(half_size),
            fft,
            actual_sample_rate,
        }
    }

    fn process_dsp(&mut self) {
        // ロックフリーキューからのデュアルストリーム抽出
        while let Some((ref_sample, meas_sample)) = self.audio_consumer.try_pop() {
            self.ref_buffer.push(ref_sample);
            self.meas_buffer.push(meas_sample);
            
            // バッファが満たされたら伝達関数を計算し、オーバーラップ処理のために半分捨てる（50% Overlap）
            if self.ref_buffer.len() >= FFT_SIZE {
                self.perform_transfer_function();
                self.ref_buffer.drain(0..(FFT_SIZE / 2));
                self.meas_buffer.drain(0..(FFT_SIZE / 2));
            }
        }
    }

    /// クロススペクトル法に基づく伝達関数とコヒーレンスの厳密な算出
    fn perform_transfer_function(&mut self) {
        let mut ref_fft_buf: Vec<Complex<f32>> = self.ref_buffer.iter().zip(self.window.iter())
            .map(|(&s, &w)| Complex { re: s * w, im: 0.0 }).collect();
            
        let mut meas_fft_buf: Vec<Complex<f32>> = self.meas_buffer.iter().zip(self.window.iter())
            .map(|(&s, &w)| Complex { re: s * w, im: 0.0 }).collect();

        self.fft.process(&mut ref_fft_buf);
        self.fft.process(&mut meas_fft_buf);

        let bin_resolution = self.actual_sample_rate as f64 / FFT_SIZE as f64;
        
        self.tf_mag_data.clear();
        self.tf_phase_data.clear();
        self.coherence_data.clear();

        for i in 1..(FFT_SIZE / 2) {
            let x = ref_fft_buf[i];
            let y = meas_fft_buf[i];

            let s_xx = x.norm_sqr();
            let s_yy = y.norm_sqr();
            let s_xy = y * x.conj();

            // EMA (Exponential Moving Average) による時間平均。
            // これを行わないと位相とコヒーレンスはランダムノイズの海に沈む。
            self.s_xx[i] = (1.0 - EMA_ALPHA) * self.s_xx[i] + EMA_ALPHA * s_xx;
            self.s_yy[i] = (1.0 - EMA_ALPHA) * self.s_yy[i] + EMA_ALPHA * s_yy;
            self.s_xy[i] = Complex {
                re: (1.0 - EMA_ALPHA) * self.s_xy[i].re + EMA_ALPHA * s_xy.re,
                im: (1.0 - EMA_ALPHA) * self.s_xy[i].im + EMA_ALPHA * s_xy.im,
            };

            let freq = i as f64 * bin_resolution;

            // 伝達関数 H(f) = S_xy / S_xx (ノイズフロアによるゼロ除算を1e-12で防御)
            let h_mag = (self.s_xy[i].norm() / self.s_xx[i].max(1e-12)).max(1e-6);
            let mag_db = 20.0 * h_mag.log10();
            
            // 位相角 (-180 to +180)
            let phase_deg = self.s_xy[i].arg() * 180.0 / std::f32::consts::PI;

            // コヒーレンス (0.0 to 1.0)
            let coherence = self.s_xy[i].norm_sqr() / (self.s_xx[i] * self.s_yy[i]).max(1e-12);

            self.tf_mag_data.push([freq, mag_db as f64]);
            self.tf_phase_data.push([freq, phase_deg as f64]);
            self.coherence_data.push([freq, coherence as f64]);
        }
    }
}

impl<C: Consumer<Item = (f32, f32)>> eframe::App for AnalyzerApp<C> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_dsp();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("Pro Audio Analyzer - Transfer Function (Fs: {} Hz)", self.actual_sample_rate));
            ui.separator();

            // Insight 2ライクなマルチメーター基盤としてのUI分割レイアウト
            ui.vertical(|ui| {
                // --- 上部: 振幅応答 (Transfer Function Magnitude) ---
                ui.allocate_ui(egui::vec2(ui.available_width(), ui.available_height() * 0.5), |ui| {
                    ui.label("Transfer Function Magnitude (dB)");
                    let line_mag = Line::new(PlotPoints::from_iter(self.tf_mag_data.iter().copied()))
                        .color(egui::Color32::from_rgb(0, 255, 128)).style(LineStyle::Solid);

                    Plot::new("magnitude_plot")
                        .x_axis_formatter(|mark, _| format!("{:.0} Hz", mark.value))
                        .y_axis_formatter(|mark, _| format!("{:.0} dB", mark.value))
                        .x_grid_spacer(egui_plot::log_grid_spacer(10))
                        .include_y(-60.0).include_y(24.0) // 基準表示領域
                        .show(ui, |plot_ui| plot_ui.line(line_mag));
                });

                ui.separator();

                // --- 下部: 位相 (Phase) & コヒーレンス (Coherence) の二画面分割 ---
                ui.columns(2, |cols| {
                    cols[0].vertical(|ui| {
                        ui.label("Phase (Degrees)");
                        let line_phase = Line::new(PlotPoints::from_iter(self.tf_phase_data.iter().copied()))
                            .color(egui::Color32::from_rgb(0, 191, 255)).style(LineStyle::Solid);

                        Plot::new("phase_plot")
                            .x_axis_formatter(|mark, _| format!("{:.0} Hz", mark.value))
                            .y_axis_formatter(|mark, _| format!("{:.0}°", mark.value))
                            .x_grid_spacer(egui_plot::log_grid_spacer(10))
                            .include_y(-180.0).include_y(180.0) // 位相の絶対境界
                            .show(ui, |plot_ui| plot_ui.line(line_phase));
                    });

                    cols[1].vertical(|ui| {
                        ui.label("Coherence (Data Reliability 0-1)");
                        let line_coh = Line::new(PlotPoints::from_iter(self.coherence_data.iter().copied()))
                            .color(egui::Color32::from_rgb(255, 165, 0)).style(LineStyle::Solid);

                        Plot::new("coherence_plot")
                            .x_axis_formatter(|mark, _| format!("{:.0} Hz", mark.value))
                            .y_axis_formatter(|mark, _| format!("{:.2}", mark.value))
                            .x_grid_spacer(egui_plot::log_grid_spacer(10))
                            .include_y(0.0).include_y(1.05) // コヒーレンスの上限
                            .show(ui, |plot_ui| plot_ui.line(line_coh));
                    });
                });
            });
        });

        // 60fps以上の即時描画を維持し、Retained GUIの遅延を否定する
        ctx.request_repaint();
    }
}

// --- オーディオI/O スレッドの初期化 ---
fn setup_audio() -> (impl Consumer<Item = (f32, f32)>, u32) {
    let host = cpal::host_from_id(cpal::HostId::Asio)
        .or_else(|_| cpal::host_from_id(cpal::HostId::Wasapi))
        .unwrap_or_else(|_| cpal::default_host());

    let device = host.default_input_device().expect("マイク入力デバイスが見つかりません。");
    println!("オーディオデバイスを初期化しました。使用ホスト: {:?}", host.id());

    let config = device.default_input_config().expect("デバイスのデフォルト設定が取得できません。");
    let sample_format = config.sample_format();
    
    let mut stream_config: StreamConfig = config.clone().into();
    
    if host.id() == cpal::HostId::Wasapi {
        if let cpal::SupportedBufferSize::Range { min, max } = config.buffer_size() {
            let target_buffer_size = (*min).max(256).min(*max);
            stream_config.buffer_size = BufferSize::Fixed(target_buffer_size);
        }
    }

    let actual_sample_rate = stream_config.sample_rate;
    let channels = stream_config.channels as usize;

    let rb = HeapRb::<(f32, f32)>::new(RINGBUF_SIZE);
    let (producer, consumer) = rb.split();

    let stream = match sample_format {
        SampleFormat::F32 => build_input_stream::<f32>(&device, stream_config.clone(), channels, producer),
        SampleFormat::I16 => build_input_stream::<i16>(&device, stream_config.clone(), channels, producer),
        SampleFormat::U16 => build_input_stream::<u16>(&device, stream_config.clone(), channels, producer),
        _ => panic!("サポートされていないサンプルフォーマットです。"),
    }
    .expect("オーディオストリームの構築に失敗しました。");

    stream.play().expect("オーディオストリームの再生開始に失敗しました。");
    Box::leak(Box::new(stream));

    (consumer, actual_sample_rate)
}

fn build_input_stream<T>(
    device: &cpal::Device,
    stream_config: StreamConfig,
    channels: usize,
    mut producer: impl Producer<Item = (f32, f32)> + Send + 'static,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>>
where
    T: Sample + SizedSample + 'static,
    f32: FromSample<T>,
{
    device.build_input_stream(
        stream_config,
        move |data: &[T], _: &_| {
            // ステレオ入力を前提としたルーティング (1ch = Ref, 2ch = Meas)
            for frame in data.chunks(channels) {
                let ref_sample = frame.first().map(|s| s.to_sample::<f32>()).unwrap_or(0.0);
                // モノラルの場合はReferenceをMeasurmentにコピー（自己相関となりコヒーレンスは常に1となる）
                let meas_sample = frame.get(1).map(|s| s.to_sample::<f32>()).unwrap_or(ref_sample); 
                let _ = producer.try_push((ref_sample, meas_sample));
            }
        },
        |err| eprintln!("オーディオストリームエラー: {}", err),
        None,
    )
    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
}

fn main() -> eframe::Result<()> {
    let (consumer, actual_sample_rate) = setup_audio();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_title("Pro Audio Analyzer - Full Rust Native"),
        ..Default::default()
    };

    eframe::run_native(
        "Audio Analyzer",
        options,
        Box::new(move |_cc| Ok(Box::new(AnalyzerApp::new(consumer, actual_sample_rate)))),
    )
}