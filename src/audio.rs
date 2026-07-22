use std::{mem, sync::Arc};

use cpal::{
    BufferSize, FromSample, Sample, SampleFormat, SizedSample, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Producer, Split},
};

use crate::model::{
    AUDIO_BLOCK_FRAMES, AudioBlock, ChannelRoute, RouteControl, RuntimeStats, StereoFrame,
};

const AUDIO_QUEUE_BLOCKS: usize = 64;

#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub host_name: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub initial_route: ChannelRoute,
}

pub struct AudioEngine {
    stream: Option<cpal::Stream>,
    route_control: Arc<RouteControl>,
    info: AudioInfo,
}

impl AudioEngine {
    pub fn start(stats: Arc<RuntimeStats>) -> Result<(Self, HeapCons<AudioBlock>), String> {
        let mut failures = Vec::new();
        for host in candidate_hosts() {
            let host_name = format!("{:?}", host.id());
            match start_on_host(host, Arc::clone(&stats)) {
                Ok((stream, consumer, route_control, info)) => {
                    return Ok((
                        Self {
                            stream: Some(stream),
                            route_control,
                            info,
                        },
                        consumer,
                    ));
                }
                Err(error) => failures.push(format!("{host_name}: {error}")),
            }
        }

        Err(format!(
            "No usable input stream was found. {}",
            failures.join(" | ")
        ))
    }

    pub fn info(&self) -> &AudioInfo {
        &self.info
    }

    pub fn set_route(&self, route: ChannelRoute) -> Result<(), String> {
        let route = route.validate(self.info.channels)?;
        self.route_control.store(route);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream.take();
    }
}

fn candidate_hosts() -> Vec<cpal::Host> {
    let mut hosts = Vec::new();

    #[cfg(target_os = "windows")]
    for id in [cpal::HostId::Asio, cpal::HostId::Wasapi] {
        if let Ok(host) = cpal::host_from_id(id) {
            hosts.push(host);
        }
    }

    let default = cpal::default_host();
    if !hosts.iter().any(|host| host.id() == default.id()) {
        hosts.push(default);
    }
    hosts
}

fn start_on_host(
    host: cpal::Host,
    stats: Arc<RuntimeStats>,
) -> Result<
    (
        cpal::Stream,
        HeapCons<AudioBlock>,
        Arc<RouteControl>,
        AudioInfo,
    ),
    String,
> {
    let device = host
        .default_input_device()
        .ok_or_else(|| "no default input device".to_owned())?;
    let device_name = device.to_string();
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("could not read the default input config: {error}"))?;
    let sample_format = supported_config.sample_format();
    let mut stream_config: StreamConfig = supported_config.into();

    #[cfg(target_os = "windows")]
    if host.id() == cpal::HostId::Wasapi
        && let cpal::SupportedBufferSize::Range { min, max } = supported_config.buffer_size()
    {
        stream_config.buffer_size = BufferSize::Fixed((*min).max(256).min(*max));
    }

    let channels = stream_config.channels as usize;
    if channels == 0 {
        return Err("the input configuration reports zero channels".to_owned());
    }

    let initial_route = ChannelRoute::default_for_channels(channels);
    let route_control = Arc::new(RouteControl::new(initial_route));
    let ring = HeapRb::<AudioBlock>::new(AUDIO_QUEUE_BLOCKS);
    let (producer, consumer) = ring.split();
    let stream = build_stream_for_format(
        sample_format,
        &device,
        stream_config,
        channels,
        producer,
        Arc::clone(&route_control),
        stats,
    )?;
    stream
        .play()
        .map_err(|error| format!("could not start the input stream: {error}"))?;

    let info = AudioInfo {
        host_name: format!("{:?}", host.id()),
        device_name,
        sample_rate: stream_config.sample_rate,
        channels,
        initial_route,
    };
    Ok((stream, consumer, route_control, info))
}

#[allow(clippy::too_many_arguments)]
fn build_stream_for_format(
    sample_format: SampleFormat,
    device: &cpal::Device,
    config: StreamConfig,
    channels: usize,
    producer: HeapProd<AudioBlock>,
    route_control: Arc<RouteControl>,
    stats: Arc<RuntimeStats>,
) -> Result<cpal::Stream, String> {
    macro_rules! build {
        ($sample:ty) => {
            build_input_stream::<$sample>(device, config, channels, producer, route_control, stats)
        };
    }

    match sample_format {
        SampleFormat::I8 => build!(i8),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I24 => build!(cpal::I24),
        SampleFormat::I32 => build!(i32),
        SampleFormat::I64 => build!(i64),
        SampleFormat::U8 => build!(u8),
        SampleFormat::U16 => build!(u16),
        SampleFormat::U24 => build!(cpal::U24),
        SampleFormat::U32 => build!(u32),
        SampleFormat::U64 => build!(u64),
        SampleFormat::F32 => build!(f32),
        SampleFormat::F64 => build!(f64),
        unsupported => Err(format!("unsupported sample format: {unsupported}")),
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    channels: usize,
    mut producer: HeapProd<AudioBlock>,
    route_control: Arc<RouteControl>,
    stats: Arc<RuntimeStats>,
) -> Result<cpal::Stream, String>
where
    T: Sample + SizedSample + 'static,
    f32: FromSample<T>,
{
    let error_stats = Arc::clone(&stats);
    let mut assembler = BlockAssembler::default();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let (route, generation) = route_control.load();
                assembler.begin_callback(route, generation);
                for frame in data.chunks_exact(channels) {
                    let measurement = frame[route.measurement].to_sample();
                    let reference = route
                        .reference
                        .map_or(0.0, |channel| frame[channel].to_sample());
                    if let Some(full_block) = assembler.push(StereoFrame {
                        reference,
                        measurement,
                    }) && let Err(mut rejected) = producer.try_push(full_block)
                    {
                        error_stats.dropped_audio_frames.fetch_add(
                            rejected.valid_frames as u64,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        rejected.clear();
                        assembler.reuse(rejected);
                    }
                }
            },
            move |error| {
                stats
                    .stream_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                eprintln!("Audio stream error: {error}");
            },
            None,
        )
        .map_err(|error| format!("could not build the input stream: {error}"))
}

#[derive(Default)]
struct BlockAssembler {
    block: AudioBlock,
    next_frame: u64,
    route_generation: Option<u64>,
}

impl BlockAssembler {
    fn begin_callback(&mut self, route: ChannelRoute, generation: u64) {
        if self
            .route_generation
            .is_some_and(|previous| previous != generation)
        {
            self.block.clear();
        }
        self.route_generation = Some(generation);
        self.block.route_generation = generation;
        self.block.has_reference = route.reference.is_some();
    }

    fn push(&mut self, frame: StereoFrame) -> Option<AudioBlock> {
        if self.block.valid_frames == 0 {
            self.block.start_frame = self.next_frame;
        }
        self.block.frames[self.block.valid_frames] = frame;
        self.block.valid_frames += 1;
        self.next_frame = self.next_frame.wrapping_add(1);

        if self.block.valid_frames == AUDIO_BLOCK_FRAMES {
            let replacement = AudioBlock {
                route_generation: self.block.route_generation,
                has_reference: self.block.has_reference,
                ..AudioBlock::default()
            };
            Some(mem::replace(&mut self.block, replacement))
        } else {
            None
        }
    }

    fn reuse(&mut self, mut block: AudioBlock) {
        block.route_generation = self.route_generation.unwrap_or(0);
        self.block = block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembler_preserves_channel_pairs_and_frame_numbers() {
        let mut assembler = BlockAssembler::default();
        assembler.begin_callback(
            ChannelRoute {
                reference: Some(0),
                measurement: 1,
            },
            0,
        );
        let mut completed = None;
        for index in 0..AUDIO_BLOCK_FRAMES {
            completed = assembler.push(StereoFrame {
                reference: index as f32,
                measurement: -(index as f32),
            });
        }
        let block = completed.expect("a complete block should be emitted");
        assert_eq!(block.start_frame, 0);
        assert_eq!(block.valid_frames, AUDIO_BLOCK_FRAMES);
        assert_eq!(block.frames[42].reference, 42.0);
        assert_eq!(block.frames[42].measurement, -42.0);
    }

    #[test]
    fn route_change_discards_a_partial_block() {
        let mut assembler = BlockAssembler::default();
        assembler.begin_callback(ChannelRoute::default_for_channels(2), 0);
        assembler.push(StereoFrame::default());
        assembler.begin_callback(ChannelRoute::default_for_channels(2), 2);
        assert_eq!(assembler.block.valid_frames, 0);
    }

    #[test]
    fn reference_state_is_preserved_across_multiple_blocks_in_one_callback() {
        let mut assembler = BlockAssembler::default();
        assembler.begin_callback(
            ChannelRoute {
                reference: Some(0),
                measurement: 1,
            },
            4,
        );

        let mut completed = Vec::new();
        for _ in 0..(AUDIO_BLOCK_FRAMES * 2) {
            if let Some(block) = assembler.push(StereoFrame::default()) {
                completed.push(block);
            }
        }

        assert_eq!(completed.len(), 2);
        assert!(completed.iter().all(|block| block.has_reference));
        assert!(completed.iter().all(|block| block.route_generation == 4));
    }
}
