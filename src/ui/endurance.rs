//! Opt-in synthetic-only diagnostic. No PCM, hardware streams, or system changes.
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const SECONDS: f64 = 600.0;
pub const HEADER: &str = "elapsed_s,utc_ms,interval_ms,wall_interval_ms,awake_interval_ms,uptime_interval_ms,update_ms,previous_cpu_ms,minimized,focused,width,height,ppp,held,reference,visible_s,records,oldest_s,newest_s,gaps,audio_drops,result_drops,coalesced,received_frames,vertices,rebuild_ms,rotating,phase,live_sequence,displayed_sequence,freshness_ms";
pub type Row = [f64; 31];
#[allow(clippy::large_enum_variant)] // Fixed 2 MiB channel storage avoids per-frame Box allocations.
enum Message {
    Frame(Row),
    Event(f64, f64, &'static str),
}
pub struct Trace {
    pub start: Instant,
    pub directory: PathBuf,
    pub next: usize,
    pub complete: bool,
    pub screenshot_requested: bool,
    pub last: Option<(Instant, f64, f64, f64)>,
    pub frames: u64,
    pub dropped: u64,
    pub gaps: usize,
    pub gap_second: u64,
    tx: Option<SyncSender<Message>>,
    writer: Option<JoinHandle<io::Result<()>>>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Orbit,
    Reference,
    Hold,
    Live,
    Wide,
    Reset,
    Finish,
}
pub const SCHEDULE: &[(f64, Action)] = &[
    (120.0, Action::Orbit),
    (180.0, Action::Reference),
    (240.0, Action::Hold),
    (300.0, Action::Live),
    (360.0, Action::Wide),
    (420.0, Action::Hold),
    (450.0, Action::Live),
    (540.0, Action::Reset),
    (600.0, Action::Finish),
];
pub fn phase(t: f64) -> u8 {
    match t {
        v if v < 120.0 => 0,
        v if v < 180.0 => 1,
        v if v < 240.0 => 2,
        v if v < 300.0 => 3,
        v if v < 360.0 => 4,
        v if v < 420.0 => 5,
        v if v < 450.0 => 6,
        v if v < 540.0 => 7,
        _ => 8,
    }
}
pub fn rotating(t: f64) -> bool {
    (120.0..300.0).contains(&t) || (360.0..540.0).contains(&t)
}
pub fn utc_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        * 1000.0
}
#[cfg(windows)]
pub fn clocks() -> (f64, f64) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount64() -> u64;
        fn QueryUnbiasedInterruptTime(value: *mut u64) -> i32;
    }
    let mut unbiased = 0;
    // Read-only OS counters. No timer-resolution or power-policy changes.
    unsafe {
        let ok = QueryUnbiasedInterruptTime(&mut unbiased);
        (
            GetTickCount64() as f64,
            if ok != 0 {
                unbiased as f64 / 10000.0
            } else {
                -1.0
            },
        )
    }
}
#[cfg(not(windows))]
pub fn clocks() -> (f64, f64) {
    (-1.0, -1.0)
}
impl Trace {
    pub fn open(directory: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        let frames = File::create_new(directory.join("frames.csv"))?;
        let events = File::create_new(directory.join("events.csv"))?;
        let (tx, rx) = mpsc::sync_channel(8192);
        let writer = thread::Builder::new()
            .name("synthetic-diagnostic-writer".into())
            .spawn(move || {
                let mut frames = BufWriter::with_capacity(262144, frames);
                let mut events = BufWriter::new(events);
                writeln!(frames, "{HEADER}")?;
                writeln!(events, "elapsed_s,utc_ms,event")?;
                let mut last_flush = Instant::now();
                while let Ok(message) = rx.recv() {
                    match message {
                        Message::Frame(row) => {
                            for (i, value) in row.iter().enumerate() {
                                if i > 0 {
                                    write!(frames, ",")?;
                                }
                                write!(frames, "{value:.6}")?;
                            }
                            writeln!(frames)?;
                        }
                        Message::Event(t, wall, name) => {
                            writeln!(events, "{t:.6},{wall:.3},{name}")?;
                            events.flush()?;
                        }
                    }
                    if last_flush.elapsed().as_secs_f64() >= 1.0 {
                        frames.flush()?;
                        last_flush = Instant::now();
                    }
                }
                frames.flush()?;
                events.flush()
            })?;
        let trace = Self {
            start: Instant::now(),
            directory: directory.to_owned(),
            next: 0,
            complete: false,
            screenshot_requested: false,
            last: None,
            frames: 0,
            dropped: 0,
            gaps: 0,
            gap_second: u64::MAX,
            tx: Some(tx),
            writer: Some(writer),
        };
        trace.event("measurement_start");
        Ok(trace)
    }
    pub fn elapsed(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    pub fn event(&self, name: &'static str) {
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(Message::Event(self.elapsed(), utc_ms(), name));
        }
    }
    pub fn frame(&mut self, row: Row) {
        self.frames += 1;
        if self
            .tx
            .as_ref()
            .is_some_and(|tx| tx.try_send(Message::Frame(row)).is_err())
        {
            self.dropped += 1;
        }
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(writer) = self.writer.take() {
            match writer.join() {
                Ok(Ok(())) => {}
                other => eprintln!("Diagnostic writer completion: {other:?}"),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_schedule_keeps_3d_and_finishes_once_at_ten_minutes() {
        assert_eq!(SCHEDULE.last(), Some(&(600.0, Action::Finish)));
        assert!(SCHEDULE.windows(2).all(|s| s[0].0 < s[1].0));
        assert!(rotating(240.0));
        assert!(!rotating(550.0));
        assert_eq!(phase(599.0), 8);
        assert_eq!(HEADER.split(',').count(), 31);
    }
}
