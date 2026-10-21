//! CPAL adapter. All driver calls are confined to the session owner thread.
pub use crate::source::AudioInfo;
use crate::{
    capture::{self, CaptureWriter},
    config::InputSettings,
    model::{ChannelRoute, FFT_SIZE, RuntimeStats},
    source::{InputSource, InputStream, OpenedSource},
};
use cpal::{
    FromSample, Sample, SampleFormat, SizedSample, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct CpalSource {
    settings: InputSettings,
}

impl InputSource for CpalSource {
    fn configure(&mut self, settings: &InputSettings) -> Result<(), String> {
        settings.validate()?;
        self.settings = settings.clone();
        Ok(())
    }
    fn open(
        &mut self,
        stats: Arc<RuntimeStats>,
        cancelled: &AtomicBool,
    ) -> Result<OpenedSource, String> {
        let mut failures = Vec::new();
        for host in candidate_hosts() {
            if self
                .settings
                .host
                .as_ref()
                .is_some_and(|name| *name != format!("{:?}", host.id()))
            {
                continue;
            }
            if cancelled.load(Ordering::Acquire) {
                return Err("input start cancelled".to_owned());
            }
            let host_name = format!("{:?}", host.id());
            stats.reset_startup_attempt();
            match start_on_host(host, Arc::clone(&stats), cancelled, &self.settings) {
                Ok(mut source) => {
                    if !failures.is_empty() {
                        source.info.selection_notes =
                            format!("Startup fallback: {}", failures.join(" | "));
                    }
                    return Ok(source);
                }
                Err(error) => failures.push(format!("{host_name}: {error}")),
            }
        }
        Err(format!(
            "No usable input stream was found. {}",
            if failures.is_empty() {
                format!(
                    "selected backend {:?} is unavailable in this build",
                    self.settings.host
                )
            } else {
                failures.join(" | ")
            }
        ))
    }
}

struct CpalStream(Option<cpal::Stream>);
impl InputStream for CpalStream {
    fn stop(&mut self) {
        self.0.take();
    }
}

fn candidate_hosts() -> Vec<cpal::Host> {
    let mut hosts: Vec<cpal::Host> = Vec::new();
    #[cfg(all(target_os = "windows", feature = "asio"))]
    if let Ok(host) = cpal::host_from_id(cpal::HostId::Asio) {
        hosts.push(host);
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
    cancelled: &AtomicBool,
    settings: &InputSettings,
) -> Result<OpenedSource, String> {
    let device = if let Some(id) = &settings.device_id {
        host.input_devices()
            .map_err(|e| format!("enumerate input: {e}"))?
            .find(|d| d.id().is_ok_and(|found| found.to_string() == *id))
            .ok_or_else(|| {
                format!(
                    "saved input '{}' is absent; select a device and retry",
                    settings.device_name.as_deref().unwrap_or(id)
                )
            })?
    } else {
        host.default_input_device().ok_or_else(|| {
            "no default input device; check Windows microphone permission and device connection"
                .to_owned()
        })?
    };
    let default = device
        .default_input_config()
        .map_err(|e| format!("read input format: {e}"))?;
    let supported = if let Some(rate) = settings.sample_rate {
        select_rate(
            device
                .supported_input_configs()
                .map_err(|e| format!("query rates: {e}"))?,
            rate,
            default.channels(),
            default.sample_format(),
        )?
    } else {
        default
    };
    let sample_format = supported.sample_format();
    if let Some(buffer) = settings.buffer_frames
        && let cpal::SupportedBufferSize::Range { min, max } = supported.buffer_size()
        && (buffer < *min || buffer > *max)
    {
        return Err(format!("buffer {buffer} frames is outside {min}..={max}"));
    }
    let mut config: StreamConfig = supported.into();
    config.buffer_size = settings
        .buffer_frames
        .map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed);
    let route = settings
        .route
        .unwrap_or_else(|| ChannelRoute::default_for_channels(config.channels as usize));
    let mut info = AudioInfo {
        host_name: format!("{:?}", host.id()),
        device_name: device.to_string(),
        sample_rate: config.sample_rate,
        channels: config.channels as usize,
        initial_route: route,
        buffer_frames: None,
        selection_notes: String::new(),
    };
    info.validate()?;
    let (writer, input) = capture::channel(info.channels, info.initial_route, Arc::clone(&stats))?;
    let received_before = stats.received_audio_frames.load(Ordering::Relaxed);
    let stream =
        build_stream_for_format(sample_format, &device, config, writer, Arc::clone(&stats))?;
    stream
        .play()
        .map_err(|error| format!("could not start the input stream: {error}"))?;
    info.buffer_frames = stream.buffer_size().ok();
    let source = OpenedSource::new(info, input, Box::new(CpalStream(Some(stream))))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while stats
        .received_audio_frames
        .load(Ordering::Relaxed)
        .wrapping_sub(received_before)
        < FFT_SIZE as u64
    {
        if cancelled.load(Ordering::Acquire) {
            return Err("input start cancelled".to_owned());
        }
        if Instant::now() >= deadline {
            return Err(
                "input stream did not deliver a complete analysis window within two seconds"
                    .to_owned(),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(source)
}

fn build_stream_for_format(
    sample_format: SampleFormat,
    device: &cpal::Device,
    config: StreamConfig,
    writer: CaptureWriter,
    stats: Arc<RuntimeStats>,
) -> Result<cpal::Stream, String> {
    macro_rules! build {
        ($sample:ty) => {
            build_input_stream::<$sample>(device, config, writer, stats)
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
    mut writer: CaptureWriter,
    stats: Arc<RuntimeStats>,
) -> Result<cpal::Stream, String>
where
    T: Sample + SizedSample + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| writer.write_interleaved(data, |sample| sample.to_sample()),
            move |_error| {
                stats.stream_errors.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|error| format!("could not build the input stream: {error}"))
}

/// Read-only discovery. No stream is built or played. Call on a worker, never UI tick.
#[derive(Clone, Debug)]
pub struct InputDeviceDescription {
    pub host: String,
    pub id: String,
    pub name: String,
    pub formats: Vec<String>,
}
pub fn enumerate_inputs() -> (Vec<InputDeviceDescription>, Vec<String>) {
    let mut devices = Vec::new();
    let mut errors = Vec::new();
    for host in candidate_hosts() {
        let host_name = format!("{:?}", host.id());
        match host.input_devices() {
            Ok(inputs) => {
                for device in inputs {
                    let id = match device.id() {
                        Ok(id) => id.to_string(),
                        Err(e) => {
                            errors.push(format!("{host_name}: {e}"));
                            continue;
                        }
                    };
                    let formats = match device.supported_input_configs() {
                        Ok(configs) => configs
                            .map(|c| {
                                format!(
                                    "{} ch / {}–{} Hz / {:?} / {:?}",
                                    c.channels(),
                                    c.min_sample_rate(),
                                    c.max_sample_rate(),
                                    c.sample_format(),
                                    c.buffer_size()
                                )
                            })
                            .collect(),
                        Err(e) => vec![format!("format query failed: {e}")],
                    };
                    devices.push(InputDeviceDescription {
                        host: host_name.clone(),
                        id,
                        name: device.to_string(),
                        formats,
                    });
                }
            }
            Err(e) => errors.push(format!("{host_name}: {e}")),
        }
    }
    (devices, errors)
}

fn select_rate(
    configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    rate: u32,
    channels: u16,
    format: SampleFormat,
) -> Result<cpal::SupportedStreamConfig, String> {
    configs
        .filter(|c| c.min_sample_rate() <= rate && rate <= c.max_sample_rate())
        .min_by_key(|c| {
            (
                usize::from(c.channels() != channels),
                usize::from(c.sample_format() != format),
            )
        })
        .map(|c| c.with_sample_rate(rate))
        .ok_or_else(|| {
            format!("{rate} Hz is not advertised by this input; choose automatic or another rate")
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_rate_preserves_float_format_and_rejects_unsupported_rate() {
        let configs = || {
            [SampleFormat::U8, SampleFormat::I16, SampleFormat::F32]
                .into_iter()
                .map(|format| {
                    cpal::SupportedStreamConfigRange::new(
                        2,
                        48000,
                        48000,
                        cpal::SupportedBufferSize::Unknown,
                        format,
                    )
                })
        };
        assert_eq!(
            select_rate(configs(), 48000, 2, SampleFormat::F32)
                .unwrap()
                .sample_format(),
            SampleFormat::F32
        );
        assert!(
            select_rate(configs(), 96000, 2, SampleFormat::F32)
                .unwrap_err()
                .contains("not advertised")
        );
    }
}
