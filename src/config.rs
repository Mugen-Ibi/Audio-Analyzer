//! Reproducible local measurement conditions. No audio is persisted.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowFunction {
    #[default]
    Hann,
    Rectangular,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisConfig {
    pub fft_size: usize,
    pub window: WindowFunction,
    /// Exponential power/cross-power time constant. Zero means no averaging.
    pub averaging_ms: u32,
}
impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            window: WindowFunction::Hann,
            averaging_ms: 100,
        }
    }
}
impl AnalysisConfig {
    pub const FAST: Self = Self {
        fft_size: 1024,
        window: WindowFunction::Hann,
        averaging_ms: 0,
    };
    pub const PRECISE: Self = Self {
        fft_size: 8192,
        window: WindowFunction::Hann,
        averaging_ms: 500,
    };
    pub fn validate(self) -> Result<Self, String> {
        if ![1024, 2048, 8192].contains(&self.fft_size) || self.averaging_ms > 10_000 {
            return Err("FFT must be 1024/2048/8192 and averaging 0..=10000 ms".into());
        }
        Ok(self)
    }
    pub fn window_ms(self, rate: u32) -> f64 {
        self.fft_size as f64 * 1000.0 / rate.max(1) as f64
    }
    pub fn bin_hz(self, rate: u32) -> f64 {
        rate as f64 / self.fft_size as f64
    }
    pub fn hop(self) -> usize {
        self.fft_size / 2
    }
    fn pack(self, generation: u64) -> u64 {
        (generation << 32)
            | self.fft_size as u64
            | ((self.window == WindowFunction::Rectangular) as u64) << 14
            | (self.averaging_ms as u64) << 15
    }
}
pub struct AnalysisControl(AtomicU64);
impl AnalysisControl {
    pub fn new(config: AnalysisConfig) -> Self {
        Self(AtomicU64::new(config.pack(0)))
    }
    pub fn load(&self) -> (AnalysisConfig, u64) {
        let p = self.0.load(Ordering::Acquire);
        (
            AnalysisConfig {
                fft_size: (p & 0x3fff) as usize,
                window: if p & (1 << 14) == 0 {
                    WindowFunction::Hann
                } else {
                    WindowFunction::Rectangular
                },
                averaging_ms: ((p >> 15) & 0x1ffff) as u32,
            },
            p >> 32,
        )
    }
    pub fn store(&self, config: AnalysisConfig) -> Result<(), String> {
        config.validate()?;
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |p| {
                Some(config.pack(((p >> 32) as u32).wrapping_add(1) as u64))
            })
            .expect("always updates");
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputSettings {
    /// None means automatic backend selection, only at startup.
    pub host: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub route: Option<crate::model::ChannelRoute>,
}
impl InputSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.sample_rate == Some(0) || self.buffer_frames == Some(0) {
            return Err("rate and buffer must be nonzero or automatic".into());
        }
        if self.device_id.is_some() && self.host.is_none() {
            return Err("a saved device requires its backend".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub input: InputSettings,
    pub analysis: AnalysisConfig,
    pub history_seconds: u32,
    pub minimum_dbfs: f32,
    pub visualizer_mode: crate::visualizer::ViewMode,
    pub camera: crate::visualizer::Camera,
    /// User-entered setup record; analog gain cannot be read reliably through CPAL.
    pub condition_notes: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            input: InputSettings::default(),
            analysis: AnalysisConfig::FAST,
            history_seconds: 600,
            minimum_dbfs: -120.0,
            visualizer_mode: Default::default(),
            camera: Default::default(),
            condition_notes: String::new(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported settings version".into());
        }
        self.input.validate()?;
        self.analysis.validate()?;
        if !self.camera.valid()
            || !(60..=600).contains(&self.history_seconds)
            || !self.minimum_dbfs.is_finite()
            || !(-160.0..=-20.0).contains(&self.minimum_dbfs)
        {
            return Err("invalid history duration or spectrum range".into());
        }
        Ok(())
    }
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("settings read: {e}"))?;
        let settings: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("settings parse: {e}"))?;
        settings.validate()?;
        Ok(settings)
    }
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        // std::fs::rename replaces an existing file on Windows too.
        std::fs::rename(&temporary, path).map_err(|e| format!("settings replace: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previous_settings_gain_visualizer_defaults_and_camera_round_trips() {
        let old: Settings = serde_json::from_str(
            r#"{"version":1,"minimum_dbfs":-80.0,"condition_notes":"preserve"}"#,
        )
        .unwrap();
        assert_eq!(old.camera, crate::visualizer::Camera::default());
        assert_eq!(old.visualizer_mode, crate::visualizer::ViewMode::Waterfall);
        assert_eq!(old.minimum_dbfs, -80.0);
        let mut next = old;
        next.visualizer_mode = crate::visualizer::ViewMode::Heatmap;
        next.camera.orbit(-60.0, 10.0);
        next.camera.zoom = 1.5;
        let decoded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&next).unwrap()).unwrap();
        assert_eq!(decoded.camera, next.camera);
        assert_eq!(decoded.visualizer_mode, next.visualizer_mode);
        assert_eq!(decoded.condition_notes, "preserve");
        next.camera.zoom = 0.0;
        assert!(next.validate().is_err());
    }
    #[test]
    fn settings_round_trip_and_reject_invalid_values() {
        let s = Settings {
            condition_notes: "M2 + AT4040 / gain位置を記録".into(),
            ..Settings::default()
        };
        let decoded: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(s.condition_notes, decoded.condition_notes);
        let mut bad = s;
        bad.analysis.fft_size = 0;
        assert!(bad.validate().is_err());
        assert!(serde_json::from_str::<Settings>("broken").is_err());
    }
    #[test]
    fn control_keeps_conditions_and_generation_together() {
        let c = AnalysisControl::new(AnalysisConfig::FAST);
        c.store(AnalysisConfig::PRECISE).unwrap();
        assert_eq!(c.load(), (AnalysisConfig::PRECISE, 1));
        assert!(
            c.store(AnalysisConfig {
                fft_size: 999,
                ..AnalysisConfig::FAST
            })
            .is_err()
        );
        assert_eq!(c.load().1, 1);
    }
}
