use std::collections::VecDeque;

use crate::model::AnalysisSnapshot;

const HISTORY_HZ: u64 = 10;
const HISTORY_CAPACITY: usize = 3_000;

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
            Self::Spectrum => "SPECTRUM",
            Self::Transfer => "TRANSFER",
            Self::Phase => "PHASE",
            Self::Coherence => "COHERENCE",
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
            let elapsed = (now - self.last_time).max(0.0) as f32;
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
    pub gap: bool,
}

#[derive(Default)]
pub struct HistoryBuffer {
    points: VecDeque<HistoryPoint>,
    last_frame: Option<u64>,
    last_discontinuities: u64,
}

impl HistoryBuffer {
    pub fn push(&mut self, snapshot: &AnalysisSnapshot, discontinuities: u64) {
        let interval = (snapshot.sample_rate as u64 / HISTORY_HZ).max(1);
        if self
            .last_frame
            .is_some_and(|last| snapshot.window_start_frame.saturating_sub(last) < interval)
        {
            return;
        }
        let gap = discontinuities != self.last_discontinuities;
        self.points.push_back(HistoryPoint {
            seconds: snapshot.window_start_frame as f64 / snapshot.sample_rate as f64,
            measurement_rms: snapshot.measurement_level.rms_dbfs,
            reference_rms: snapshot.reference_level.map(|level| level.rms_dbfs),
            gap,
        });
        self.last_frame = Some(snapshot.window_start_frame);
        self.last_discontinuities = discontinuities;
        while self.points.len() > HISTORY_CAPACITY {
            self.points.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.last_frame = None;
    }

    pub fn points(&self) -> &VecDeque<HistoryPoint> {
        &self.points
    }
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
    fn peak_hold_waits_then_releases_at_twenty_four_db_per_second() {
        let mut hold = PeakHold::default();
        assert_eq!(hold.update(-3.0, 1.0), -3.0);
        assert_eq!(hold.update(-20.0, 1.4), -3.0);
        let released = hold.update(-20.0, 1.9);
        assert!((released + 15.0).abs() < 0.01);
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
