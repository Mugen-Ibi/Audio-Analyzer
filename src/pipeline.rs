use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Observer, Producer, Split},
};

use crate::{
    dsp::SpectrumAnalyzer,
    model::{AnalysisSnapshot, AudioBlock, ChannelRoute, RouteControl, RuntimeStats},
    source::{AudioInfo, InputSource, OpenedSource},
};

const RESULT_QUEUE_CAPACITY: usize = 3;

pub struct AnalyzerRuntime {
    audio: OpenedSource,
    dsp: DspWorker,
    reader: Option<AnalysisReader>,
    route_control: Arc<RouteControl>,
    stats: Arc<RuntimeStats>,
}

impl AnalyzerRuntime {
    #[cfg(feature = "audio-device")]
    pub fn start() -> Result<Self, String> {
        Self::start_with(&mut crate::audio::CpalSource, &AtomicBool::new(false))
    }

    pub fn start_with(
        source: &mut dyn InputSource,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("input start cancelled".to_owned());
        }
        let stats = Arc::new(RuntimeStats::default());
        let mut audio = source.open(Arc::clone(&stats), cancelled)?;
        audio.info().validate()?;
        if cancelled.load(Ordering::Acquire) {
            return Err("input start cancelled".to_owned());
        }
        let input = audio.input.take().expect("a new source owns its input");
        let route_control = input.route_control;
        let result_ring = HeapRb::<AnalysisSnapshot>::new(RESULT_QUEUE_CAPACITY);
        let (result_producer, result_consumer) = result_ring.split();
        let dsp = DspWorker::spawn(
            input.consumer,
            result_producer,
            audio.info().sample_rate,
            Arc::clone(&stats),
        )?;

        let channels = audio.info().channels;
        Ok(Self {
            audio,
            dsp,
            reader: Some(AnalysisReader {
                result_consumer,
                route_control: Arc::clone(&route_control),
                channels,
            }),
            route_control,
            stats,
        })
    }

    pub fn info(&self) -> &AudioInfo {
        self.audio.info()
    }

    pub fn stats(&self) -> &RuntimeStats {
        &self.stats
    }

    pub fn set_route(&self, route: ChannelRoute) -> Result<(), String> {
        self.route_control
            .store(route.validate(self.info().channels)?);
        Ok(())
    }

    pub fn take_latest(&mut self) -> Option<AnalysisSnapshot> {
        self.reader.as_mut().and_then(AnalysisReader::take_latest)
    }

    pub(crate) fn take_reader(&mut self) -> AnalysisReader {
        self.reader
            .take()
            .expect("result reader can only be transferred once")
    }

    pub(crate) fn shared_stats(&self) -> Arc<RuntimeStats> {
        Arc::clone(&self.stats)
    }
}

/// Single-owner result endpoint; independent of the thread-bound device stream.
pub struct AnalysisReader {
    result_consumer: HeapCons<AnalysisSnapshot>,
    route_control: Arc<RouteControl>,
    channels: usize,
}

impl AnalysisReader {
    pub fn take_latest(&mut self) -> Option<AnalysisSnapshot> {
        take_latest_matching(&mut self.result_consumer, self.route_control.load().1)
    }

    pub fn set_route(&self, route: ChannelRoute) -> Result<(), String> {
        self.route_control.store(route.validate(self.channels)?);
        Ok(())
    }
}

fn take_latest_matching(
    consumer: &mut HeapCons<AnalysisSnapshot>,
    generation: u64,
) -> Option<AnalysisSnapshot> {
    let mut latest = None;
    // Bound work to what was available at entry even if DSP keeps publishing.
    for _ in 0..consumer.occupied_len() {
        let Some(snapshot) = consumer.try_pop() else {
            break;
        };
        if snapshot.route_generation == generation {
            latest = Some(snapshot);
        }
    }
    latest
}

impl Drop for AnalyzerRuntime {
    fn drop(&mut self) {
        self.audio.stop();
        self.dsp.stop();
    }
}

struct DspWorker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl DspWorker {
    fn spawn(
        mut audio_consumer: HeapCons<AudioBlock>,
        mut result_producer: HeapProd<AnalysisSnapshot>,
        sample_rate: u32,
        stats: Arc<RuntimeStats>,
    ) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("audio-analyzer-dsp".to_owned())
            .spawn(move || {
                let mut analyzer = SpectrumAnalyzer::new(sample_rate);
                let mut expected_frame = None;
                let mut route_generation = None;
                let mut pending = None;

                while !worker_stop.load(Ordering::Acquire) {
                    let mut did_work = false;
                    while !worker_stop.load(Ordering::Acquire)
                        && let Some(block) = audio_consumer.try_pop()
                    {
                        did_work = true;
                        let frame_gap =
                            expected_frame.is_some_and(|expected| expected != block.start_frame);
                        let route_changed = route_generation
                            .is_some_and(|generation| generation != block.route_generation);
                        if frame_gap || route_changed {
                            stats.discontinuities.fetch_add(1, Ordering::Relaxed);
                        }
                        expected_frame = Some(block.start_frame + block.valid_frames as u64);
                        route_generation = Some(block.route_generation);

                        if let Some(snapshot) = analyzer.process_block(&block)
                            && pending.replace(snapshot).is_some()
                        {
                            stats.dropped_results.fetch_add(1, Ordering::Relaxed);
                        }
                        publish_pending(&mut result_producer, &mut pending);
                    }
                    publish_pending(&mut result_producer, &mut pending);
                    if !did_work {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            })
            .map_err(|error| format!("could not start the DSP worker: {error}"))?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn publish_pending(
    producer: &mut HeapProd<AnalysisSnapshot>,
    pending: &mut Option<AnalysisSnapshot>,
) {
    if let Some(snapshot) = pending.take() {
        *pending = producer.try_push(snapshot).err();
    }
}

impl Drop for DspWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_switch_filters_queued_old_results() {
        let (mut producer, mut consumer) = HeapRb::new(3).split();
        producer.try_push(AnalysisSnapshot::default()).unwrap();
        assert!(take_latest_matching(&mut consumer, 1).is_none());
        for sequence in 1..=3 {
            producer
                .try_push(AnalysisSnapshot {
                    sequence,
                    route_generation: 1,
                    ..AnalysisSnapshot::default()
                })
                .unwrap();
        }
        assert_eq!(take_latest_matching(&mut consumer, 1).unwrap().sequence, 3);
    }

    #[test]
    fn full_queue_retries_the_last_snapshot_without_new_audio() {
        let (mut producer, mut consumer) = HeapRb::new(1).split();
        producer.try_push(AnalysisSnapshot::default()).unwrap();
        let mut pending = Some(AnalysisSnapshot {
            sequence: 42,
            ..AnalysisSnapshot::default()
        });
        publish_pending(&mut producer, &mut pending);
        assert_eq!(pending.as_ref().unwrap().sequence, 42);
        consumer.try_pop().unwrap();
        publish_pending(&mut producer, &mut pending);
        assert!(pending.is_none());
        assert_eq!(consumer.try_pop().unwrap().sequence, 42);
    }

    #[test]
    fn worker_processes_audio_and_stops() {
        let (mut producer, consumer) = HeapRb::new(16).split();
        let (results, mut snapshots) = HeapRb::new(3).split();
        let mut worker =
            DspWorker::spawn(consumer, results, 48_000, Arc::new(RuntimeStats::default())).unwrap();
        for index in 0..8 {
            producer
                .try_push(AudioBlock {
                    start_frame: index * crate::model::AUDIO_BLOCK_FRAMES as u64,
                    valid_frames: crate::model::AUDIO_BLOCK_FRAMES,
                    ..AudioBlock::default()
                })
                .unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(snapshot) = snapshots.try_pop() {
                assert_eq!(snapshot.window_start_frame, 0);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker failed to produce a snapshot"
            );
            thread::sleep(Duration::from_millis(1));
        }
        worker.stop();
        assert!(worker.handle.is_none());
    }
}
