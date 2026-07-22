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
    traits::{Consumer, Producer, Split},
};

use crate::{
    audio::{AudioEngine, AudioInfo},
    dsp::SpectrumAnalyzer,
    model::{AnalysisSnapshot, AudioBlock, ChannelRoute, RuntimeStats},
};

const RESULT_QUEUE_CAPACITY: usize = 3;

pub struct AnalyzerRuntime {
    audio: AudioEngine,
    dsp: DspWorker,
    result_consumer: HeapCons<AnalysisSnapshot>,
    stats: Arc<RuntimeStats>,
}

impl AnalyzerRuntime {
    pub fn start() -> Result<Self, String> {
        let stats = Arc::new(RuntimeStats::default());
        let (audio, audio_consumer) = AudioEngine::start(Arc::clone(&stats))?;
        let result_ring = HeapRb::<AnalysisSnapshot>::new(RESULT_QUEUE_CAPACITY);
        let (result_producer, result_consumer) = result_ring.split();
        let dsp = DspWorker::spawn(
            audio_consumer,
            result_producer,
            audio.info().sample_rate,
            Arc::clone(&stats),
        )?;

        Ok(Self {
            audio,
            dsp,
            result_consumer,
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
        self.audio.set_route(route)
    }

    pub fn take_latest(&mut self) -> Option<AnalysisSnapshot> {
        let mut latest = None;
        while let Some(snapshot) = self.result_consumer.try_pop() {
            latest = Some(snapshot);
        }
        latest
    }
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

                while !worker_stop.load(Ordering::Acquire) {
                    let mut did_work = false;
                    while let Some(block) = audio_consumer.try_pop() {
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
                            && result_producer.try_push(snapshot).is_err()
                        {
                            stats.dropped_results.fetch_add(1, Ordering::Relaxed);
                        }
                    }
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

impl Drop for DspWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
