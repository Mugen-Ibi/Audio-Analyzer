use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::model::{
    AnalysisSnapshot, AudioBlock, BinMetrics, FFT_SIZE, HOP_SIZE, SPECTRUM_BINS, SignalLevel,
    StereoFrame, VECTOR_SCOPE_POINTS,
};

const AVERAGING_ALPHA: f32 = 0.2;
const MIN_DB: f32 = -120.0;
const POWER_EPSILON: f32 = 1.0e-20;

pub struct SpectrumAnalyzer {
    sample_rate: u32,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    coherent_gain: f32,
    samples: Vec<StereoFrame>,
    reference_fft: Vec<Complex<f32>>,
    measurement_fft: Vec<Complex<f32>>,
    averaged_reference_power: Vec<f32>,
    averaged_measurement_power: Vec<f32>,
    averaged_cross_power: Vec<Complex<f32>>,
    averaging_initialized: bool,
    expected_input_frame: Option<u64>,
    window_start_frame: u64,
    route_generation: Option<u64>,
    sequence: u64,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|index| {
                0.5 * (1.0
                    - (2.0 * std::f32::consts::PI * index as f32 / (FFT_SIZE - 1) as f32).cos())
            })
            .collect();
        let coherent_gain = window.iter().sum::<f32>() / FFT_SIZE as f32;

        Self {
            sample_rate,
            fft,
            window,
            coherent_gain,
            samples: Vec::with_capacity(FFT_SIZE),
            reference_fft: vec![Complex::default(); FFT_SIZE],
            measurement_fft: vec![Complex::default(); FFT_SIZE],
            averaged_reference_power: vec![0.0; SPECTRUM_BINS],
            averaged_measurement_power: vec![0.0; SPECTRUM_BINS],
            averaged_cross_power: vec![Complex::default(); SPECTRUM_BINS],
            averaging_initialized: false,
            expected_input_frame: None,
            window_start_frame: 0,
            route_generation: None,
            sequence: 0,
        }
    }

    pub fn process_block(&mut self, block: &AudioBlock) -> Option<AnalysisSnapshot> {
        let input_discontinuity = self
            .expected_input_frame
            .is_some_and(|expected| expected != block.start_frame);
        let route_changed = self
            .route_generation
            .is_some_and(|generation| generation != block.route_generation);

        if input_discontinuity || route_changed {
            self.reset_history();
        }

        self.route_generation = Some(block.route_generation);
        self.expected_input_frame = Some(block.start_frame + block.valid_frames as u64);

        let mut newest = None;
        for (offset, frame) in block.frames[..block.valid_frames].iter().enumerate() {
            if self.samples.is_empty() {
                self.window_start_frame = block.start_frame + offset as u64;
            }
            self.samples.push(*frame);

            if self.samples.len() == FFT_SIZE {
                newest = Some(self.analyze(block.has_reference));
                self.samples.copy_within(HOP_SIZE..FFT_SIZE, 0);
                self.samples.truncate(FFT_SIZE - HOP_SIZE);
                self.window_start_frame += HOP_SIZE as u64;
            }
        }
        newest
    }

    pub fn reset_history(&mut self) {
        self.samples.clear();
        self.averaging_initialized = false;
        self.expected_input_frame = None;
        self.averaged_reference_power.fill(0.0);
        self.averaged_measurement_power.fill(0.0);
        self.averaged_cross_power.fill(Complex::default());
    }

    fn analyze(&mut self, has_reference: bool) -> AnalysisSnapshot {
        for index in 0..FFT_SIZE {
            let frame = self.samples[index];
            let window = self.window[index];
            self.reference_fft[index] = Complex::new(frame.reference * window, 0.0);
            self.measurement_fft[index] = Complex::new(frame.measurement * window, 0.0);
        }

        self.fft.process(&mut self.reference_fft);
        self.fft.process(&mut self.measurement_fft);

        let mut snapshot = AnalysisSnapshot {
            sequence: self.sequence,
            window_start_frame: self.window_start_frame,
            sample_rate: self.sample_rate,
            has_reference,
            measurement_level: signal_level(self.samples.iter().map(|frame| frame.measurement)),
            reference_level: has_reference
                .then(|| signal_level(self.samples.iter().map(|frame| frame.reference))),
            phase_correlation: has_reference.then(|| phase_correlation(&self.samples)),
            ..AnalysisSnapshot::default()
        };
        self.sequence = self.sequence.wrapping_add(1);

        if has_reference {
            let stride = FFT_SIZE / VECTOR_SCOPE_POINTS;
            for (scope_index, sample_index) in (0..FFT_SIZE)
                .step_by(stride)
                .take(VECTOR_SCOPE_POINTS)
                .enumerate()
            {
                snapshot.scope_points[scope_index] = self.samples[sample_index];
                snapshot.scope_len += 1;
            }
        }

        for bin in 0..SPECTRUM_BINS {
            let reference = self.reference_fft[bin];
            let measurement = self.measurement_fft[bin];
            let reference_power = reference.norm_sqr();
            let measurement_power = measurement.norm_sqr();
            let cross_power = measurement * reference.conj();

            if !self.averaging_initialized {
                self.averaged_reference_power[bin] = reference_power;
                self.averaged_measurement_power[bin] = measurement_power;
                self.averaged_cross_power[bin] = cross_power;
            } else {
                self.averaged_reference_power[bin] =
                    blend(self.averaged_reference_power[bin], reference_power);
                self.averaged_measurement_power[bin] =
                    blend(self.averaged_measurement_power[bin], measurement_power);
                self.averaged_cross_power[bin] =
                    blend_complex(self.averaged_cross_power[bin], cross_power);
            }

            let one_sided_factor = if bin == 0 || bin == FFT_SIZE / 2 {
                1.0
            } else {
                2.0
            };
            let reference_amplitude =
                reference.norm() * one_sided_factor / (FFT_SIZE as f32 * self.coherent_gain);
            let amplitude =
                measurement.norm() * one_sided_factor / (FFT_SIZE as f32 * self.coherent_gain);
            let measurement_dbfs = amplitude_to_db(amplitude);

            let mut metrics = BinMetrics {
                reference_dbfs: amplitude_to_db(reference_amplitude),
                measurement_dbfs,
                transfer_db: MIN_DB,
                phase_degrees: 0.0,
                coherence: 0.0,
            };

            if has_reference && self.averaged_reference_power[bin] > POWER_EPSILON {
                let transfer = self.averaged_cross_power[bin] / self.averaged_reference_power[bin];
                metrics.transfer_db = amplitude_to_db(transfer.norm());
                metrics.phase_degrees = transfer.arg().to_degrees();
                let denominator =
                    self.averaged_reference_power[bin] * self.averaged_measurement_power[bin];
                if denominator > POWER_EPSILON {
                    metrics.coherence =
                        (self.averaged_cross_power[bin].norm_sqr() / denominator).clamp(0.0, 1.0);
                }
            }
            snapshot.bins[bin] = metrics;
        }
        self.averaging_initialized = true;
        snapshot
    }
}

fn blend(previous: f32, current: f32) -> f32 {
    previous + AVERAGING_ALPHA * (current - previous)
}

fn blend_complex(previous: Complex<f32>, current: Complex<f32>) -> Complex<f32> {
    previous + (current - previous) * AVERAGING_ALPHA
}

fn amplitude_to_db(amplitude: f32) -> f32 {
    20.0 * amplitude.max(10.0_f32.powf(MIN_DB / 20.0)).log10()
}

fn signal_level(samples: impl Iterator<Item = f32>) -> SignalLevel {
    let mut peak = 0.0_f32;
    let mut sum_squares = 0.0_f32;
    let mut count = 0_usize;
    for sample in samples {
        peak = peak.max(sample.abs());
        sum_squares += sample * sample;
        count += 1;
    }
    let rms = if count == 0 {
        0.0
    } else {
        (sum_squares / count as f32).sqrt()
    };
    SignalLevel {
        peak_dbfs: amplitude_to_db(peak),
        rms_dbfs: amplitude_to_db(rms),
    }
}

fn phase_correlation(samples: &[StereoFrame]) -> f32 {
    let mut cross = 0.0_f32;
    let mut reference_power = 0.0_f32;
    let mut measurement_power = 0.0_f32;
    for frame in samples {
        cross += frame.reference * frame.measurement;
        reference_power += frame.reference * frame.reference;
        measurement_power += frame.measurement * frame.measurement;
    }
    let denominator = (reference_power * measurement_power).sqrt();
    if denominator <= POWER_EPSILON {
        0.0
    } else {
        (cross / denominator).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AUDIO_BLOCK_FRAMES, AudioBlock, StereoFrame};

    fn analyze_signals(
        sample_rate: u32,
        reference: impl Fn(usize) -> f32,
        measurement: impl Fn(usize) -> f32,
    ) -> AnalysisSnapshot {
        let mut analyzer = SpectrumAnalyzer::new(sample_rate);
        let mut snapshot = None;
        for block_index in 0..(FFT_SIZE / AUDIO_BLOCK_FRAMES) {
            let mut block = AudioBlock {
                start_frame: (block_index * AUDIO_BLOCK_FRAMES) as u64,
                valid_frames: AUDIO_BLOCK_FRAMES,
                has_reference: true,
                ..AudioBlock::default()
            };
            for frame_index in 0..AUDIO_BLOCK_FRAMES {
                let index = block_index * AUDIO_BLOCK_FRAMES + frame_index;
                block.frames[frame_index] = StereoFrame {
                    reference: reference(index),
                    measurement: measurement(index),
                };
            }
            snapshot = analyzer.process_block(&block).or(snapshot);
        }
        snapshot.expect("one FFT frame should be produced")
    }

    #[test]
    fn hann_window_reports_bin_centered_full_scale_tone_at_zero_dbfs() {
        let sample_rate = 48_000;
        let bin = 64;
        let tone = |index: usize| {
            (2.0 * std::f32::consts::PI * bin as f32 * index as f32 / FFT_SIZE as f32).sin()
        };
        let snapshot = analyze_signals(sample_rate, tone, tone);
        assert!(snapshot.bins[bin].measurement_dbfs.abs() < 0.05);
        assert!(snapshot.bins[bin].reference_dbfs.abs() < 0.05);
        assert!(snapshot.measurement_level.peak_dbfs.abs() < 0.01);
        assert!((snapshot.measurement_level.rms_dbfs + 3.0103).abs() < 0.01);
    }

    #[test]
    fn transfer_function_reports_half_gain_with_unit_coherence() {
        let bin = 80;
        let tone = |index: usize| {
            (2.0 * std::f32::consts::PI * bin as f32 * index as f32 / FFT_SIZE as f32).sin()
        };
        let snapshot = analyze_signals(48_000, tone, |index| 0.5 * tone(index));
        assert!((snapshot.bins[bin].transfer_db + 6.0206).abs() < 0.05);
        assert!(snapshot.bins[bin].coherence > 0.999);
    }

    #[test]
    fn transfer_phase_matches_a_known_sample_delay() {
        let bin = 40;
        let delay = 4.0;
        let phase = |index: f32| 2.0 * std::f32::consts::PI * bin as f32 * index / FFT_SIZE as f32;
        let snapshot = analyze_signals(
            48_000,
            |index| phase(index as f32).sin(),
            |index| phase(index as f32 - delay).sin(),
        );
        let expected_degrees = -360.0 * bin as f32 * delay / FFT_SIZE as f32;
        assert!((snapshot.bins[bin].phase_degrees - expected_degrees).abs() < 0.05);
    }

    #[test]
    fn a_frame_gap_resets_the_overlap_history() {
        let mut analyzer = SpectrumAnalyzer::new(48_000);
        let mut block = AudioBlock {
            valid_frames: AUDIO_BLOCK_FRAMES,
            ..AudioBlock::default()
        };
        assert!(analyzer.process_block(&block).is_none());
        block.start_frame = (AUDIO_BLOCK_FRAMES * 2) as u64;
        assert!(analyzer.process_block(&block).is_none());
        assert_eq!(analyzer.samples.len(), AUDIO_BLOCK_FRAMES);
    }

    #[test]
    fn correlation_reports_in_phase_opposite_and_quadrature_signals() {
        let bin = 32;
        let tone = |index: usize| {
            (2.0 * std::f32::consts::PI * bin as f32 * index as f32 / FFT_SIZE as f32).sin()
        };
        let in_phase = analyze_signals(48_000, tone, tone);
        let opposite = analyze_signals(48_000, tone, |index| -tone(index));
        let quadrature = analyze_signals(48_000, tone, |index| {
            (2.0 * std::f32::consts::PI * bin as f32 * index as f32 / FFT_SIZE as f32).cos()
        });
        assert!(in_phase.phase_correlation.unwrap() > 0.999);
        assert!(opposite.phase_correlation.unwrap() < -0.999);
        assert!(quadrature.phase_correlation.unwrap().abs() < 0.001);
    }

    #[test]
    fn vector_scope_preserves_reference_measurement_pairs() {
        let snapshot = analyze_signals(
            48_000,
            |index| index as f32 / FFT_SIZE as f32,
            |index| -(index as f32) / FFT_SIZE as f32,
        );
        assert_eq!(snapshot.scope_len, VECTOR_SCOPE_POINTS);
        let stride = FFT_SIZE / VECTOR_SCOPE_POINTS;
        let point = snapshot.scope_points[42];
        let expected = (42 * stride) as f32 / FFT_SIZE as f32;
        assert!((point.reference - expected).abs() < f32::EPSILON);
        assert!((point.measurement + expected).abs() < f32::EPSILON);
    }

    #[test]
    fn mono_analysis_omits_reference_only_metrics() {
        let mut analyzer = SpectrumAnalyzer::new(48_000);
        let mut snapshot = None;
        for block_index in 0..(FFT_SIZE / AUDIO_BLOCK_FRAMES) {
            let block = AudioBlock {
                start_frame: (block_index * AUDIO_BLOCK_FRAMES) as u64,
                valid_frames: AUDIO_BLOCK_FRAMES,
                has_reference: false,
                ..AudioBlock::default()
            };
            snapshot = analyzer.process_block(&block).or(snapshot);
        }
        let snapshot = snapshot.unwrap();
        assert!(snapshot.reference_level.is_none());
        assert!(snapshot.phase_correlation.is_none());
        assert_eq!(snapshot.scope_len, 0);
    }
}
