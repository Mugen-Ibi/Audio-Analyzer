//! Deterministic demo input. Never opens an audio device or an output stream.
use crate::{
    capture,
    config::InputSettings,
    model::{AUDIO_BLOCK_FRAMES, ChannelRoute, RuntimeStats},
    source::{AudioInfo, InputSource, InputStream, OpenedSource},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct SyntheticSource {
    route: Option<ChannelRoute>,
}
struct SyntheticStream {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}
impl InputStream for SyntheticStream {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
impl InputSource for SyntheticSource {
    fn configure(&mut self, settings: &InputSettings) -> Result<(), String> {
        self.route = settings.route;
        Ok(())
    }
    fn open(&mut self, stats: Arc<RuntimeStats>, _: &AtomicBool) -> Result<OpenedSource, String> {
        let rate = 48_000;
        let route = self
            .route
            .unwrap_or(ChannelRoute::default_for_channels(2))
            .validate(2)?;
        let (mut writer, input) = capture::channel(2, route, stats)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("synthetic-input".into())
            .spawn(move || {
                let mut position = 0_u64;
                let start = Instant::now();
                let mut data = [0.0_f32; AUDIO_BLOCK_FRAMES * 2];
                while !stopped.load(Ordering::Acquire) {
                    for pair in data.as_chunks_mut::<2>().0 {
                        let seconds = position as f64 / rate as f64;
                        let envelope = if (seconds as u64 / 3).is_multiple_of(2) {
                            0.5
                        } else {
                            0.1
                        };
                        pair[0] =
                            (envelope * (std::f64::consts::TAU * 750.0 * seconds).sin()) as f32;
                        pair[1] =
                            (envelope * 0.5 * (std::f64::consts::TAU * 1500.0 * seconds).sin())
                                as f32;
                        position += 1;
                    }
                    writer.write_interleaved(&data, |s| s);
                    if let Some(wait) = Duration::from_secs_f64(position as f64 / rate as f64)
                        .checked_sub(start.elapsed())
                    {
                        thread::sleep(wait);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        OpenedSource::new(
            AudioInfo {
                host_name: "SYNTHETIC — no hardware".into(),
                device_name: "750 / 1500 Hz deterministic demo".into(),
                sample_rate: rate,
                channels: 2,
                initial_route: route,
                buffer_frames: Some(AUDIO_BLOCK_FRAMES as u32),
                selection_notes: "Test data only. No microphone capture or speaker output.".into(),
            },
            input,
            Box::new(SyntheticStream {
                stop,
                handle: Some(handle),
            }),
        )
    }
}
