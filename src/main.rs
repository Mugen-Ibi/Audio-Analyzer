use audio_analyzer::{audio::CpalSource, controller::AnalyzerController, ui::AnalyzerApp};
use eframe::egui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut controller =
        AnalyzerController::with_source(CpalSource).map_err(std::io::Error::other)?;
    controller.start().map_err(std::io::Error::other)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([760.0, 640.0])
            .with_title("Pro Audio Analyzer"),
        ..Default::default()
    };

    eframe::run_native(
        "Pro Audio Analyzer",
        options,
        Box::new(move |creation_context| {
            Ok(Box::new(AnalyzerApp::new(controller, creation_context)))
        }),
    )?;
    Ok(())
}
