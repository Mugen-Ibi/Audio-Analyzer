use std::collections::VecDeque;

use crate::model::AnalysisSnapshot;

const HISTORY_HZ: u64 = 10;
const HISTORY_CAPACITY: usize = 6_001;
pub const HISTORY_SPECTRUM_BANDS: usize = 96;
pub const HISTORY_SECONDS: f64 = 600.0;
pub const DEFAULT_NO_SIGNAL_THRESHOLD_DBFS: f32 = -90.0;

const SIGNAL_HYSTERESIS_DB: f32 = 6.0;
const SIGNAL_RELEASE_SECONDS: f64 = 0.5;
const HISTORY_ZOOM_LEVELS: [f64; 6] = [600.0, 300.0, 120.0, 60.0, 30.0, 10.0];

#[derive(Clone, Copy, Debug, Default)]
pub struct SignalGate {
    active: bool,
    below_since: Option<f64>,
}

impl SignalGate {
    pub fn update(&mut self, rms_dbfs: f32, now: f64, threshold_dbfs: f32) -> bool {
        if self.active {
            if rms_dbfs < threshold_dbfs {
                let below_since = self.below_since.get_or_insert(now);
                if now - *below_since >= SIGNAL_RELEASE_SECONDS {
                    self.active = false;
                    self.below_since = None;
                }
            } else {
                self.below_since = None;
            }
        } else if rms_dbfs >= threshold_dbfs + SIGNAL_HYSTERESIS_DB {
            self.active = true;
            self.below_since = None;
        }
        self.active
    }

    pub fn active(&self) -> bool {
        self.active
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HistoryViewport {
    zoom_index: usize,
    offset_from_live: f64,
}

impl Default for HistoryViewport {
    fn default() -> Self {
        Self {
            zoom_index: 3,
            offset_from_live: 0.0,
        }
    }
}

impl HistoryViewport {
    pub fn visible_seconds(self) -> f64 {
        HISTORY_ZOOM_LEVELS[self.zoom_index]
    }

    pub fn offset_from_live(self) -> f64 {
        self.offset_from_live
    }

    pub fn zoom_in(&mut self) {
        if self.zoom_index + 1 < HISTORY_ZOOM_LEVELS.len() {
            self.zoom_index += 1;
            self.clamp_offset();
        }
    }

    pub fn zoom_out(&mut self) {
        self.zoom_index = self.zoom_index.saturating_sub(1);
        self.clamp_offset();
    }

    pub fn pan_older(&mut self) {
        self.offset_from_live += self.visible_seconds() * 0.5;
        self.clamp_offset();
    }

    pub fn pan_newer(&mut self) {
        self.offset_from_live = (self.offset_from_live - self.visible_seconds() * 0.5).max(0.0);
    }

    pub fn jump_live(&mut self) {
        self.offset_from_live = 0.0;
    }

    pub fn time_bounds(self, live_seconds: f64) -> (f64, f64) {
        let end = live_seconds - self.offset_from_live;
        (end - self.visible_seconds(), end)
    }

    fn clamp_offset(&mut self) {
        self.offset_from_live = self
            .offset_from_live
            .clamp(0.0, HISTORY_SECONDS - self.visible_seconds());
    }
}

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
    pub measurement_clipped: bool,
    pub reference_clipped: bool,
    pub seconds: f64,
    pub measurement_rms: f32,
    pub reference_rms: Option<f32>,
    pub spectrum_dbfs: [f32; HISTORY_SPECTRUM_BANDS],
    pub gap: bool,
    pub reference_spectrum_dbfs: Option<[f32; HISTORY_SPECTRUM_BANDS]>,
    pub segment: u64,
    pub config: crate::config::AnalysisConfig,
    pub sample_rate: u32,
}

/// Match the cursor and renderer to the same measured interval. Small timing
/// variations from FFT hops are continuous; missing/changed conditions never are.
pub fn history_interval_end(point: &HistoryPoint, next: Option<&HistoryPoint>) -> f64 {
    if point.gap {
        return point.seconds;
    }
    if let Some(next) = next
        && !next.gap
        && next.segment == point.segment
        && next.seconds > point.seconds
        && next.seconds - point.seconds <= history_max_spacing(point.config, point.sample_rate)
    {
        return next.seconds;
    }
    point.seconds + 0.1
}

fn history_max_spacing(config: crate::config::AnalysisConfig, sample_rate: u32) -> f64 {
    // The first available FFT after the 10 Hz deadline can be one hop later.
    // In precise mode a legitimate record interval can therefore exceed 150 ms.
    0.1 + config.hop() as f64 / sample_rate.max(1) as f64 + 1e-6
}

pub struct HistoryBuffer {
    points: VecDeque<HistoryPoint>,
    last_frame: Option<u64>,
    last_discontinuities: u64,
    retention_seconds: f64,
    segment: u64,
    boundary: bool,
    labels: VecDeque<(u64, String)>,
}

impl Default for HistoryBuffer {
    fn default() -> Self {
        Self {
            points: VecDeque::new(),
            last_frame: None,
            last_discontinuities: 0,
            retention_seconds: 600.0,
            segment: 0,
            boundary: false,
            labels: VecDeque::new(),
        }
    }
}
impl HistoryBuffer {
    pub fn begin_segment(&mut self, label: String) {
        // A slider can change conditions repeatedly before one complete window
        // arrives. Such empty segments must not evict metadata for real results.
        if self.boundary {
            self.labels.pop_back();
        }
        self.segment = self.segment.wrapping_add(1);
        self.labels.push_back((self.segment, label));
        while self.labels.len() > HISTORY_CAPACITY {
            self.labels.pop_front();
        }
        self.last_frame = None;
        self.last_discontinuities = 0;
        self.boundary = true;
    }
    pub fn label(&self, segment: u64) -> &str {
        self.labels
            .iter()
            .find(|(id, _)| *id == segment)
            .map_or("条件不明（保持範囲外）", |(_, s)| s.as_str())
    }
    pub fn set_retention(&mut self, seconds: u32) {
        self.retention_seconds = seconds.clamp(60, 600) as f64;
    }
    pub fn expire(&mut self, now: f64) {
        while self
            .points
            .front()
            .is_some_and(|p| p.seconds < now - self.retention_seconds)
        {
            self.points.pop_front();
        }
        while self.labels.len() > 1
            && self
                .points
                .front()
                .is_none_or(|p| self.labels[1].0 <= p.segment)
        {
            self.labels.pop_front();
        }
    }
    pub fn push(&mut self, snapshot: &AnalysisSnapshot, discontinuities: u64) -> bool {
        if self
            .last_frame
            .is_some_and(|last| snapshot.window_start_frame < last)
        {
            self.clear();
        }
        let seconds = snapshot.window_start_frame as f64 / snapshot.sample_rate.max(1) as f64;
        self.push_at(snapshot, discontinuities, seconds)
    }
    pub fn push_at(
        &mut self,
        snapshot: &AnalysisSnapshot,
        discontinuities: u64,
        seconds: f64,
    ) -> bool {
        if snapshot.sample_rate == 0 || !seconds.is_finite() {
            return false;
        }
        let interval = (snapshot.sample_rate as u64 / HISTORY_HZ).max(1);
        let discontinuity = discontinuities != self.last_discontinuities;
        let clip_changed = self.points.back().is_some_and(|point| {
            point.measurement_clipped != snapshot.measurement_clipped
                || point.reference_clipped != snapshot.reference_clipped
        });
        if !discontinuity
            && !clip_changed
            && !self.boundary
            && snapshot.invalid_samples == 0
            && self
                .last_frame
                .is_some_and(|last| snapshot.window_start_frame.saturating_sub(last) < interval)
        {
            return false;
        }
        let gap = self.boundary
            || discontinuity
            || snapshot.invalid_samples > 0
            || self.points.back().is_some_and(|last| {
                seconds - last.seconds > history_max_spacing(snapshot.config, snapshot.sample_rate)
            });
        self.points.push_back(HistoryPoint {
            measurement_clipped: snapshot.measurement_clipped,
            reference_clipped: snapshot.reference_clipped,
            seconds,
            measurement_rms: snapshot.measurement_level.rms_dbfs,
            reference_rms: snapshot.reference_level.map(|l| l.rms_dbfs),
            spectrum_dbfs: history_spectrum(snapshot, false),
            reference_spectrum_dbfs: snapshot
                .has_reference
                .then(|| history_spectrum(snapshot, true)),
            gap,
            segment: self.segment,
            config: snapshot.config,
            sample_rate: snapshot.sample_rate,
        });
        self.last_frame = Some(snapshot.window_start_frame);
        self.last_discontinuities = discontinuities;
        self.boundary = false;
        self.expire(seconds);
        while self.points.len() > HISTORY_CAPACITY {
            self.points.pop_front();
        }
        true
    }
    pub fn clear(&mut self) {
        *self = Self {
            retention_seconds: self.retention_seconds,
            ..Self::default()
        };
    }
    pub fn points(&self) -> &VecDeque<HistoryPoint> {
        &self.points
    }
    pub fn at(&self, seconds: f64) -> Option<&HistoryPoint> {
        self.points
            .iter()
            .enumerate()
            .find(|(index, p)| {
                !p.gap
                    && p.seconds <= seconds
                    && seconds < history_interval_end(p, self.points.get(index + 1))
            })
            .map(|(_, p)| p)
    }
}

pub fn history_spectrum(
    snapshot: &AnalysisSnapshot,
    reference: bool,
) -> [f32; HISTORY_SPECTRUM_BANDS] {
    let mut levels = [-120.0; HISTORY_SPECTRUM_BANDS];
    if snapshot.sample_rate == 0 {
        return levels;
    }

    let min_frequency = 20.0_f32;
    let nyquist = 20_000.0_f32;
    let resolution = snapshot.sample_rate as f32 / snapshot.config.fft_size as f32;
    let log_span = (nyquist / min_frequency).ln();
    if log_span <= 0.0 {
        return levels;
    }
    for (bin, metrics) in snapshot
        .bins
        .iter()
        .enumerate()
        .take(snapshot.bin_count)
        .skip(1)
    {
        let frequency = bin as f32 * resolution;
        if frequency < min_frequency || frequency > nyquist {
            continue;
        }
        let band = ((frequency / min_frequency).ln() / log_span
            * (HISTORY_SPECTRUM_BANDS - 1) as f32)
            .round() as usize;
        let level = &mut levels[band.min(HISTORY_SPECTRUM_BANDS - 1)];
        *level = level.max(if reference {
            metrics.reference_dbfs
        } else {
            metrics.measurement_dbfs
        });
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnalysisSnapshot, FFT_SIZE, SPECTRUM_BINS, SignalLevel};

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
                if bin as f32 * rate as f32 / FFT_SIZE as f32 > 20_000.0 {
                    continue;
                }
                let mut input = snapshot(0);
                input.sample_rate = rate;
                for metrics in input.bins.iter_mut() {
                    metrics.measurement_dbfs = -120.0;
                }
                input.bins[bin].measurement_dbfs = -3.0;
                assert_eq!(
                    history_spectrum(&input, false)
                        .into_iter()
                        .fold(-120.0, f32::max),
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
        for metrics in input.bins.iter_mut() {
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
    fn history_records_fit_the_bounded_capacity() {
        let mut history = HistoryBuffer::default();
        for index in 0..3_100_u64 {
            history.push(&snapshot(index * 4_800), 0);
        }
        assert!(history.points().len() <= HISTORY_CAPACITY);
    }

    #[test]
    fn display_hold_freezes_only_plot_updates() {
        let mut hold = DisplayHold::default();
        assert!(hold.accepts_plot_update());
        hold.toggle();
        assert!(hold.active());
        assert!(!hold.accepts_plot_update());
    }

    #[test]
    fn signal_gate_uses_hysteresis_and_release_delay() {
        let mut gate = SignalGate::default();
        assert!(!gate.update(-90.0, 0.0, -90.0));
        assert!(gate.update(-84.0, 0.1, -90.0));
        assert!(gate.update(-100.0, 0.2, -90.0));
        assert!(gate.update(-100.0, 0.69, -90.0));
        assert!(!gate.update(-100.0, 0.71, -90.0));
    }

    #[test]
    fn history_viewport_zooms_and_pans_within_fixed_window() {
        let mut viewport = HistoryViewport::default();
        assert_eq!(viewport.visible_seconds(), 60.0);
        assert_eq!(viewport.time_bounds(100.0), (40.0, 100.0));
        viewport.zoom_in();
        assert_eq!(viewport.visible_seconds(), 30.0);
        viewport.pan_older();
        assert_eq!(viewport.time_bounds(100.0), (55.0, 85.0));
        viewport.jump_live();
        assert_eq!(viewport.offset_from_live(), 0.0);
    }
}
