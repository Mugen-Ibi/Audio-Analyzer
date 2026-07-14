use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{StreamConfig, BufferSize};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints, LineStyle};
use ringbuf::{traits::{Consumer, Producer, Split}, HeapRb};
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;

// --- 定数定義 ---
const FFT_SIZE: usize = 2048;
const RINGBUF_SIZE: usize = FFT_SIZE * 8; // 余裕を持たせたバッファサイズ
const SAMPLE_RATE: u32 = 48000;

/// アプリケーションのメイン状態
/// Consumer トレイトを実装した型を汎用的に受け入れるようジェネリクス化
struct AnalyzerApp<C: Consumer<Item = f32>> {
    /// ロックフリーでオーディオI/Oスレッドからデータを受け取るコンシューマ
    audio_consumer: C,
    /// 蓄積用バッファ
    sample_buffer: Vec<f32>,
    /// 描画用の周波数特性データ (X: 周波数, Y: 振幅dB)
    magnitude_data: Vec<[f64; 2]>,
    /// FFTプランナー
    planner: FftPlanner<f32>,
}

impl<C: Consumer<Item = f32>> AnalyzerApp<C> {
    fn new(audio_consumer: C) -> Self {
        Self {
            audio_consumer,
            sample_buffer: Vec::with_capacity(FFT_SIZE),
            magnitude_data: vec![[0.0; 2]; FFT_SIZE / 2],
            planner: FftPlanner::new(),
        }
    }

    /// バッファからデータを読み出し、十分なサンプルがあればFFTを実行する
    fn process_dsp(&mut self) {
        // ロックフリーで利用可能なサンプルを取り出す（ミューテックスの完全排除）
        while let Some(sample) = self.audio_consumer.try_pop() {
            self.sample_buffer.push(sample);
            
            // バッファがFFTサイズに達したら処理を実行
            if self.sample_buffer.len() >= FFT_SIZE {
                self.perform_fft();
                // 処理後、バッファの半分を破棄（50%オーバーラップ処理）
                self.sample_buffer.drain(0..(FFT_SIZE / 2));
            }
        }
    }

    fn perform_fft(&mut self) {
        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        
        // ハニング窓の適用と複素数への変換
        let mut buffer: Vec<Complex<f32>> = self.sample_buffer
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
                Complex { re: s * window, im: 0.0 }
            })
            .collect();

        // FFT実行 (In-place)
        fft.process(&mut buffer);

        // 振幅のdB換算 (ナイキスト周波数まで)
        let bin_resolution = SAMPLE_RATE as f64 / FFT_SIZE as f64;
        self.magnitude_data.clear();

        for i in 1..(FFT_SIZE / 2) {
            let freq = i as f64 * bin_resolution;
            let norm = 2.0 / FFT_SIZE as f32; // 正規化
            let mag = (buffer[i].norm() * norm).max(1e-6); // ゼロ除算・Log(0)回避
            let db = 20.0 * mag.log10();
            
            self.magnitude_data.push([freq, db as f64]);
        }
    }
}

impl<C: Consumer<Item = f32>> eframe::App for AnalyzerApp<C> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. DSP処理の実行 (Consumerからのデータ吸い出しとFFT)
        self.process_dsp();

        // 2. Immediate Mode GUIによる即時描画
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Acoustic Analyzer - Realtime Spectrum (Phase 3)");
            ui.separator();

            let line = Line::new(PlotPoints::from_iter(self.magnitude_data.iter().copied()))
                .color(egui::Color32::from_rgb(0, 255, 128))
                .style(LineStyle::Solid)
                .name("Magnitude (dB)");

            Plot::new("magnitude_plot")
                .x_axis_formatter(|val, _, _| format!("{:.0} Hz", val))
                .y_axis_formatter(|val, _, _| format!("{:.1} dB", val))
                .x_grid_spacer(egui_plot::log_grid_spacer(10)) // 対数スケール化
                .view_aspect(2.0)
                .show(ui, |plot_ui| {
                    plot_ui.line(line);
                });
        });

        // 60fps以上の応答性を保証するため、連続的な再描画を要求
        ctx.request_repaint();
    }
}

// --- オーディオI/O スレッドの初期化 ---
// 戻り値を具象型ではなく、impl Trait による抽象型に変更し、バージョン間の差異を吸収
fn setup_audio() -> impl Consumer<Item = f32> {
    // Windows環境におけるOSミキサーのバイパスを意図し、可能であればWASAPIを指定。
    // クロスプラットフォーム対応のため、デフォルトホストをフォールバックとして取得。
    let host = cpal::host_from_id(cpal::HostId::Wasapi)
        .unwrap_or_else(|_| cpal::default_host());

    let device = host.default_input_device().expect("マイク入力デバイスが見つかりません。");
    println!("使用デバイス: {}", device.name().unwrap_or_else(|_| "Unknown".to_string()));

    let config = device.default_input_config().expect("デバイスのデフォルト設定が取得できません。");
    let sample_format = config.sample_format();
    
    // 強制的に要求仕様のサンプリングレートへオーバーライド
    let stream_config = StreamConfig {
        channels: 1, // Phase 3時点ではモノラル入力
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: BufferSize::Default,
    };

    // スレッド間通信用のロックフリー・リングバッファを生成
    let rb = HeapRb::<f32>::new(RINGBUF_SIZE);
    let (mut producer, consumer) = rb.split();

    // DSP/オーディオスレッド構築（GCやヒープ割り当てを内部で一切行わない）
    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            // エラーを解消するため、コンパイラの要求に従い参照ではなく値（clone/into）として渡す
            device.build_input_stream(
                stream_config.into(),
                move |data: &[f32], _: &_| {
                    // Lock-free data transmission
                    producer.push_slice(data);
                },
                err_fn,
                None,
            )
        },
        _ => panic!("f32以外のサンプルフォーマットは現段階で未対応です。"),
    }.expect("オーディオストリームの構築に失敗しました。");

    stream.play().expect("オーディオストリームの再生開始に失敗しました。");

    // メインスレッドが終了するまでオーディオストリームを維持するため、
    // Box::leakを用いてライフタイムを'staticに引き上げる（アーキテクチャ上のハック）
    Box::leak(Box::new(stream));

    consumer
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("オーディオ入力ストリームエラーが発生しました: {}", err);
}

fn main() -> eframe::Result<()> {
    // 1. オーディオI/Oを初期化し、Lock-free Queueのコンシューマを受け取る
    let consumer = setup_audio();

    // 2. GUIの初期設定 (Immediate Mode, Retained GUIの徹底排除)
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 600.0])
            .with_title("Pro Audio Analyzer"),
        ..Default::default()
    };

    // 3. アプリケーションの実行開始
    eframe::run_native(
        "Audio Analyzer",
        options,
        Box::new(|_cc| Ok(Box::new(AnalyzerApp::new(consumer)))),
    )
}