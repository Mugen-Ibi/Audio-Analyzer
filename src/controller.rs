//! Nonblocking UI control plane. Driver ownership stays on the supervisor.
use crate::{
    model::{AnalysisSnapshot, ChannelRoute, RuntimeStats},
    pipeline::{AnalysisReader, AnalyzerRuntime},
    source::{AudioInfo, InputSource},
};
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed(String),
}

struct SessionView {
    info: AudioInfo,
    reader: AnalysisReader,
    stats: Arc<RuntimeStats>,
}

enum Event {
    Started(SessionView),
    Stopped,
    Failed(String),
}

pub struct AnalyzerController {
    state: ConnectionState,
    session: Option<SessionView>,
    idle_stats: RuntimeStats,
    starts: HeapProd<()>,
    events: HeapCons<Event>,
    cancelled: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AnalyzerController {
    pub fn with_source(mut source: impl InputSource + Send + 'static) -> Result<Self, String> {
        let (starts, mut requests) = HeapRb::new(1).split();
        let (mut events_out, events) = HeapRb::new(1).split();
        let cancelled = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_shutdown = Arc::clone(&shutdown);
        let handle = thread::Builder::new()
            .name("audio-analyzer-control".to_owned())
            .spawn(move || {
                let mut runtime = None;
                let mut pending = None;
                while !worker_shutdown.load(Ordering::Acquire) {
                    if worker_cancelled.load(Ordering::Acquire) && runtime.is_some() {
                        drop(runtime.take());
                        pending = Some(Event::Stopped);
                    }
                    if requests.try_pop().is_some() {
                        let result = AnalyzerRuntime::start_with(&mut source, &worker_cancelled);
                        if worker_cancelled.load(Ordering::Acquire) {
                            drop(result);
                            pending = Some(Event::Stopped);
                        } else {
                            match result {
                                Ok(mut started) => {
                                    let view = SessionView {
                                        info: started.info().clone(),
                                        stats: started.shared_stats(),
                                        reader: started.take_reader(),
                                    };
                                    runtime = Some(started);
                                    pending = Some(Event::Started(view));
                                }
                                Err(error) => pending = Some(Event::Failed(error)),
                            }
                        }
                    }
                    if let Some(event) = pending.take() {
                        pending = events_out.try_push(event).err();
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                drop(runtime); // Input stop precedes DSP join via AnalyzerRuntime::drop.
            })
            .map_err(|error| format!("could not start the control worker: {error}"))?;
        Ok(Self {
            state: ConnectionState::Stopped,
            session: None,
            idle_stats: RuntimeStats::default(),
            starts,
            events,
            cancelled,
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn state(&self) -> &ConnectionState {
        &self.state
    }

    /// Returns immediately. Repeated starts while busy are rejected.
    pub fn start(&mut self) -> Result<(), String> {
        if !matches!(
            self.state,
            ConnectionState::Stopped | ConnectionState::Failed(_)
        ) {
            return Err("input is already connected or changing state".to_owned());
        }
        self.cancelled.store(false, Ordering::Release);
        self.starts
            .try_push(())
            .map_err(|_| "an input start request is already pending".to_owned())?;
        self.state = ConnectionState::Starting;
        Ok(())
    }

    /// Old results become inaccessible immediately, before the device stops.
    pub fn stop(&mut self) {
        if matches!(
            self.state,
            ConnectionState::Starting | ConnectionState::Running
        ) {
            self.cancelled.store(true, Ordering::Release);
            self.session = None;
            self.state = ConnectionState::Stopping;
        }
    }

    /// Call once per UI tick before reading info or results.
    pub fn poll(&mut self) {
        while let Some(event) = self.events.try_pop() {
            match event {
                Event::Started(session) if self.state != ConnectionState::Stopping => {
                    self.session = Some(session);
                    self.state = ConnectionState::Running;
                }
                Event::Started(_) => {}
                Event::Stopped => {
                    self.session = None;
                    self.state = ConnectionState::Stopped;
                }
                Event::Failed(error) => {
                    self.session = None;
                    self.state = if self.state == ConnectionState::Stopping {
                        ConnectionState::Stopped
                    } else {
                        ConnectionState::Failed(error)
                    };
                }
            }
        }
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            self.session = None;
            self.state = ConnectionState::Failed("control worker exited unexpectedly".to_owned());
        }
    }

    pub fn info(&self) -> Option<&AudioInfo> {
        self.session.as_ref().map(|session| &session.info)
    }

    pub fn stats(&self) -> &RuntimeStats {
        self.session
            .as_ref()
            .map_or(&self.idle_stats, |session| &session.stats)
    }

    pub fn set_route(&self, route: ChannelRoute) -> Result<(), String> {
        self.session
            .as_ref()
            .ok_or_else(|| "input is not connected".to_owned())?
            .reader
            .set_route(route)
    }

    pub fn take_latest(&mut self) -> Option<AnalysisSnapshot> {
        self.session
            .as_mut()
            .and_then(|session| session.reader.take_latest())
    }
}

impl Drop for AnalyzerController {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.shutdown.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capture::{self, CaptureWriter},
        source::{InputStream, OpenedSource},
    };
    use ringbuf::traits::Observer;
    use std::{sync::atomic::AtomicUsize, time::Instant};

    struct SilentSource(Arc<AtomicUsize>);
    struct SilentStream(Option<CaptureWriter>, Arc<AtomicUsize>);
    impl InputStream for SilentStream {
        fn stop(&mut self) {
            self.0.take();
            self.1.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl InputSource for SilentSource {
        fn open(
            &mut self,
            stats: Arc<RuntimeStats>,
            _: &AtomicBool,
        ) -> Result<OpenedSource, String> {
            let route = ChannelRoute::default_for_channels(1);
            let (writer, input) = capture::channel(1, route, stats)?;
            OpenedSource::new(
                AudioInfo {
                    host_name: "test".into(),
                    device_name: "silent".into(),
                    sample_rate: 48_000,
                    channels: 1,
                    initial_route: route,
                },
                input,
                Box::new(SilentStream(Some(writer), Arc::clone(&self.0))),
            )
        }
    }

    #[test]
    fn stop_with_unread_started_event_cannot_resurrect_the_old_session() {
        let stops = Arc::new(AtomicUsize::new(0));
        let mut controller =
            AnalyzerController::with_source(SilentSource(Arc::clone(&stops))).unwrap();
        controller.start().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while controller.events.is_empty() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        // The UI has not consumed Started, so the status queue is full.
        controller.stop();
        while stops.load(Ordering::Relaxed) == 0 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        loop {
            controller.poll();
            assert!(controller.info().is_none());
            if controller.state() == &ConnectionState::Stopped {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        controller.start().unwrap();
        loop {
            controller.poll();
            if controller.state() == &ConnectionState::Running {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        drop(controller);
        assert_eq!(stops.load(Ordering::Relaxed), 2);
    }
}
