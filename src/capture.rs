//! Device-independent, allocation-free callback ingress.
use crate::model::{
    AUDIO_BLOCK_FRAMES, AudioBlock, ChannelRoute, RouteControl, RuntimeStats, StereoFrame,
};
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Producer, Split},
};
use std::{
    mem,
    sync::{Arc, atomic::Ordering},
};

pub const AUDIO_QUEUE_BLOCKS: usize = 64;

/// Owned by exactly one input callback. Construct before starting the stream.
pub struct CaptureWriter {
    channels: usize,
    producer: HeapProd<AudioBlock>,
    route_control: Arc<RouteControl>,
    stats: Arc<RuntimeStats>,
    assembler: BlockAssembler,
}

pub struct CapturedInput {
    pub(crate) channels: usize,
    pub(crate) consumer: HeapCons<AudioBlock>,
    pub(crate) route_control: Arc<RouteControl>,
}

pub fn channel(
    channels: usize,
    route: ChannelRoute,
    stats: Arc<RuntimeStats>,
) -> Result<(CaptureWriter, CapturedInput), String> {
    if channels == 0 || channels > u16::MAX as usize {
        return Err("input must have 1..=65535 channels".to_owned());
    }
    let route_control = Arc::new(RouteControl::new(route.validate(channels)?));
    let (producer, consumer) = HeapRb::new(AUDIO_QUEUE_BLOCKS).split();
    Ok((
        CaptureWriter {
            channels,
            producer,
            route_control: Arc::clone(&route_control),
            stats,
            assembler: BlockAssembler::default(),
        },
        CapturedInput {
            channels,
            consumer,
            route_control,
        },
    ))
}

impl CaptureWriter {
    /// Data must contain complete interleaved frames. A malformed callback is
    /// rejected as a whole, advancing the timeline so DSP resets its overlap.
    /// Conversion must not allocate, lock, or wait.
    pub fn write_interleaved<T: Copy>(&mut self, data: &[T], convert: impl Fn(T) -> f32) {
        let frames = data.len() / self.channels;
        if !data.len().is_multiple_of(self.channels) {
            let discarded = self.assembler.block.valid_frames;
            self.assembler.block.clear();
            self.assembler.next_frame = self.assembler.next_frame.wrapping_add(frames as u64 + 1);
            self.stats
                .dropped_audio_frames
                .fetch_add((discarded + frames + 1) as u64, Ordering::Relaxed);
            self.stats.stream_errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.stats
            .received_audio_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        let (route, generation) = self.route_control.load();
        let discarded = self.assembler.begin_callback(route, generation);
        self.stats
            .dropped_audio_frames
            .fetch_add(discarded as u64, Ordering::Relaxed);
        for frame in data.chunks_exact(self.channels) {
            let pair = StereoFrame {
                measurement: convert(frame[route.measurement]),
                reference: route
                    .reference
                    .map_or(0.0, |channel| convert(frame[channel])),
            };
            if let Some(block) = self.assembler.push(pair)
                && let Err(mut rejected) = self.producer.try_push(block)
            {
                self.stats
                    .dropped_audio_frames
                    .fetch_add(rejected.valid_frames as u64, Ordering::Relaxed);
                rejected.clear();
                self.assembler.reuse(rejected);
            }
        }
    }
}
#[derive(Default)]
struct BlockAssembler {
    block: AudioBlock,
    next_frame: u64,
    route_generation: Option<u64>,
}

impl BlockAssembler {
    fn begin_callback(&mut self, route: ChannelRoute, generation: u64) -> usize {
        let mut discarded = 0;
        if self
            .route_generation
            .is_some_and(|previous| previous != generation)
        {
            discarded = self.block.valid_frames;
            self.block.clear();
        }
        self.route_generation = Some(generation);
        self.block.route_generation = generation;
        self.block.has_reference = route.reference.is_some();
        discarded
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
    use ringbuf::traits::Consumer;

    #[test]
    fn overload_drops_whole_pairs_and_preserves_the_next_timestamp() {
        let stats = Arc::new(RuntimeStats::default());
        let (mut writer, mut input) =
            channel(2, ChannelRoute::default_for_channels(2), Arc::clone(&stats)).unwrap();
        let data = [0.25; AUDIO_BLOCK_FRAMES * 2];
        for _ in 0..AUDIO_QUEUE_BLOCKS + 2 {
            writer.write_interleaved(&data, |value| value);
        }
        assert_eq!(
            stats.dropped_audio_frames.load(Ordering::Relaxed),
            (2 * AUDIO_BLOCK_FRAMES) as u64
        );
        for index in 0..AUDIO_QUEUE_BLOCKS {
            let block = input.consumer.try_pop().unwrap();
            assert_eq!(block.start_frame, (index * AUDIO_BLOCK_FRAMES) as u64);
            assert_eq!(block.valid_frames, AUDIO_BLOCK_FRAMES);
            assert!(block.has_reference);
        }
        writer.write_interleaved(&data, |value| value);
        assert_eq!(
            input.consumer.try_pop().unwrap().start_frame,
            ((AUDIO_QUEUE_BLOCKS + 2) * AUDIO_BLOCK_FRAMES) as u64
        );
    }

    #[test]
    fn route_change_discards_partial_input_and_uses_one_complete_new_route() {
        let stats = Arc::new(RuntimeStats::default());
        let (mut writer, mut input) =
            channel(2, ChannelRoute::default_for_channels(2), Arc::clone(&stats)).unwrap();
        writer.write_interleaved(&[0.5, 0.25], |value| value);
        input.route_control.store(ChannelRoute {
            reference: None,
            measurement: 0,
        });
        writer.write_interleaved(&[0.5; AUDIO_BLOCK_FRAMES * 2], |value| value);
        let block = input.consumer.try_pop().unwrap();
        assert_eq!(block.start_frame, 1);
        assert_eq!(block.route_generation, 1);
        assert!(!block.has_reference);
        assert!(
            block
                .frames
                .iter()
                .all(|frame| frame.measurement == 0.5 && frame.reference == 0.0)
        );
        assert_eq!(stats.dropped_audio_frames.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn malformed_interleaved_input_creates_a_gap_instead_of_shifted_pairs() {
        let stats = Arc::new(RuntimeStats::default());
        let (mut writer, mut input) =
            channel(2, ChannelRoute::default_for_channels(2), Arc::clone(&stats)).unwrap();
        writer.write_interleaved(&[0.5, 0.25], |value| value);
        writer.write_interleaved(&[0.5, 0.25, 0.5], |value| value);
        writer.write_interleaved(&[0.25; AUDIO_BLOCK_FRAMES * 2], |value| value);
        assert_eq!(input.consumer.try_pop().unwrap().start_frame, 3);
        assert_eq!(stats.stream_errors.load(Ordering::Relaxed), 1);
        assert_eq!(stats.dropped_audio_frames.load(Ordering::Relaxed), 3);
    }

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
        assert_eq!(
            assembler.begin_callback(ChannelRoute::default_for_channels(2), 2),
            1
        );
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
