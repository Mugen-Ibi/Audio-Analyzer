use audio_analyzer::{pipeline::AnalyzerRuntime, ui::AnalyzerApp};
use eframe::egui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = AnalyzerRuntime::start().map_err(std::io::Error::other)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 900.0])
            .with_title("Pro Audio Analyzer"),
        ..Default::default()
    };

    eframe::run_native(
        "Pro Audio Analyzer",
        options,
        Box::new(move |_creation_context| Ok(Box::new(AnalyzerApp::new(runtime)))),
    )?;
    Ok(())
}
