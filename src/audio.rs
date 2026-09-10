//! CPAL adapter. All driver calls are confined to the session owner thread.
pub use crate::source::AudioInfo;
use crate::{
    capture::{self, CaptureWriter},
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
pub struct CpalSource;

impl InputSource for CpalSource {
    fn open(
        &mut self,
        stats: Arc<RuntimeStats>,
        cancelled: &AtomicBool,
    ) -> Result<OpenedSource, String> {
        let mut failures = Vec::new();
        for host in candidate_hosts() {
            if cancelled.load(Ordering::Acquire) {
                return Err("input start cancelled".to_owned());
            }
            let host_name = format!("{:?}", host.id());
            match start_on_host(host, Arc::clone(&stats), cancelled) {
                Ok(source) => return Ok(source),
                Err(error) => failures.push(format!("{host_name}: {error}")),
            }
        }
        Err(format!(
            "No usable input stream was found. {}",
            failures.join(" | ")
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
) -> Result<OpenedSource, String> {
    let device = host
        .default_input_device()
        .ok_or_else(|| "no default input device".to_owned())?;
    let supported = device
        .default_input_config()
        .map_err(|error| format!("could not read the default input config: {error}"))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let info = AudioInfo {
        host_name: format!("{:?}", host.id()),
        device_name: device.to_string(),
        sample_rate: config.sample_rate,
        channels: config.channels as usize,
        initial_route: ChannelRoute::default_for_channels(config.channels as usize),
    };
    info.validate()?;
    let (writer, input) = capture::channel(info.channels, info.initial_route, Arc::clone(&stats))?;
    let received_before = stats.received_audio_frames.load(Ordering::Relaxed);
    let stream =
        build_stream_for_format(sample_format, &device, config, writer, Arc::clone(&stats))?;
    stream
        .play()
        .map_err(|error| format!("could not start the input stream: {error}"))?;
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
