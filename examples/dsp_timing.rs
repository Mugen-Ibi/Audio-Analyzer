//! Pure DSP microbenchmark; excludes device/driver/display latency.
use audio_analyzer::{
    config::{AnalysisConfig, WindowFunction},
    dsp::SpectrumAnalyzer,
    model::{AUDIO_BLOCK_FRAMES, AudioBlock, StereoFrame},
};
use std::time::Instant;
fn main() {
    println!(
        "Pure DSP: 48 kHz, 2 channels, 50% overlap, 3000 measured windows; excludes device, queues and rendering."
    );
    for config in [
        AnalysisConfig::FAST,
        AnalysisConfig::default(),
        AnalysisConfig::PRECISE,
        AnalysisConfig {
            fft_size: 8192,
            window: WindowFunction::Rectangular,
            averaging_ms: 0,
        },
    ] {
        let mut a = SpectrumAnalyzer::with_config(48000, config).unwrap();
        let mut block = AudioBlock {
            valid_frames: AUDIO_BLOCK_FRAMES,
            has_reference: true,
            ..Default::default()
        };
        for (i, f) in block.frames.iter_mut().enumerate() {
            let x = (std::f32::consts::TAU * i as f32 / 64.0).sin() * 0.5;
            *f = StereoFrame {
                reference: x,
                measurement: x * 0.5,
            };
        }
        let mut times = Vec::with_capacity(3000);
        let mut windows = 0;
        let began = Instant::now();
        while windows < 3100 {
            let t = Instant::now();
            let s = std::hint::black_box(a.process_block(&block));
            let dt = t.elapsed().as_secs_f64() * 1e6;
            if s.is_some() {
                if windows >= 100 {
                    times.push(dt);
                }
                windows += 1;
            }
            block.start_frame += AUDIO_BLOCK_FRAMES as u64;
        }
        times.sort_by(f64::total_cmp);
        println!(
            "FFT={} {:?} avg={}ms: analyze-block p50={:.2}us p95={:.2}us max={:.2}us; total={:.2}ms (includes non-FFT blocks)",
            config.fft_size,
            config.window,
            config.averaging_ms,
            times[1499],
            times[2849],
            times[2999],
            began.elapsed().as_secs_f64() * 1000.0
        );
    }
}
