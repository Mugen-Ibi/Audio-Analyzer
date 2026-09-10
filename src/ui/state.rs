use std::collections::VecDeque;

use crate::model::{AnalysisSnapshot, FFT_SIZE, SPECTRUM_BINS};

const HISTORY_HZ: u64 = 10;
const HISTORY_CAPACITY: usize = 3_000;
pub const HISTORY_SPECTRUM_BANDS: usize = 96;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryView {
    #[default]
    Level,
    Spectrum,
}

impl HistoryView {
    pub const ALL: [Self; 2] = [Self::Level, Self::Spectrum];

    pub fn label(self) -> &'static str {
        match self {
            Self::Level => "音量 / LEVEL",
            Self::Spectrum => "周波数 / SPECTRUM",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisView {
    Spectrum,
    Transfer,
    Phase,
    Coherence,
}

#[derive(Default)]
pub struct DisplayHold {
    active: bool,
}

impl DisplayHold {
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
    }

    pub fn accepts_plot_update(&self) -> bool {
        !self.active
    }
}

impl AnalysisView {
    pub const ALL: [Self; 4] = [Self::Spectrum, Self::Transfer, Self::Phase, Self::Coherence];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spectrum => "周波数",
            Self::Transfer => "伝達関数",
            Self::Phase => "位相",
            Self::Coherence => "コヒーレンス",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PeakHold {
    held_dbfs: f32,
    hold_until: f64,
    last_time: f64,
}

impl Default for PeakHold {
    fn default() -> Self {
        Self {
            held_dbfs: -120.0,
            hold_until: 0.0,
            last_time: 0.0,
        }
    }
}

impl PeakHold {
    pub fn update(&mut self, peak_dbfs: f32, now: f64) -> f32 {
        if peak_dbfs >= self.held_dbfs {
            self.held_dbfs = peak_dbfs;
            self.hold_until = now + 0.5;
        } else if now > self.hold_until {
            let elapsed = (now - self.last_time.max(self.hold_until)).max(0.0) as f32;
            self.held_dbfs = (self.held_dbfs - 24.0 * elapsed).max(peak_dbfs);
        }
        self.last_time = now;
        self.held_dbfs
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HistoryPoint {
    pub seconds: f64,
    pub measurement_rms: f32,
    pub reference_rms: Option<f32>,
    pub spectrum_dbfs: [f32; HISTORY_SPECTRUM_BANDS],
    pub gap: bool,
}

#[derive(Default)]
pub struct HistoryBuffer {
    points: VecDeque<HistoryPoint>,
    last_frame: Option<u64>,
    last_discontinuities: u64,
}

impl HistoryBuffer {
    pub fn push(&mut self, snapshot: &AnalysisSnapshot, discontinuities: u64) -> bool {
        if snapshot.sample_rate == 0 {
            return false;
        }
        if self
            .last_frame
            .is_some_and(|last| snapshot.window_start_frame < last)
        {
            self.clear();
        }
        let interval = (snapshot.sample_rate as u64 / HISTORY_HZ).max(1);
        if self
            .last_frame
            .is_some_and(|last| snapshot.window_start_frame.saturating_sub(last) < interval)
        {
            return false;
        }
        let gap = discontinuities != self.last_discontinuities;
        self.points.push_back(HistoryPoint {
            seconds: snapshot.window_start_frame as f64 / snapshot.sample_rate as f64,
            measurement_rms: snapshot.measurement_level.rms_dbfs,
            reference_rms: snapshot.reference_level.map(|level| level.rms_dbfs),
            spectrum_dbfs: history_spectrum(snapshot),
            gap,
        });
        self.last_frame = Some(snapshot.window_start_frame);
        self.last_discontinuities = discontinuities;
        while self.points.len() > HISTORY_CAPACITY {
            self.points.pop_front();
        }
        true
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.last_frame = None;
        self.last_discontinuities = 0;
    }

    pub fn points(&self) -> &VecDeque<HistoryPoint> {
        &self.points
    }
}

fn history_spectrum(snapshot: &AnalysisSnapshot) -> [f32; HISTORY_SPECTRUM_BANDS] {
    let mut levels = [-120.0; HISTORY_SPECTRUM_BANDS];
    if snapshot.sample_rate == 0 {
        return levels;
    }

    let min_frequency = 20.0_f32;
    let nyquist = snapshot.sample_rate as f32 * 0.5;
    let resolution = snapshot.sample_rate as f32 / FFT_SIZE as f32;
    let log_span = (nyquist / min_frequency).ln();
    if log_span <= 0.0 {
        return levels;
    }
    for (bin, metrics) in snapshot.bins.iter().enumerate().take(SPECTRUM_BINS).skip(1) {
        let frequency = bin as f32 * resolution;
        if frequency < min_frequency {
            continue;
        }
        let band = ((frequency / min_frequency).ln() / log_span
            * (HISTORY_SPECTRUM_BANDS - 1) as f32)
            .round() as usize;
        let level = &mut levels[band.min(HISTORY_SPECTRUM_BANDS - 1)];
        *level = level.max(metrics.measurement_dbfs);
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnalysisSnapshot, SignalLevel};

    fn snapshot(frame: u64) -> AnalysisSnapshot {
        AnalysisSnapshot {
            window_start_frame: frame,
            sample_rate: 48_000,
            measurement_level: SignalLevel {
                peak_dbfs: -3.0,
                rms_dbfs: -6.0,
            },
            ..AnalysisSnapshot::default()
        }
    }

    #[test]
    fn history_keeps_every_audible_fft_bin_peak() {
        for rate in [48_000, 96_000, 192_000] {
            for bin in 1..SPECTRUM_BINS {
                let mut input = snapshot(0);
                input.sample_rate = rate;
                for metrics in &mut input.bins {
                    metrics.measurement_dbfs = -120.0;
                }
                input.bins[bin].measurement_dbfs = -3.0;
                assert_eq!(
                    history_spectrum(&input).into_iter().fold(-120.0, f32::max),
                    -3.0
                );
            }
        }
    }

    #[test]
    fn history_rejects_zero_rate_and_recovers_after_frame_reset() {
        let mut history = HistoryBuffer::default();
        assert!(!history.push(&AnalysisSnapshot::default(), 0));
        assert!(history.push(&snapshot(48_000), 0));
        assert!(history.push(&snapshot(0), 0));
        assert_eq!(history.points().len(), 1);
        assert!(history.points()[0].seconds.is_finite());
    }

    #[test]
    fn peak_hold_does_not_release_during_hold_when_updates_are_sparse() {
        let mut hold = PeakHold::default();
        hold.update(-3.0, 1.0);
        assert!((hold.update(-60.0, 1.6) + 5.4).abs() < 0.01);
    }

    #[test]
    fn peak_hold_waits_then_releases_at_twenty_four_db_per_second() {
        let mut hold = PeakHold::default();
        assert_eq!(hold.update(-3.0, 1.0), -3.0);
        assert_eq!(hold.update(-20.0, 1.4), -3.0);
        let released = hold.update(-20.0, 1.9);
        assert!((released + 12.6).abs() < 0.01);
    }

    #[test]
    fn history_downsamples_to_ten_hertz_and_marks_gaps() {
        let mut history = HistoryBuffer::default();
        history.push(&snapshot(0), 0);
        history.push(&snapshot(1_000), 0);
        history.push(&snapshot(4_800), 0);
        history.push(&snapshot(9_600), 1);
        assert_eq!(history.points().len(), 3);
        assert!(!history.points()[1].gap);
        assert!(history.points()[2].gap);
    }

    #[test]
    fn history_records_log_spaced_frequency_levels() {
        let mut input = snapshot(0);
        for metrics in &mut input.bins {
            metrics.measurement_dbfs = -120.0;
        }
        let band = HISTORY_SPECTRUM_BANDS / 2;
        let fraction = band as f32 / (HISTORY_SPECTRUM_BANDS - 1) as f32;
        let frequency = 20.0 * ((input.sample_rate as f32 * 0.5 / 20.0).ln() * fraction).exp();
        let bin = (frequency / (input.sample_rate as f32 / FFT_SIZE as f32)).round() as usize;
        input.bins[bin].measurement_dbfs = -7.0;

        let mut history = HistoryBuffer::default();
        assert!(history.push(&input, 0));
        let peak = history.points()[0]
            .spectrum_dbfs
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(peak, -7.0);
    }

    #[test]
    fn history_never_exceeds_three_thousand_points() {
        let mut history = HistoryBuffer::default();
        for index in 0..3_100_u64 {
            history.push(&snapshot(index * 4_800), 0);
        }
        assert_eq!(history.points().len(), HISTORY_CAPACITY);
    }

    #[test]
    fn display_hold_freezes_only_plot_updates() {
        let mut hold = DisplayHold::default();
        assert!(hold.accepts_plot_update());
        hold.toggle();
        assert!(hold.active());
        assert!(!hold.accepts_plot_update());
    }
}
