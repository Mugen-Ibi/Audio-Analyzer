use std::{sync::atomic::Ordering, time::Duration};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

use crate::{
    model::{AnalysisSnapshot, ChannelRoute, FFT_SIZE},
    pipeline::AnalyzerRuntime,
};

pub struct AnalyzerApp {
    runtime: AnalyzerRuntime,
    reference_channel: Option<usize>,
    measurement_channel: usize,
    route_error: Option<String>,
    measurement_points: Vec<[f64; 2]>,
    transfer_points: Vec<[f64; 2]>,
    phase_points: Vec<[f64; 2]>,
    coherence_points: Vec<[f64; 2]>,
    latest_sequence: Option<u64>,
    latest_window_start: u64,
    has_reference: bool,
}

impl AnalyzerApp {
    pub fn new(runtime: AnalyzerRuntime) -> Self {
        let route = runtime.info().initial_route;
        Self {
            runtime,
            reference_channel: route.reference,
            measurement_channel: route.measurement,
            route_error: None,
            measurement_points: Vec::with_capacity(FFT_SIZE / 2),
            transfer_points: Vec::with_capacity(FFT_SIZE / 2),
            phase_points: Vec::with_capacity(FFT_SIZE / 2),
            coherence_points: Vec::with_capacity(FFT_SIZE / 2),
            latest_sequence: None,
            latest_window_start: 0,
            has_reference: route.reference.is_some(),
        }
    }

    fn update_snapshot(&mut self, snapshot: AnalysisSnapshot) {
        self.measurement_points.clear();
        self.transfer_points.clear();
        self.phase_points.clear();
        self.coherence_points.clear();

        let resolution = snapshot.sample_rate as f64 / FFT_SIZE as f64;
        for (bin, metrics) in snapshot.bins.iter().enumerate().skip(1) {
            let frequency = bin as f64 * resolution;
            let log_frequency = frequency.log10();
            self.measurement_points
                .push([log_frequency, metrics.measurement_dbfs as f64]);
            if snapshot.has_reference {
                self.transfer_points
                    .push([log_frequency, metrics.transfer_db as f64]);
                self.phase_points
                    .push([log_frequency, metrics.phase_degrees as f64]);
                self.coherence_points
                    .push([log_frequency, metrics.coherence as f64]);
            }
        }
        self.latest_sequence = Some(snapshot.sequence);
        self.latest_window_start = snapshot.window_start_frame;
        self.has_reference = snapshot.has_reference;
    }

    fn channel_controls(&mut self, ui: &mut egui::Ui) {
        let channels = self.runtime.info().channels;
        let previous_route = ChannelRoute {
            reference: self.reference_channel,
            measurement: self.measurement_channel,
        };

        ui.horizontal(|ui| {
            ui.label("Reference:");
            egui::ComboBox::from_id_salt("reference_channel")
                .selected_text(match self.reference_channel {
                    Some(channel) => format!("Input {}", channel + 1),
                    None => "Disabled".to_owned(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.reference_channel, None, "Disabled");
                    for channel in 0..channels {
                        ui.selectable_value(
                            &mut self.reference_channel,
                            Some(channel),
                            format!("Input {}", channel + 1),
                        );
                    }
                });

            ui.label("Measurement:");
            egui::ComboBox::from_id_salt("measurement_channel")
                .selected_text(format!("Input {}", self.measurement_channel + 1))
                .show_ui(ui, |ui| {
                    for channel in 0..channels {
                        ui.selectable_value(
                            &mut self.measurement_channel,
                            channel,
                            format!("Input {}", channel + 1),
                        );
                    }
                });
        });

        let new_route = ChannelRoute {
            reference: self.reference_channel,
            measurement: self.measurement_channel,
        };
        if new_route != previous_route {
            self.route_error = self.runtime.set_route(new_route).err();
        }
    }

    fn show_plot(
        ui: &mut egui::Ui,
        id: &'static str,
        points: &[[f64; 2]],
        y_min: f64,
        y_max: f64,
        height: f32,
    ) {
        let line = Line::new(PlotPoints::from_iter(points.iter().copied()))
            .color(egui::Color32::from_rgb(0, 255, 128));
        Plot::new(id)
            .height(height)
            .include_y(y_min)
            .include_y(y_max)
            .x_axis_formatter(|mark, _| format_frequency(10.0_f64.powf(mark.value)))
            .show(ui, |plot_ui| plot_ui.line(line));
    }
}

impl eframe::App for AnalyzerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(snapshot) = self.runtime.take_latest() {
            self.update_snapshot(snapshot);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let info = self.runtime.info();
            ui.heading("Pro Audio Analyzer");
            ui.label(format!(
                "{} / {} / {} Hz / {} channels",
                info.host_name, info.device_name, info.sample_rate, info.channels
            ));
            self.channel_controls(ui);

            let stats = self.runtime.stats();
            let dropped_audio = stats.dropped_audio_frames.load(Ordering::Relaxed);
            let discontinuities = stats.discontinuities.load(Ordering::Relaxed);
            let stream_errors = stats.stream_errors.load(Ordering::Relaxed);
            let dropped_results = stats.dropped_results.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                ui.label(format!(
                    "FFT: {}  Start frame: {}",
                    self.latest_sequence
                        .map_or_else(|| "waiting".to_owned(), |value| value.to_string()),
                    self.latest_window_start
                ));
                let status = format!(
                    "Audio drops: {dropped_audio}  Discontinuities: {discontinuities}  Stream errors: {stream_errors}  Display drops: {dropped_results}"
                );
                if dropped_audio > 0 || stream_errors > 0 {
                    ui.colored_label(egui::Color32::YELLOW, status);
                } else {
                    ui.label(status);
                }
            });
            if let Some(error) = &self.route_error {
                ui.colored_label(egui::Color32::RED, error);
            }
            ui.separator();

            if self.has_reference {
                ui.label("Measurement spectrum (dBFS)");
                Self::show_plot(
                    ui,
                    "measurement_spectrum",
                    &self.measurement_points,
                    -120.0,
                    6.0,
                    150.0,
                );
                ui.label("Transfer magnitude (dB)");
                Self::show_plot(
                    ui,
                    "transfer_magnitude",
                    &self.transfer_points,
                    -60.0,
                    20.0,
                    130.0,
                );
                ui.label("Transfer phase (degrees)");
                Self::show_plot(
                    ui,
                    "transfer_phase",
                    &self.phase_points,
                    -180.0,
                    180.0,
                    130.0,
                );
                ui.label("Coherence");
                Self::show_plot(
                    ui,
                    "coherence",
                    &self.coherence_points,
                    0.0,
                    1.0,
                    130.0,
                );
            } else {
                ui.label("Measurement spectrum (dBFS)");
                Self::show_plot(
                    ui,
                    "measurement_spectrum",
                    &self.measurement_points,
                    -120.0,
                    6.0,
                    480.0,
                );
            }
        });

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn format_frequency(frequency: f64) -> String {
    if frequency >= 1_000.0 {
        format!("{:.1} kHz", frequency / 1_000.0)
    } else {
        format!("{frequency:.0} Hz")
    }
}
