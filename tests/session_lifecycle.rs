//! Production capture, DSP and controller exercised without hardware.
use audio_analyzer::{
    capture,
    controller::{AnalyzerController, ConnectionState},
    model::{AUDIO_BLOCK_FRAMES, AnalysisSnapshot, ChannelRoute, FFT_SIZE, RuntimeStats},
    source::{AudioInfo, InputSource, InputStream, OpenedSource},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct ToneSource {
    sample_rate: u32,
    fail_next: bool,
    opens: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

struct ToneStream {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    stops: Arc<AtomicUsize>,
}

impl InputStream for ToneStream {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
            self.stops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl InputSource for ToneSource {
    fn open(
        &mut self,
        stats: Arc<RuntimeStats>,
        _cancelled: &AtomicBool,
    ) -> Result<OpenedSource, String> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        if std::mem::take(&mut self.fail_next) {
            return Err("test device unavailable".to_owned());
        }
        let route = ChannelRoute::default_for_channels(2);
        let (mut writer, input) = capture::channel(2, route, stats)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut position = 0;
            let mut data = [0.0; AUDIO_BLOCK_FRAMES * 2];
            while !thread_stop.load(Ordering::Acquire) {
                for pair in data.chunks_exact_mut(2) {
                    let phase = 2.0 * std::f32::consts::PI * 32.0 * (position % FFT_SIZE) as f32
                        / FFT_SIZE as f32;
                    pair[0] = 0.5 * phase.sin();
                    pair[1] = 0.25 * phase.sin();
                    position += 1;
                }
                writer.write_interleaved(&data, |value| value);
                thread::sleep(Duration::from_millis(1));
            }
        });
        OpenedSource::new(
            AudioInfo {
                host_name: "synthetic".to_owned(),
                device_name: "paired tone".to_owned(),
                sample_rate: self.sample_rate,
                channels: 2,
                initial_route: route,
            },
            input,
            Box::new(ToneStream {
                stop,
                handle: Some(handle),
                stops: Arc::clone(&self.stops),
            }),
        )
    }
}

fn wait_until(controller: &mut AnalyzerController, ready: impl Fn(&ConnectionState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        controller.poll();
        if ready(controller.state()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "unexpected state: {:?}",
            controller.state()
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn snapshot(controller: &mut AnalyzerController) -> AnalysisSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        controller.poll();
        if let Some(snapshot) = controller.take_latest() {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "missing snapshot: {:?}",
            controller.state()
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn failed_start_retries_routes_stop_and_reconnect_use_fresh_sessions() {
    let opens = Arc::new(AtomicUsize::new(0));
    let stops = Arc::new(AtomicUsize::new(0));
    let mut controller = AnalyzerController::with_source(ToneSource {
        sample_rate: 48_000,
        fail_next: true,
        opens: Arc::clone(&opens),
        stops: Arc::clone(&stops),
    })
    .unwrap();
    assert!(controller.info().is_none());
    controller.start().unwrap();
    assert!(controller.start().is_err());
    wait_until(
        &mut controller,
        |state| matches!(state, ConnectionState::Failed(error) if error == "test device unavailable"),
    );
    controller.start().unwrap();
    wait_until(&mut controller, |state| *state == ConnectionState::Running);
    let initial = snapshot(&mut controller);
    assert!(initial.has_reference);
    assert!((initial.bins[32].transfer_db + 6.0206).abs() < 0.03);
    assert!((initial.bins[32].measurement_dbfs + 12.0412).abs() < 0.03);
    assert!(initial.bins[32].coherence > 0.999);
    assert!(
        controller
            .set_route(ChannelRoute {
                reference: None,
                measurement: 2
            })
            .is_err()
    );
    controller
        .set_route(ChannelRoute {
            reference: None,
            measurement: 0,
        })
        .unwrap();
    let mono = snapshot(&mut controller);
    assert!(!mono.has_reference);
    assert_ne!(mono.route_generation, initial.route_generation);
    assert!((mono.bins[32].measurement_dbfs + 6.0206).abs() < 0.03);
    controller.stop();
    assert!(controller.info().is_none());
    assert!(controller.take_latest().is_none());
    wait_until(&mut controller, |state| *state == ConnectionState::Stopped);
    assert_eq!(stops.load(Ordering::Relaxed), 1);
    controller.start().unwrap();
    wait_until(&mut controller, |state| *state == ConnectionState::Running);
    let reconnected = snapshot(&mut controller);
    assert!(reconnected.has_reference);
    assert_eq!(reconnected.route_generation, 0);
    drop(controller);
    assert_eq!(opens.load(Ordering::Relaxed), 3);
    assert_eq!(stops.load(Ordering::Relaxed), 2);
}

#[test]
fn production_pipeline_supports_high_sample_rates() {
    for sample_rate in [96_000, 192_000] {
        let mut controller = AnalyzerController::with_source(ToneSource {
            sample_rate,
            fail_next: false,
            opens: Arc::default(),
            stops: Arc::default(),
        })
        .unwrap();
        controller.start().unwrap();
        let result = snapshot(&mut controller);
        assert_eq!(result.sample_rate, sample_rate);
        assert!((result.bins[32].transfer_db + 6.0206).abs() < 0.03);
    }
}

struct WaitingSource {
    entered: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
}
impl InputSource for WaitingSource {
    fn open(
        &mut self,
        _stats: Arc<RuntimeStats>,
        cancelled: &AtomicBool,
    ) -> Result<OpenedSource, String> {
        self.entered.store(true, Ordering::Release);
        while !cancelled.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
        self.exited.store(true, Ordering::Release);
        Err("cancelled".to_owned())
    }
}

#[test]
fn pending_driver_start_can_be_cancelled_without_blocking_the_caller() {
    let entered = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let mut controller = AnalyzerController::with_source(WaitingSource {
        entered: Arc::clone(&entered),
        exited: Arc::clone(&exited),
    })
    .unwrap();
    controller.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !entered.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(controller.state(), &ConnectionState::Starting);
    controller.stop();
    wait_until(&mut controller, |state| *state == ConnectionState::Stopped);
    assert!(exited.load(Ordering::Acquire));
}

#[test]
fn dropping_during_start_cancels_and_joins_the_owner() {
    let entered = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let mut controller = AnalyzerController::with_source(WaitingSource {
        entered: Arc::clone(&entered),
        exited: Arc::clone(&exited),
    })
    .unwrap();
    controller.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !entered.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    drop(controller);
    assert!(exited.load(Ordering::Acquire));
}
