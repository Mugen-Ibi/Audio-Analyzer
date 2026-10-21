use audio_analyzer::{
    config::{AnalysisConfig, InputSettings, Settings},
    controller::{AnalyzerController, ConnectionState},
    history::HistoryBuffer,
    model::{AnalysisSnapshot, SignalLevel},
    synthetic::SyntheticSource,
};
use std::{
    thread,
    time::{Duration, Instant},
};
fn next(c: &mut AnalyzerController) -> AnalysisSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        c.poll();
        if let Some(s) = c.take_latest() {
            return s;
        }
        assert!(Instant::now() < deadline, "{:?}", c.state());
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn changing_analysis_drops_old_generations_and_starts_fresh_average() {
    let mut c = AnalyzerController::with_source(SyntheticSource::default()).unwrap();
    c.start_configured(InputSettings::default(), AnalysisConfig::FAST)
        .unwrap();
    let before = next(&mut c);
    assert_eq!(before.config, AnalysisConfig::FAST);
    assert_eq!(before.analysis_generation, 0);
    c.set_analysis(AnalysisConfig::PRECISE).unwrap();
    let after = next(&mut c);
    assert_eq!(after.analysis_generation, 1);
    assert_eq!(after.config, AnalysisConfig::PRECISE);
    assert_eq!(after.averaged_windows, 1);
    assert!(!after.comparison_ready);
    c.stop();
    assert_eq!(c.state(), &ConnectionState::Stopping);
    assert!(c.take_latest().is_none());
}
fn s(frame: u64) -> AnalysisSnapshot {
    AnalysisSnapshot {
        window_start_frame: frame,
        sample_rate: 48000,
        measurement_level: SignalLevel {
            peak_dbfs: -6.0,
            rms_dbfs: -9.0,
        },
        ..Default::default()
    }
}
#[test]
fn history_is_time_bounded_and_retains_condition_boundaries() {
    let mut h = HistoryBuffer::default();
    h.begin_segment("M2 ch1 / FAST".into());
    for i in 0..6200 {
        h.push_at(&s(i * 4800), 0, i as f64 / 10.0);
    }
    assert!(h.points().len() <= 6001);
    assert!(h.points().front().unwrap().seconds >= 19.9);
    h.begin_segment("Built-in / PRECISE / AGC unknown".into());
    h.push_at(&s(0), 0, 620.0);
    let p = h.points().back().unwrap();
    assert!(p.gap);
    assert!(h.label(p.segment).contains("AGC unknown"));
    assert!(h.points().iter().any(|p| p.segment == 1));
    assert!(h.at(619.5).is_some());
    assert!(h.at(620.0).is_none());
    h.set_retention(60);
    h.expire(620.0);
    assert!(h.points().front().unwrap().seconds >= 560.0);
    h.expire(2000.0);
    assert!(h.points().is_empty());
}
#[test]
fn settings_save_replace_load_and_corruption_are_checked() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("audio-analyzer-settings-{}", std::process::id()));
    let path = dir.join("settings.json");
    let mut s = Settings::default();
    s.save(&path).unwrap();
    s.condition_notes = "M2 gain 10時 / AGC unknown".into();
    s.save(&path).unwrap();
    assert_eq!(
        Settings::load(&path).unwrap().condition_notes,
        s.condition_notes
    );
    std::fs::write(&path, b"invalid-json").unwrap();
    assert!(Settings::load(&path).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn cursor_uses_the_rendered_interval_and_never_borrows_a_future_value_across_a_gap() {
    let mut h = HistoryBuffer::default();
    h.push_at(&s(0), 0, 0.0);
    h.push_at(&s(5120), 0, 5120.0 / 48000.0);
    assert!(
        h.at(0.105).is_some(),
        "normal FFT hop spacing stays continuous"
    );
    h.push_at(&s(48000), 1, 1.0);
    h.push_at(&s(53120), 1, 53120.0 / 48000.0);
    assert!(h.at(0.95).is_none(), "do not borrow the next measurement");
    assert!(h.at(1.05).is_none(), "the gap marker interval is blank");
    assert!(h.at(1.11).is_some());
}

#[test]
fn precise_fft_hop_spacing_is_continuous_in_both_histories() {
    let mut history = HistoryBuffer::default();
    let mut first = s(0);
    first.config = AnalysisConfig::PRECISE;
    let mut second = s(8192);
    second.config = AnalysisConfig::PRECISE;
    history.push_at(&first, 0, 0.0);
    history.push_at(&second, 0, 8192.0 / 48000.0);
    assert!(!history.points().back().unwrap().gap);
    assert!(history.at(0.16).is_some());
}

#[test]
fn pending_slider_changes_do_not_evict_metadata_for_retained_results() {
    let mut history = HistoryBuffer::default();
    history.begin_segment("retained setup".into());
    history.push_at(&s(0), 0, 0.0);
    for i in 0..7000 {
        history.begin_segment(format!("pending setup {i}"));
    }
    assert_eq!(
        history.label(history.points().front().unwrap().segment),
        "retained setup"
    );
}
