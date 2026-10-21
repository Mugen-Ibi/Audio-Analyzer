//! A01–A04: digital reference computations, never an audio device.
use audio_analyzer::{
    config::{AnalysisConfig, WindowFunction},
    dsp::SpectrumAnalyzer,
    model::{AUDIO_BLOCK_FRAMES, AnalysisSnapshot, AudioBlock, StereoFrame},
};

fn analyze(
    config: AnalysisConfig,
    rate: u32,
    windows: usize,
    signal: impl Fn(usize) -> (f32, f32),
) -> AnalysisSnapshot {
    let mut analyzer = SpectrumAnalyzer::with_config(rate, config).unwrap();
    let mut latest = None;
    let frames = config.fft_size + (windows - 1) * config.hop();
    for index in (0..frames).step_by(AUDIO_BLOCK_FRAMES) {
        let mut block = AudioBlock {
            start_frame: index as u64,
            valid_frames: AUDIO_BLOCK_FRAMES,
            has_reference: true,
            ..Default::default()
        };
        for (offset, frame) in block.frames.iter_mut().enumerate() {
            let (a, b) = signal(index + offset);
            *frame = StereoFrame {
                reference: a,
                measurement: b,
            };
        }
        if let Some(snapshot) = analyzer.process_block(&block) {
            latest = Some(snapshot);
        }
    }
    latest.unwrap()
}
#[test]
fn a01_amplitude_frequency_peak_and_rms_matrix() {
    for rate in [48000, 96000] {
        for size in [1024, 2048, 8192] {
            for window in [WindowFunction::Hann, WindowFunction::Rectangular] {
                for db in [-6.0_f64, -20.0, -60.0] {
                    let amplitude = 10.0_f64.powf(db / 20.0);
                    let bin = 32;
                    let s = analyze(
                        AnalysisConfig {
                            fft_size: size,
                            window,
                            averaging_ms: 0,
                        },
                        rate,
                        1,
                        |i| {
                            let value = (amplitude
                                * (std::f64::consts::TAU * bin as f64 * i as f64 / size as f64)
                                    .sin()) as f32;
                            (value, value * 0.5)
                        },
                    );
                    assert!(
                        (s.bins[bin].reference_dbfs as f64 - db).abs() < 0.1,
                        "rate={rate} size={size} window={window:?} db={db}"
                    );
                    assert!((s.bins[bin].measurement_dbfs as f64 - db + 6.020599913).abs() < 0.1);
                    assert!((s.reference_level.unwrap().peak_dbfs as f64 - db).abs() < 0.1);
                    assert!(
                        (s.reference_level.unwrap().rms_dbfs as f64 - db + 3.010299957).abs() < 0.1
                    );
                    assert_eq!(s.bin_count, size / 2 + 1);
                    assert!(
                        (s.config.bin_hz(rate) * bin as f64
                            - bin as f64 * rate as f64 / size as f64)
                            .abs()
                            < 1e-9
                    );
                }
            }
        }
    }
}
fn reference_db(
    size: usize,
    window: WindowFunction,
    bin: usize,
    signal: impl Fn(usize) -> f64,
) -> f64 {
    let mut re = 0.0;
    let mut im = 0.0;
    let mut sum = 0.0;
    for i in 0..size {
        let w = match window {
            WindowFunction::Hann => {
                0.5 * (1.0 - (std::f64::consts::TAU * i as f64 / (size - 1) as f64).cos())
            }
            WindowFunction::Rectangular => 1.0,
        };
        let phase = std::f64::consts::TAU * bin as f64 * i as f64 / size as f64;
        re += signal(i) * w * phase.cos();
        im -= signal(i) * w * phase.sin();
        sum += w;
    }
    (20.0
        * (re.hypot(im)
            * if bin == 0 || bin == size / 2 {
                1.0
            } else {
                2.0
            }
            / sum)
            .max(1e-6)
            .log10())
    .max(-120.0)
}
#[test]
fn a02_off_bin_multitone_and_noise_match_independent_f64_dft() {
    for size in [1024, 2048, 8192] {
        for window in [WindowFunction::Hann, WindowFunction::Rectangular] {
            let signal = |i: usize| {
                let phase = std::f64::consts::TAU * i as f64 / size as f64;
                0.25 * (32.4 * phase).sin()
                    + 0.1 * (80.0 * phase).cos()
                    + 0.0001
                        * ((i.wrapping_mul(1664525).wrapping_add(1013904223) % 65536) as f64
                            / 32768.0
                            - 1.0)
            };
            let s = analyze(
                AnalysisConfig {
                    fft_size: size,
                    window,
                    averaging_ms: 0,
                },
                48000,
                1,
                |i| (signal(i) as f32, signal(i) as f32),
            );
            for bin in [0, 31, 32, 33, 79, 80, 81, size / 2] {
                let expected = reference_db(size, window, bin, signal);
                assert!(
                    (s.bins[bin].measurement_dbfs as f64 - expected).abs() < 0.05,
                    "{size} {window:?} bin={bin}: {} vs {expected}",
                    s.bins[bin].measurement_dbfs
                );
            }
        }
    }
}
#[test]
fn a03_invalid_and_clipped_data_are_flagged_then_recover() {
    let config = AnalysisConfig::FAST;
    let mut analyzer = SpectrumAnalyzer::with_config(48000, config).unwrap();
    let mut latest = None;
    for index in 0..12 {
        let mut block = AudioBlock {
            start_frame: index * AUDIO_BLOCK_FRAMES as u64,
            valid_frames: AUDIO_BLOCK_FRAMES,
            has_reference: true,
            frames: [StereoFrame {
                reference: 0.5,
                measurement: 0.25,
            }; AUDIO_BLOCK_FRAMES],
            ..Default::default()
        };
        if index == 0 {
            block.frames[0].reference = f32::NAN;
            block.frames[1].measurement = f32::INFINITY;
            block.frames[2].measurement = 1.1;
        }
        if let Some(s) = analyzer.process_block(&block) {
            if index == 3 {
                assert!(s.invalid_samples > 0);
                assert!(s.measurement_clipped);
                assert!(!s.comparison_ready);
            }
            latest = Some(s);
        }
    }
    let s = latest.unwrap();
    assert_eq!(s.invalid_samples, 0);
    assert!(!s.measurement_clipped);
    assert!((s.bins[0].measurement_dbfs + 12.0412).abs() < 0.01);
}
#[test]
fn a04_gain_delay_and_coherence_require_averaging_warmup() {
    let config = AnalysisConfig {
        fft_size: 2048,
        window: WindowFunction::Hann,
        averaging_ms: 100,
    };
    let bin = 40;
    let signal = |i: usize| {
        let phase = std::f64::consts::TAU * bin as f64 / 2048.0;
        (
            (0.5 * (phase * i as f64).sin()) as f32,
            (0.25 * (phase * (i as f64 - 4.0)).sin()) as f32,
        )
    };
    assert!(!analyze(config, 48000, 1, signal).comparison_ready);
    let s = analyze(config, 48000, 12, signal);
    assert!(s.comparison_ready);
    assert!((s.bins[bin].transfer_db + 6.0206).abs() < 0.1);
    assert!((s.bins[bin].phase_degrees + 360.0 * bin as f32 * 4.0 / 2048.0).abs() < 1.0);
    assert!(s.bins[bin].coherence > 0.999);
    assert!(!analyze(AnalysisConfig::FAST, 48000, 12, signal).comparison_ready);
}

#[test]
fn result_pool_preserves_held_data_and_reports_exhaustion_without_allocating() {
    let mut analyzer = SpectrumAnalyzer::with_config(48000, AnalysisConfig::FAST).unwrap();
    let mut held = Vec::new();
    for index in 0..24 {
        let block = AudioBlock {
            start_frame: index * AUDIO_BLOCK_FRAMES as u64,
            valid_frames: AUDIO_BLOCK_FRAMES,
            frames: [StereoFrame {
                reference: 0.0,
                measurement: 0.5,
            }; AUDIO_BLOCK_FRAMES],
            ..Default::default()
        };
        if let Some(s) = analyzer.process_block(&block) {
            held.push(s);
        }
    }
    assert_eq!(held.len(), 8);
    assert!(analyzer.skipped_results() > 0);
    assert!((held[0].bins[0].measurement_dbfs + 6.0206).abs() < 0.01);
    held.clear();
    let mut recovered = None;
    for index in 24..28 {
        let block = AudioBlock {
            start_frame: index * AUDIO_BLOCK_FRAMES as u64,
            valid_frames: AUDIO_BLOCK_FRAMES,
            ..Default::default()
        };
        recovered = analyzer.process_block(&block).or(recovered);
    }
    assert_eq!(recovered.unwrap().measurement_level.rms_dbfs, -120.0);
    assert!(std::mem::size_of::<AnalysisSnapshot>() < 2048);
}

#[test]
fn clipping_invalidates_comparison_and_does_not_contaminate_recovery_average() {
    let config = AnalysisConfig {
        averaging_ms: 500,
        ..AnalysisConfig::FAST
    };
    let mut analyzer = SpectrumAnalyzer::with_config(48000, config).unwrap();
    let mut ready_before_clip = false;
    let mut clip_seen = false;
    let mut recovered = false;
    let mut history = audio_analyzer::history::HistoryBuffer::default();
    for index in 0..48 {
        let value = if index < 24 {
            0.25
        } else if index < 28 {
            1.1
        } else {
            0.125
        };
        let block = AudioBlock {
            start_frame: index * AUDIO_BLOCK_FRAMES as u64,
            valid_frames: AUDIO_BLOCK_FRAMES,
            has_reference: true,
            frames: [StereoFrame {
                reference: 0.5,
                measurement: value,
            }; AUDIO_BLOCK_FRAMES],
            ..Default::default()
        };
        if let Some(snapshot) = analyzer.process_block(&block) {
            history.push_at(&snapshot, 0, snapshot.window_start_frame as f64 / 48000.0);
            if index < 24 {
                ready_before_clip |= snapshot.comparison_ready;
            }
            if snapshot.measurement_clipped {
                assert!(!snapshot.comparison_ready);
                clip_seen = true;
            }
            if index >= 28 && !snapshot.measurement_clipped && snapshot.averaged_windows == 1 {
                assert!((snapshot.bins[0].measurement_dbfs + 18.0618).abs() < 0.02);
                assert!((snapshot.bins[0].transfer_db + 12.0412).abs() < 0.02);
                recovered = true;
            }
        }
    }
    assert!(ready_before_clip && clip_seen && recovered);
    assert!(
        history
            .points()
            .iter()
            .any(|point| point.measurement_clipped),
        "short clipping remains visible in history after recovery"
    );
}
