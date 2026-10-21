use std::sync::atomic::{AtomicU64, Ordering};

pub const AUDIO_BLOCK_FRAMES: usize = 256;
pub const FFT_SIZE: usize = 2048;
pub const HOP_SIZE: usize = FFT_SIZE / 2;
pub const SPECTRUM_BINS: usize = FFT_SIZE / 2 + 1;
pub const MAX_FFT_SIZE: usize = 8192;
pub const MAX_SPECTRUM_BINS: usize = MAX_FFT_SIZE / 2 + 1;

/// Small snapshot handle. The DSP fills only uniquely owned preallocated slots;
/// frozen/UI snapshots keep their slot immutable through Arc ownership.
#[derive(Clone, Debug)]
pub struct BinStorage(std::sync::Arc<[BinMetrics]>);
impl Default for BinStorage {
    fn default() -> Self {
        Self(std::sync::Arc::from(
            vec![BinMetrics::default(); MAX_SPECTRUM_BINS].into_boxed_slice(),
        ))
    }
}
impl BinStorage {
    pub(crate) fn available(&self) -> bool {
        std::sync::Arc::strong_count(&self.0) == 1
    }
}
impl std::ops::Deref for BinStorage {
    type Target = [BinMetrics];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for BinStorage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        std::sync::Arc::make_mut(&mut self.0)
    }
}
pub const VECTOR_SCOPE_POINTS: usize = 128;

#[derive(Clone, Copy, Debug, Default)]
pub struct StereoFrame {
    pub reference: f32,
    pub measurement: f32,
}

#[derive(Debug)]
pub struct AudioBlock {
    pub start_frame: u64,
    pub valid_frames: usize,
    pub route_generation: u64,
    pub has_reference: bool,
    pub frames: [StereoFrame; AUDIO_BLOCK_FRAMES],
}

impl Default for AudioBlock {
    fn default() -> Self {
        Self {
            start_frame: 0,
            valid_frames: 0,
            route_generation: 0,
            has_reference: false,
            frames: [StereoFrame::default(); AUDIO_BLOCK_FRAMES],
        }
    }
}

impl AudioBlock {
    pub fn clear(&mut self) {
        self.valid_frames = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChannelRoute {
    pub reference: Option<usize>,
    pub measurement: usize,
}

impl ChannelRoute {
    pub fn default_for_channels(channels: usize) -> Self {
        if channels >= 2 {
            Self {
                reference: Some(0),
                measurement: 1,
            }
        } else {
            Self {
                reference: None,
                measurement: 0,
            }
        }
    }

    pub fn validate(self, channels: usize) -> Result<Self, String> {
        if channels == 0 || channels > u16::MAX as usize {
            return Err("input must have 1..=65535 channels".to_owned());
        }
        if self.measurement >= channels {
            return Err(format!(
                "Measurement channel {} is outside the {} available channels",
                self.measurement + 1,
                channels
            ));
        }
        if self.reference.is_some_and(|channel| channel >= channels) {
            return Err(format!(
                "Reference channel is outside the {channels} available channels"
            ));
        }
        if self.reference == Some(self.measurement) {
            return Err(
                "comparison channels must be different; disable the green input for mono".into(),
            );
        }
        Ok(self)
    }
}

pub struct RouteControl {
    packed_route: AtomicU64,
}

impl RouteControl {
    pub fn new(route: ChannelRoute) -> Self {
        Self {
            packed_route: AtomicU64::new(pack_route(route, 0)),
        }
    }

    pub fn load(&self) -> (ChannelRoute, u64) {
        unpack_route(self.packed_route.load(Ordering::Acquire))
    }

    pub fn store(&self, route: ChannelRoute) {
        self.packed_route
            .try_update(Ordering::AcqRel, Ordering::Acquire, |packed| {
                let (_, generation) = unpack_route(packed);
                Some(pack_route(route, generation.wrapping_add(1)))
            })
            .expect("the route update closure always returns a value");
    }
}

fn pack_route(route: ChannelRoute, generation: u64) -> u64 {
    let measurement = route.measurement as u16 as u64;
    let reference = route
        .reference
        .map_or(0, |channel| (channel as u16).saturating_add(1)) as u64;
    ((generation as u32 as u64) << 32) | (reference << 16) | measurement
}

fn unpack_route(packed: u64) -> (ChannelRoute, u64) {
    let measurement = (packed & 0xffff) as usize;
    let encoded_reference = ((packed >> 16) & 0xffff) as usize;
    let generation = packed >> 32;
    (
        ChannelRoute {
            reference: encoded_reference.checked_sub(1),
            measurement,
        },
        generation,
    )
}

#[derive(Default)]
pub struct RuntimeStats {
    pub coalesced_results: AtomicU64,
    pub invalid_samples: AtomicU64,
    pub clipped_frames: AtomicU64,
    pub callback_frames: AtomicU64,
    pub callback_min_frames: AtomicU64,
    pub callback_max_frames: AtomicU64,
    pub latest_frame: AtomicU64,
    pub received_audio_frames: AtomicU64,
    pub dropped_audio_frames: AtomicU64,
    pub stream_errors: AtomicU64,
    pub discontinuities: AtomicU64,
    pub dropped_results: AtomicU64,
}

#[cfg(feature = "audio-device")]
impl RuntimeStats {
    /// Only between fully stopped startup attempts, before a new stream exists.
    pub(crate) fn reset_startup_attempt(&self) {
        for counter in [
            &self.coalesced_results,
            &self.invalid_samples,
            &self.clipped_frames,
            &self.callback_frames,
            &self.callback_min_frames,
            &self.callback_max_frames,
            &self.latest_frame,
            &self.received_audio_frames,
            &self.dropped_audio_frames,
            &self.stream_errors,
            &self.discontinuities,
            &self.dropped_results,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BinMetrics {
    pub reference_dbfs: f32,
    pub measurement_dbfs: f32,
    pub transfer_db: f32,
    pub phase_degrees: f32,
    pub coherence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct SignalLevel {
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

impl Default for SignalLevel {
    fn default() -> Self {
        Self {
            peak_dbfs: -120.0,
            rms_dbfs: -120.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AnalysisSnapshot {
    /// Input continuity counter captured with this result, not sampled later by UI.
    pub discontinuity_id: u64,
    /// Capture progress at processing completion, relative to this window end.
    pub queue_lag_frames: u64,
    pub analysis_generation: u64,
    pub config: crate::config::AnalysisConfig,
    pub bin_count: usize,
    pub processed_at: std::time::Instant,
    pub invalid_samples: u32,
    pub measurement_clipped: bool,
    pub reference_clipped: bool,
    pub averaged_windows: u32,
    pub comparison_ready: bool,
    pub route_generation: u64,
    pub sequence: u64,
    pub window_start_frame: u64,
    pub sample_rate: u32,
    pub has_reference: bool,
    pub measurement_level: SignalLevel,
    pub reference_level: Option<SignalLevel>,
    pub phase_correlation: Option<f32>,
    pub scope_points: [StereoFrame; VECTOR_SCOPE_POINTS],
    pub scope_len: usize,
    pub bins: BinStorage,
}

impl Default for AnalysisSnapshot {
    fn default() -> Self {
        Self::with_bins(BinStorage::default())
    }
}
impl AnalysisSnapshot {
    pub(crate) fn with_bins(bins: BinStorage) -> Self {
        Self {
            discontinuity_id: 0,
            queue_lag_frames: 0,
            analysis_generation: 0,
            config: crate::config::AnalysisConfig::default(),
            bin_count: SPECTRUM_BINS,
            processed_at: std::time::Instant::now(),
            invalid_samples: 0,
            measurement_clipped: false,
            reference_clipped: false,
            averaged_windows: 0,
            comparison_ready: false,
            route_generation: 0,
            sequence: 0,
            window_start_frame: 0,
            sample_rate: 0,
            has_reference: false,
            measurement_level: SignalLevel::default(),
            reference_level: None,
            phase_correlation: None,
            scope_points: [StereoFrame::default(); VECTOR_SCOPE_POINTS],
            scope_len: 0,
            bins,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_reference_round_trips() {
        let route = ChannelRoute::default_for_channels(1);
        let control = RouteControl::new(route);
        assert_eq!(control.load(), (route, 0));
        control.store(route);
        assert_eq!(control.load(), (route, 1));
    }

    #[test]
    fn route_control_publishes_a_complete_route() {
        let control = RouteControl::new(ChannelRoute::default_for_channels(2));
        let route = ChannelRoute {
            reference: Some(3),
            measurement: 2,
        };
        control.store(route);
        let (loaded, generation) = control.load();
        assert_eq!(loaded, route);
        assert_eq!(generation, 1);
    }

    #[test]
    fn route_validation_rejects_out_of_range_channels() {
        assert!(
            ChannelRoute {
                reference: Some(0),
                measurement: 2,
            }
            .validate(2)
            .is_err()
        );
    }
}
