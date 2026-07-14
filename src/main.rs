use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints, LineStyle};
use ringbuf::{traits::{Consumer, Producer, Split}, HeapRb};
use rustfft::{num_complex::Complex, FftPlanner};

// --- 定数定義 ---
const FFT_SIZE: usize = 2048;
const RINGBUF_SIZE: usize = FFT_SIZE * 8; // 余裕を持たせたバッファサイズ

/// アプリケーションのメイン状態
struct AnalyzerApp<C: Consumer<Item = f32>> {
    audio_consumer: C,
    sample_buffer: Vec<f32>,
    magnitude_data: Vec<[f64; 2]>,
    planner: FftPlanner<f32>,
    /// デバイスのネイティブサンプリングレートを動的に保持
    actual_sample_rate: u32,
}

impl<C: Consumer<Item = f32>> AnalyzerApp<C> {
    fn new(audio_consumer: C, actual_sample_rate: u32) -> Self {
        Self {
            audio_consumer,
            sample_buffer: Vec::with_capacity(FFT_SIZE),
            magnitude_data: vec![[0.0; 2]; FFT_SIZE / 2],
            planner: FftPlanner::new(),
            actual_sample_rate,
        }
    }

    fn process_dsp(&mut self) {
        while let Some(sample) = self.audio_consumer.try_pop() {
            self.sample_buffer.push(sample);
            
            if self.sample_buffer.len() >= FFT_SIZE {
                self.perform_fft();
                self.sample_buffer.drain(0..(FFT_SIZE / 2));
            }
        }
    }

    fn perform_fft(&mut self) {
        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        
        let mut buffer: Vec<Complex<f32>> = self.sample_buffer
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
                Complex { re: s * window, im: 0.0 }
            })
            .collect();

        fft.process(&mut buffer);

        // ネイティブサンプリングレートに基づく正確な周波数分解能の算出
        let bin_resolution = self.actual_sample_rate as f64 / FFT_SIZE as f64;
        self.magnitude_data.clear();

        for i in 1..(FFT_SIZE / 2) {
            let freq = i as f64 * bin_resolution;
            let norm = 2.0 / FFT_SIZE as f32;
            let mag = (buffer[i].norm() * norm).max(1e-6);
            let db = 20.0 * mag.log10();
            
            self.magnitude_data.push([freq, db as f64]);
        }
    }
}

impl<C: Consumer<Item = f32>> eframe::App for AnalyzerApp<C> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_dsp();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("Acoustic Analyzer - Realtime Spectrum ({} Hz)", self.actual_sample_rate));
            ui.separator();

            let line = Line::new(PlotPoints::from_iter(self.magnitude_data.iter().copied()))
                .color(egui::Color32::from_rgb(0, 255, 128))
                .style(LineStyle::Solid)
                .name("Magnitude (dB)");

            Plot::new("magnitude_plot")
                // egui_plot 0.30 仕様: 2引数 (GridMark, &RangeInclusive) を受け取る
                .x_axis_formatter(|mark, _range| format!("{:.0} Hz", mark.value))
                .y_axis_formatter(|mark, _range| format!("{:.1} dB", mark.value))
                .x_grid_spacer(egui_plot::log_grid_spacer(10))
                .view_aspect(2.0)
                .show(ui, |plot_ui| {
                    plot_ui.line(line);
                });
        });

        ctx.request_repaint();
    }
}

// --- オーディオI/O スレッドの初期化 ---
// コンシューマに加えて、デバイスから取得したネイティブなサンプリングレートを返す
fn setup_audio() -> (impl Consumer<Item = f32>, u32) {
    // 1. プロフェッショナル要件の第一原理: OSミキサー完全バイパスのため、ASIOを最優先とする。
    // ASIOが利用不可能な場合のみ、WASAPIへフォールバックする。
    let host = cpal::host_from_id(cpal::HostId::Asio)
        .or_else(|_| cpal::host_from_id(cpal::HostId::Wasapi))
        .unwrap_or_else(|_| cpal::default_host());

    let device = host.default_input_device().expect("マイク入力デバイスが見つかりません。");
    println!("オーディオデバイスを初期化しました。使用ホスト: {:?}", host.id());

    let config = device.default_input_config().expect("デバイスのデフォルト設定が取得できません。");
    let sample_format = config.sample_format();
    
    // デバイスのネイティブ設定を完全尊重し、強制的なリサンプリングを回避する
    let mut stream_config: StreamConfig = config.clone().into();
    
    // Windows環境 (WASAPI) における E_INVALIDARG (-2147024809) 回避策:
    // BufferSize::Default をそのまま渡すと、一部のオーディオスタックがパニックを起こすため、
    // ハードウェアがサポートするバッファサイズ範囲から具体的な値を明示的に指定する。
    if host.id() == cpal::HostId::Wasapi {
        if let cpal::SupportedBufferSize::Range { min, max } = config.buffer_size() {
            // レイテンシと安定性のバランスを取り、最小値と最大値の安全な範囲を選択
            let target_buffer_size = (*min).max(256).min(*max);
            stream_config.buffer_size = BufferSize::Fixed(target_buffer_size);
        }
    }

    // cpal's StreamConfig.sample_rate may be a plain u32 in some versions,
    // so handle it directly without field access.
    let actual_sample_rate = stream_config.sample_rate;
    let channels = stream_config.channels as usize;

    let rb = HeapRb::<f32>::new(RINGBUF_SIZE);
    let (producer, consumer) = rb.split();

    let stream = match sample_format {
        SampleFormat::F32 => build_input_stream::<f32>(&device, stream_config.clone(), channels, producer),
        SampleFormat::I16 => build_input_stream::<i16>(&device, stream_config.clone(), channels, producer),
        SampleFormat::U16 => build_input_stream::<u16>(&device, stream_config.clone(), channels, producer),
        _ => {
            panic!("f32 / i16 / u16 以外のサンプルフォーマットは現段階で未対応です。");
        }
    }
    .expect("オーディオストリームの構築に失敗しました。");

    stream.play().expect("オーディオストリームの再生開始に失敗しました。");

    Box::leak(Box::new(stream));

    (consumer, actual_sample_rate)
}

fn main() -> eframe::Result<()> {
    // ネイティブサンプリングレートを取得し、アプリケーションへ伝搬
    let (consumer, actual_sample_rate) = setup_audio();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 600.0])
            .with_title("Pro Audio Analyzer"),
        ..Default::default()
    };

    eframe::run_native(
        "Audio Analyzer",
        options,
        Box::new(move |_cc| Ok(Box::new(AnalyzerApp::new(consumer, actual_sample_rate)))),
    )
}

fn build_input_stream<T>(
    device: &cpal::Device,
    stream_config: StreamConfig,
    channels: usize,
    mut producer: impl Producer<Item = f32> + Send + 'static,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>>
where
    T: Sample + SizedSample + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            stream_config,
        move |data: &[T], _: &_| {
            // マルチチャンネル環境でFFTの周波数軸が狂うのを防ぐため、Lチャンネル(1ch目)のみを抽出
            for frame in data.chunks(channels) {
                if let Some(&sample) = frame.first() {
                    let sample_f32: f32 = sample.to_sample();
                    let _ = producer.try_push(sample_f32);
                }
            }
        },
        // 型推論に委ねるクロージャとして定義し、cpalのバージョン間差異を吸収
        |err| eprintln!("オーディオストリームエラー: {}", err),
        None,
    )
    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
}