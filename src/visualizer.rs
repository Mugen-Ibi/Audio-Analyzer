//! Shared, bounded display geometry. Analysis and retained history stay untouched.
use crate::{
    config::AnalysisConfig,
    history::{HISTORY_SPECTRUM_BANDS, HistoryPoint},
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::Path};

pub const MAX_SLICES: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    #[default]
    Waterfall,
    Heatmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.32,
            pitch: 0.58,
            zoom: 1.0,
        }
    }
}
impl Camera {
    pub fn valid(self) -> bool {
        self.yaw.is_finite()
            && (-1.0..=1.0).contains(&self.yaw)
            && self.pitch.is_finite()
            && (0.25..=1.2).contains(&self.pitch)
            && self.zoom.is_finite()
            && (0.65..=2.0).contains(&self.zoom)
    }
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            self.yaw = (self.yaw + dx * 0.006).clamp(-1.0, 1.0);
            self.pitch = (self.pitch + dy * 0.006).clamp(0.25, 1.2);
        }
    }
    /// Orthographic camera: positive depth is older, positive level is upwards.
    pub fn project(self, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let depth = -sy * x + cy * z;
        [cy * x + sy * z, cp * y + sp * depth, cp * depth - sp * y]
    }
}

pub fn band_hz(band: usize) -> f64 {
    20.0 * 1000.0_f64
        .powf(band.min(HISTORY_SPECTRUM_BANDS - 1) as f64 / (HISTORY_SPECTRUM_BANDS - 1) as f64)
}
/// A log band with no FFT bin is unavailable, rather than a measured silence.
pub fn band_has_bins(band: usize, config: AnalysisConfig, sample_rate: u32) -> bool {
    if sample_rate == 0 || config.fft_size == 0 || band >= HISTORY_SPECTRUM_BANDS {
        return false;
    }
    let resolution = sample_rate as f64 / config.fft_size as f64;
    let lower = (20.0 * 1000.0_f64.powf((band as f64 - 0.5) / 95.0)).max(20.0);
    let bin = (lower / resolution).ceil().max(1.0) as usize;
    let hz = bin as f64 * resolution;
    bin <= config.fft_size / 2
        && hz <= 20000.0
        && ((hz / 20.0).ln() / 1000.0_f64.ln() * 95.0).round() as usize == band
}
pub fn level_height(db: f32, minimum: f32) -> f32 {
    if !db.is_finite() || !minimum.is_finite() || minimum >= 0.0 {
        return 0.0;
    }
    ((db - minimum) / -minimum).clamp(0.0, 1.0)
}
pub fn depth(seconds: f64, start: f64, end: f64) -> f32 {
    ((end - seconds) / (end - start).max(0.001)).clamp(0.0, 1.0) as f32 * 1.25
}

/// At most one actual record per time bucket, choosing the strongest transient.
/// No averaging across times/channels/conditions; full 10 Hz history is retained.
pub fn slice_indices(points: &VecDeque<HistoryPoint>, start: f64, end: f64) -> Vec<usize> {
    let mut selected: Vec<_> = sample_indices(points, start, end, MAX_SLICES)
        .into_iter()
        .flatten()
        .collect();
    selected.dedup();
    selected
}
pub fn sample_indices(
    points: &VecDeque<HistoryPoint>,
    start: f64,
    end: f64,
    count: usize,
) -> Vec<Option<usize>> {
    let count = count.clamp(1, 768);
    let mut buckets = vec![None; count];
    let score = |p: &HistoryPoint| {
        p.spectrum_dbfs
            .iter()
            .chain(p.reference_spectrum_dbfs.iter().flatten())
            .copied()
            .fold(-160.0_f32, f32::max)
    };
    for (index, p) in points.iter().enumerate() {
        let valid_end = crate::history::history_interval_end(p, points.get(index + 1));
        if p.gap || p.seconds > end || valid_end <= start {
            continue;
        }
        let first = (((p.seconds - start) / (end - start).max(0.001)) * count as f64)
            .floor()
            .clamp(0.0, (count - 1) as f64) as usize;
        let last = ((((valid_end - start) / (end - start).max(0.001)) * count as f64).ceil() - 1.0)
            .clamp(0.0, (count - 1) as f64) as usize;
        for bucket in buckets.iter_mut().take(last + 1).skip(first) {
            if bucket.is_none_or(|old| score(p) >= score(&points[old])) {
                *bucket = Some(index);
            }
        }
    }
    buckets
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpectrumReference {
    pub version: u32,
    pub seconds: f64,
    pub sample_rate: u32,
    pub config: AnalysisConfig,
    pub description: String,
    pub measurement: Vec<f32>,
    pub reference: Option<Vec<f32>>,
}
impl SpectrumReference {
    pub fn validate(&self) -> Result<(), String> {
        self.config.validate()?;
        if self.version != 1
            || self.description.len() > 16384
            || !self.seconds.is_finite()
            || self.sample_rate == 0
            || self.measurement.len() != HISTORY_SPECTRUM_BANDS
            || self
                .reference
                .as_ref()
                .is_some_and(|v| v.len() != HISTORY_SPECTRUM_BANDS)
            || self
                .measurement
                .iter()
                .chain(self.reference.iter().flatten())
                .any(|v| !v.is_finite() || !(-200.0..=60.0).contains(v))
        {
            return Err("invalid spectrum reference".into());
        }
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(
            &temp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temp, path).map_err(|e| e.to_string())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 65536 {
            return Err("reference file too large".into());
        }
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let value: Self = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heatmap_rows_cover_valid_record_intervals_but_leave_real_missing_time_blank() {
        let p = HistoryPoint {
            seconds: 0.0,
            measurement_rms: -20.0,
            reference_rms: None,
            spectrum_dbfs: [-40.0; 96],
            reference_spectrum_dbfs: None,
            gap: false,
            segment: 0,
            config: AnalysisConfig::FAST,
            sample_rate: 48000,
            measurement_clipped: false,
            reference_clipped: false,
        };
        let points = VecDeque::from([
            p,
            HistoryPoint { seconds: 0.1, ..p },
            HistoryPoint {
                seconds: 0.2,
                gap: true,
                ..p
            },
            HistoryPoint { seconds: 0.4, ..p },
        ]);
        let rows = sample_indices(&points, 0.0, 0.5, 100);
        assert!(rows[..40].iter().all(Option::is_some));
        assert!(rows[40..80].iter().all(Option::is_none));
        assert!(rows[80..].iter().all(|p| *p == Some(3)));
        assert_eq!(slice_indices(&points, 0.0, 0.5), vec![0, 1, 3]);
    }
    #[test]
    fn unavailable_fft_bands_are_not_reported_as_silence() {
        assert!(!band_has_bins(0, AnalysisConfig::FAST, 48000));
        let band = ((1500.0_f64 / 20.0).ln() / 1000.0_f64.ln() * 95.0).round() as usize;
        assert!(band_has_bins(band, AnalysisConfig::FAST, 48000));
        assert!(!band_has_bins(95, AnalysisConfig::FAST, 16000));
    }
    #[test]
    fn reference_file_replace_preserves_values_and_rejects_oversized_input() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-data")
            .join(format!("visualizer-reference-{}.json", std::process::id()));
        let mut r = SpectrumReference {
            version: 1,
            seconds: 2.0,
            sample_rate: 48000,
            config: AnalysisConfig::FAST,
            description: "青ch2 緑ch1 AGC不明".into(),
            measurement: vec![-12.0; 96],
            reference: None,
        };
        r.save(&path).unwrap();
        r.measurement[60] = -24.0;
        r.save(&path).unwrap();
        let loaded = SpectrumReference::load(&path).unwrap();
        assert_eq!(loaded.measurement[60], -24.0);
        assert!(loaded.reference.is_none());
        assert_eq!(loaded.description, r.description);
        std::fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(
            SpectrumReference::load(&path)
                .unwrap_err()
                .contains("too large")
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn axes_and_camera_keep_frequency_level_time_independent() {
        assert_eq!(band_hz(0), 20.0);
        assert!((band_hz(95) - 20000.0).abs() < 1e-8);
        assert_eq!(level_height(-60.0, -120.0), 0.5);
        assert_eq!(level_height(-60.0, -80.0), 0.25);
        assert_eq!(depth(10.0, 0.0, 10.0), 0.0);
        assert_eq!(depth(0.0, 0.0, 10.0), 1.25);
        let c = Camera::default();
        assert!(c.valid());
        let a = c.project([0.0, 0.0, 0.0]);
        let b = c.project([0.0, 1.0, 0.0]);
        assert!(b[1] > a[1]);
        let mut c = c;
        c.orbit(10000.0, -10000.0);
        assert!(c.valid());
        c.orbit(f32::NAN, 2.0);
        assert!(c.valid());
    }
    #[test]
    fn bounded_slices_preserve_transients_and_do_not_invent_gap_values() {
        let p = HistoryPoint {
            seconds: 0.0,
            measurement_rms: -60.0,
            reference_rms: None,
            spectrum_dbfs: [-90.0; 96],
            reference_spectrum_dbfs: None,
            gap: false,
            segment: 0,
            config: AnalysisConfig::FAST,
            sample_rate: 48000,
            measurement_clipped: false,
            reference_clipped: false,
        };
        let mut points: VecDeque<_> = (0..6000)
            .map(|i| HistoryPoint {
                seconds: i as f64 * 0.1,
                ..p
            })
            .collect();
        points[100].spectrum_dbfs[20] = -6.0;
        points[200].gap = true;
        let out = slice_indices(&points, 0.0, 600.0);
        assert!(out.len() <= MAX_SLICES);
        assert!(out.contains(&100));
        assert!(!out.contains(&200));
        assert!(out.windows(2).all(|p| p[0] < p[1]));
    }
    #[test]
    fn reference_roundtrip_rejects_wrong_bands_and_invalid_numbers() {
        let r = SpectrumReference {
            version: 1,
            seconds: 3.0,
            sample_rate: 48000,
            config: AnalysisConfig::FAST,
            description: "gain unknown".into(),
            measurement: vec![-12.0; 96],
            reference: Some(vec![-6.0; 96]),
        };
        let bytes = serde_json::to_vec(&r).unwrap();
        let mut decoded: SpectrumReference = serde_json::from_slice(&bytes).unwrap();
        decoded.validate().unwrap();
        decoded.measurement.pop();
        assert!(decoded.validate().is_err());
    }
}
