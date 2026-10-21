use audio_analyzer::{
    audio::{CpalSource, enumerate_inputs},
    config::Settings,
    controller::AnalyzerController,
    synthetic::SyntheticSource,
    ui::{AnalyzerApp, AppOptions},
};
use eframe::egui;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list-inputs") {
        let (devices, errors) = enumerate_inputs();
        println!("Read-only input inventory; no capture/output stream opened.");
        for device in devices {
            println!("{} / {} / id: {}", device.host, device.name, device.id);
            for format in device.formats {
                println!("  {format}");
            }
        }
        for error in errors {
            eprintln!("{error}");
        }
        return Ok(());
    }
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let demo = args.iter().any(|a| a == "--demo");
    let screenshot = value("--qa-screenshot").map(PathBuf::from);
    let qa_idle = args.iter().any(|a| a == "--qa-idle");
    if screenshot.is_some() && !demo && !qa_idle {
        return Err(
            "native QA requires --demo or --qa-idle; audio capture is never auto-started".into(),
        );
    }
    let settings_path = value("--settings").map(PathBuf::from).or_else(|| {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("AudioAnalyzer/settings.json"))
    });
    let (settings, notice) = if demo || qa_idle {
        (Settings::default(), String::new())
    } else if let Some(path) = settings_path.as_ref().filter(|p| p.exists()) {
        match Settings::load(path) {
            Ok(s) => (s, String::new()),
            Err(e) => (
                Settings::default(),
                format!("保存条件を復元できません: {e}。入力停止中に確認してください。"),
            ),
        }
    } else {
        (Settings::default(), String::new())
    };
    let mut controller = if demo {
        AnalyzerController::with_source(SyntheticSource::default())
    } else {
        AnalyzerController::with_source(CpalSource::default())
    }
    .map_err(std::io::Error::other)?;
    // Only explicit synthetic demo mode starts itself. Normal launch stays stopped.
    if demo {
        controller
            .start_configured(settings.input.clone(), settings.analysis)
            .map_err(std::io::Error::other)?;
    }
    let width = value("--width")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(1440.0)
        .max(760.0);
    let height = value("--height")
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(1000.0)
        .max(640.0);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_min_inner_size([760.0, 640.0])
            .with_title(if demo {
                "Audio Visualizer — SYNTHETIC"
            } else {
                "Audio Visualizer"
            }),
        ..Default::default()
    };
    let qa = screenshot.is_some();
    let app_options = AppOptions {
        settings,
        settings_path: if demo || qa_idle { None } else { settings_path },
        initial_notice: notice,
        demo,
        screenshot,
        open_settings: args.iter().any(|a| a == "--qa-settings"),
        qa_hold: demo && qa && args.iter().any(|a| a == "--qa-hold"),
        qa_history: qa && args.iter().any(|a| a == "--qa-history"),
        qa_settings_page: value("--qa-settings-page")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        qa_zoom: if qa {
            value("--qa-zoom")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1.0)
        } else {
            1.0
        },
        qa_delay_seconds: value("--qa-delay")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5.0),
        qa_2d: qa && args.iter().any(|a| a == "--qa-2d"),
        qa_orbit: qa && args.iter().any(|a| a == "--qa-orbit"),
        qa_reference: qa && args.iter().any(|a| a == "--qa-reference"),
        qa_cursor: qa && args.iter().any(|a| a == "--qa-cursor"),
        qa_duration: value("--qa-duration")
            .and_then(|s| s.parse().ok())
            .unwrap_or(60.0),
        qa_minimum_dbfs: if qa {
            value("--qa-minimum-dbfs").and_then(|s| s.parse().ok())
        } else {
            None
        },
    };
    eframe::run_native(
        "Audio Visualizer",
        options,
        Box::new(move |cc| Ok(Box::new(AnalyzerApp::new(controller, cc, app_options)))),
    )?;
    Ok(())
}
