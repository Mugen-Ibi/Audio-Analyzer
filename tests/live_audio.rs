//! Opt-in test: opens the default input device, but never records audio to disk.
use audio_analyzer::{
    audio::CpalSource,
    controller::{AnalyzerController, ConnectionState},
    model::{AnalysisSnapshot, ChannelRoute},
};
use std::{
    thread,
    time::{Duration, Instant},
};

fn wait_for_snapshot(runtime: &mut AnalyzerController) -> AnalysisSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        runtime.poll();
        assert!(
            !matches!(runtime.state(), ConnectionState::Failed(_)),
            "input failed: {:?}",
            runtime.state()
        );
        if let Some(snapshot) = runtime.take_latest() {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "no audio analysis received within five seconds: received={}, dropped={}, errors={}",
            runtime
                .stats()
                .received_audio_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            runtime
                .stats()
                .dropped_audio_frames
                .load(std::sync::atomic::Ordering::Relaxed),
            runtime
                .stats()
                .stream_errors
                .load(std::sync::atomic::Ordering::Relaxed)
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires a working input device; opens the default microphone"]
fn live_input_route_switch_and_shutdown() {
    let mut runtime = AnalyzerController::with_source(CpalSource).unwrap();
    runtime.start().unwrap();
    let initial = wait_for_snapshot(&mut runtime);
    println!("Input: {:?}", runtime.info().unwrap());
    println!("Initial snapshot received");
    assert_eq!(initial.sample_rate, runtime.info().unwrap().sample_rate);
    runtime
        .set_route(ChannelRoute {
            reference: None,
            measurement: 0,
        })
        .unwrap();
    let mono = wait_for_snapshot(&mut runtime);
    println!("Mono snapshot received");
    assert!(!mono.has_reference);
    assert_ne!(mono.route_generation, initial.route_generation);
    assert!(mono.measurement_level.rms_dbfs.is_finite());
    runtime
        .set_route(runtime.info().unwrap().initial_route)
        .unwrap();
    let restored = wait_for_snapshot(&mut runtime);
    assert_eq!(
        restored.has_reference,
        runtime.info().unwrap().initial_route.reference.is_some()
    );
    assert_ne!(restored.route_generation, mono.route_generation);
    runtime.stop();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        runtime.poll();
        if runtime.state() == &ConnectionState::Stopped {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "input did not stop: {:?}",
            runtime.state()
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(runtime.take_latest().is_none());
    runtime.start().unwrap();
    let restarted = wait_for_snapshot(&mut runtime);
    assert_eq!(restarted.route_generation, 0);
    assert_eq!(restarted.sample_rate, runtime.info().unwrap().sample_rate);
    println!("Reconnected snapshot received through the control thread");
    drop(runtime);
}
