//! Input port. Implementations own their stream on the thread that opened it.
use std::sync::{Arc, atomic::AtomicBool};

use crate::{
    capture::CapturedInput,
    model::{ChannelRoute, RuntimeStats},
};

#[derive(Clone, Debug)]
pub struct AudioInfo {
    pub host_name: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub initial_route: ChannelRoute,
}

impl AudioInfo {
    pub fn validate(&self) -> Result<(), String> {
        if self.sample_rate == 0 || self.channels == 0 || self.channels > u16::MAX as usize {
            return Err("input requires a nonzero sample rate and 1..=65535 channels".to_owned());
        }
        self.initial_route.validate(self.channels)?;
        Ok(())
    }
}

/// Only the factory crosses threads. The opened stream need not implement Send.
pub trait InputSource {
    fn open(
        &mut self,
        stats: Arc<RuntimeStats>,
        cancelled: &AtomicBool,
    ) -> Result<OpenedSource, String>;
}

/// Stop must quiesce callbacks before returning. Called once by OpenedSource.
pub trait InputStream {
    fn stop(&mut self);
}

pub struct OpenedSource {
    pub(crate) info: AudioInfo,
    pub(crate) input: Option<CapturedInput>,
    stream: Option<Box<dyn InputStream>>,
}

impl OpenedSource {
    pub fn new(
        info: AudioInfo,
        input: CapturedInput,
        stream: Box<dyn InputStream>,
    ) -> Result<Self, String> {
        let source = Self {
            info,
            input: Some(input),
            stream: Some(stream),
        };
        source.info.validate()?;
        let input = source.input.as_ref().expect("input was just set");
        if input.channels != source.info.channels
            || input.route_control.load().0 != source.info.initial_route
        {
            return Err("source metadata does not match its capture channel".to_owned());
        }
        Ok(source)
    }

    pub fn info(&self) -> &AudioInfo {
        &self.info
    }

    pub fn stop(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.stop();
        }
    }
}

impl Drop for OpenedSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    struct LocalStream(Rc<Cell<usize>>);
    impl InputStream for LocalStream {
        fn stop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn thread_local_stream_is_stopped_exactly_once() {
        let stops = Rc::new(Cell::new(0));
        let route = ChannelRoute::default_for_channels(2);
        let (_writer, input) = crate::capture::channel(2, route, Arc::default()).unwrap();
        let mut source = OpenedSource::new(
            AudioInfo {
                host_name: "local".into(),
                device_name: "local".into(),
                sample_rate: 48_000,
                channels: 2,
                initial_route: route,
            },
            input,
            Box::new(LocalStream(Rc::clone(&stops))),
        )
        .unwrap();
        source.stop();
        source.stop();
        drop(source);
        assert_eq!(stops.get(), 1);
    }

    #[test]
    fn incompatible_metadata_is_rejected_and_stream_is_stopped() {
        let stops = Rc::new(Cell::new(0));
        let route = ChannelRoute::default_for_channels(2);
        let (_writer, input) = crate::capture::channel(2, route, Arc::default()).unwrap();
        let result = OpenedSource::new(
            AudioInfo {
                host_name: "invalid".into(),
                device_name: "invalid".into(),
                sample_rate: 48_000,
                channels: 3,
                initial_route: route,
            },
            input,
            Box::new(LocalStream(Rc::clone(&stops))),
        );
        assert!(result.is_err());
        assert_eq!(stops.get(), 1);
    }
}
