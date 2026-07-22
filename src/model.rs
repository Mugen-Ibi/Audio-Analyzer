use std::sync::atomic::{AtomicU64, Ordering};

pub const AUDIO_BLOCK_FRAMES: usize = 256;
pub const FFT_SIZE: usize = 2048;
pub const HOP_SIZE: usize = FFT_SIZE / 2;
pub const SPECTRUM_BINS: usize = FFT_SIZE / 2 + 1;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |packed| {
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
            reference: (encoded_reference != 0).then_some(encoded_reference - 1),
            measurement,
        },
        generation,
    )
}

#[derive(Default)]
pub struct RuntimeStats {
    pub dropped_audio_frames: AtomicU64,
    pub stream_errors: AtomicU64,
    pub discontinuities: AtomicU64,
    pub dropped_results: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BinMetrics {
    pub measurement_dbfs: f32,
    pub transfer_db: f32,
    pub phase_degrees: f32,
    pub coherence: f32,
}

#[derive(Debug)]
pub struct AnalysisSnapshot {
    pub sequence: u64,
    pub window_start_frame: u64,
    pub sample_rate: u32,
    pub has_reference: bool,
    pub bins: [BinMetrics; SPECTRUM_BINS],
}

impl Default for AnalysisSnapshot {
    fn default() -> Self {
        Self {
            sequence: 0,
            window_start_frame: 0,
            sample_rate: 0,
            has_reference: false,
            bins: [BinMetrics::default(); SPECTRUM_BINS],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
