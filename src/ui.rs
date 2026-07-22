mod state;
mod theme;
mod widgets;

use std::{sync::atomic::Ordering, time::Duration};

use eframe::egui::{
    self, Align, CentralPanel, Frame, Layout, RichText, ScrollArea, SidePanel, Stroke,
    TopBottomPanel, ViewportCommand, vec2,
};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::{
    model::{AnalysisSnapshot, ChannelRoute, FFT_SIZE, SignalLevel, StereoFrame},
    pipeline::AnalyzerRuntime,
};

use self::{
    state::{AnalysisView, DisplayHold, HistoryBuffer, PeakHold},
    theme::{BACKGROUND, CYAN, GREEN, GREEN_BRIGHT, OUTLINE, RED, SURFACE, TEXT_MUTED},
};

pub struct AnalyzerApp {
    runtime: AnalyzerRuntime,
    reference_channel: Option<usize>,
    measurement_channel: usize,
    route_error: Option<String>,
    reference_points: Vec<[f64; 2]>,
    measurement_points: Vec<[f64; 2]>,
    transfer_points: Vec<[f64; 2]>,
    phase_points: Vec<[f64; 2]>,
    coherence_points: Vec<[f64; 2]>,
    scope_points: Vec<StereoFrame>,
    measurement_level: SignalLevel,
    reference_level: Option<SignalLevel>,
    phase_correlation: Option<f32>,
    measurement_peak_hold: PeakHold,
    reference_peak_hold: PeakHold,
    measurement_held_dbfs: f32,
    reference_held_dbfs: f32,
    history: HistoryBuffer,
    display_hold: DisplayHold,
    analysis_view: AnalysisView,
    latest_sequence: Option<u64>,
    latest_window_start: u64,
    has_reference: bool,
    show_routing: bool,
    show_settings: bool,
    show_about: bool,
    show_top_cards: bool,
    show_timeline: bool,
    history_only: bool,
    fullscreen: bool,
}

impl AnalyzerApp {
    pub fn new(runtime: AnalyzerRuntime, creation_context: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation_context.egui_ctx);
        let route = runtime.info().initial_route;
        Self {
            runtime,
            reference_channel: route.reference,
            measurement_channel: route.measurement,
            route_error: None,
            reference_points: Vec::with_capacity(FFT_SIZE / 2),
            measurement_points: Vec::with_capacity(FFT_SIZE / 2),
            transfer_points: Vec::with_capacity(FFT_SIZE / 2),
            phase_points: Vec::with_capacity(FFT_SIZE / 2),
            coherence_points: Vec::with_capacity(FFT_SIZE / 2),
            scope_points: Vec::with_capacity(crate::model::VECTOR_SCOPE_POINTS),
            measurement_level: SignalLevel::default(),
            reference_level: None,
            phase_correlation: None,
            measurement_peak_hold: PeakHold::default(),
            reference_peak_hold: PeakHold::default(),
            measurement_held_dbfs: -120.0,
            reference_held_dbfs: -120.0,
            history: HistoryBuffer::default(),
            display_hold: DisplayHold::default(),
            analysis_view: AnalysisView::Spectrum,
            latest_sequence: None,
            latest_window_start: 0,
            has_reference: route.reference.is_some(),
            show_routing: false,
            show_settings: false,
            show_about: false,
            show_top_cards: true,
            show_timeline: true,
            history_only: false,
            fullscreen: false,
        }
    }

    fn update_snapshot(&mut self, snapshot: AnalysisSnapshot, now: f64, discontinuities: u64) {
        self.history.push(&snapshot, discontinuities);
        self.measurement_level = snapshot.measurement_level;
        self.reference_level = snapshot.reference_level;
        self.phase_correlation = snapshot.phase_correlation;
        self.measurement_held_dbfs = self
            .measurement_peak_hold
            .update(snapshot.measurement_level.peak_dbfs, now);
        self.reference_held_dbfs = snapshot.reference_level.map_or(-120.0, |level| {
            self.reference_peak_hold.update(level.peak_dbfs, now)
        });
        self.scope_points.clear();
        self.scope_points
            .extend_from_slice(&snapshot.scope_points[..snapshot.scope_len]);

        if self.display_hold.accepts_plot_update() {
            self.reference_points.clear();
            self.measurement_points.clear();
            self.transfer_points.clear();
            self.phase_points.clear();
            self.coherence_points.clear();
            let resolution = snapshot.sample_rate as f64 / FFT_SIZE as f64;
            for (bin, metrics) in snapshot.bins.iter().enumerate().skip(1) {
                let frequency = bin as f64 * resolution;
                if frequency < 20.0 {
                    continue;
                }
                let log_frequency = frequency.log10();
                self.measurement_points
                    .push([log_frequency, metrics.measurement_dbfs as f64]);
                if snapshot.has_reference {
                    self.reference_points
                        .push([log_frequency, metrics.reference_dbfs as f64]);
                    self.transfer_points
                        .push([log_frequency, metrics.transfer_db as f64]);
                    self.phase_points
                        .push([log_frequency, metrics.phase_degrees as f64]);
                    self.coherence_points
                        .push([log_frequency, metrics.coherence as f64]);
                }
            }
        }

        self.latest_sequence = Some(snapshot.sequence);
        self.latest_window_start = snapshot.window_start_frame;
        self.has_reference = snapshot.has_reference;
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        TopBottomPanel::top("instrument_top_bar")
            .exact_height(40.0)
            .frame(
                Frame::none()
                    .fill(BACKGROUND)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("PRO AUDIO ANALYZER")
                            .font(theme::bold(18.0))
                            .color(GREEN_BRIGHT),
                    );
                    ui.add_space(22.0);
                    ui.menu_button("File", |ui| {
                        if ui.button("Exit").clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("View", |ui| {
                        ui.checkbox(&mut self.show_top_cards, "Signal cards");
                        ui.checkbox(&mut self.show_timeline, "Timeline history");
                    });
                    if ui.button("Settings").clicked() {
                        self.show_settings = true;
                    }
                    if ui.button("Routing").clicked() {
                        self.show_routing = true;
                    }
                    if ui.button("Help").clicked() {
                        self.show_about = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        if ui
                            .button(RichText::new("PWR").color(GREEN).font(theme::bold(10.0)))
                            .on_hover_text("Close application")
                            .clicked()
                        {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui
                            .button(RichText::new("FULL").color(CYAN).font(theme::bold(10.0)))
                            .clicked()
                        {
                            self.fullscreen = !self.fullscreen;
                            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
                        }
                        if ui
                            .button(RichText::new("CFG").color(GREEN).font(theme::bold(10.0)))
                            .clicked()
                        {
                            self.show_settings = true;
                        }
                    });
                });
            });
    }

    fn side_bar(&mut self, ctx: &egui::Context) {
        SidePanel::left("instrument_side_bar")
            .exact_width(80.0)
            .resizable(false)
            .frame(
                Frame::none()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("AUDIO_CORE")
                            .font(theme::bold(8.0))
                            .color(GREEN_BRIGHT),
                    );
                    ui.add_space(16.0);
                    if widgets::nav_item(ui, "I/O", "INPUT", false, true).clicked() {
                        self.show_routing = true;
                    }
                    let _ = widgets::nav_item(ui, "--", "OUTPUT", false, false);
                    let _ = widgets::nav_item(ui, "EQ", "FILTERS", false, false);
                    if widgets::nav_item(ui, "~", "ANALYSIS", !self.history_only, true).clicked() {
                        self.history_only = false;
                    }
                    if widgets::nav_item(ui, "H", "HISTORY", self.history_only, true).clicked() {
                        self.history_only = true;
                    }
                    let _ = widgets::nav_item(ui, "EX", "EXPORT", false, false);
                });
            });
    }

    fn status_bar(&self, ctx: &egui::Context) {
        TopBottomPanel::bottom("instrument_status_bar")
            .exact_height(24.0)
            .frame(
                Frame::none()
                    .fill(BACKGROUND)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    let stats = self.runtime.stats();
                    let stream_errors = stats.stream_errors.load(Ordering::Relaxed);
                    let dropped_audio = stats.dropped_audio_frames.load(Ordering::Relaxed);
                    ui.colored_label(
                        if stream_errors == 0 { GREEN } else { RED },
                        if stream_errors == 0 {
                            "● SYSTEM_READY"
                        } else {
                            "● STREAM_ERROR"
                        },
                    );
                    ui.label(format!("AUDIO_DROPS: {dropped_audio}"));
                    ui.label(format!(
                        "DISPLAY_DROPS: {}",
                        stats.dropped_results.load(Ordering::Relaxed)
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        let info = self.runtime.info();
                        ui.colored_label(
                            CYAN,
                            format!("{} Hz / {} ch", info.sample_rate, info.channels),
                        );
                        ui.label(&info.device_name);
                    });
                });
            });
    }

    fn dashboard(&mut self, ctx: &egui::Context) {
        CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.history_only {
                            widgets::module(
                                ui,
                                "TIMELINE_HISTORY",
                                true,
                                ui.available_height().max(500.0),
                                |ui| {
                                    ui.label(
                                        RichText::new("BUFFER: 300s")
                                            .font(theme::bold(9.0))
                                            .color(GREEN),
                                    );
                                },
                                |ui| widgets::history_plot(ui, self.history.points(), 520.0),
                            );
                            return;
                        }

                        if self.show_top_cards {
                            self.top_cards(ui);
                            ui.add_space(8.0);
                        }
                        let timeline_space = if self.show_timeline { 180.0 } else { 0.0 };
                        let plot_height = (ui.available_height() - timeline_space - 8.0).max(310.0);
                        self.analysis_module(ui, plot_height);
                        if self.show_timeline {
                            ui.add_space(8.0);
                            self.timeline_module(ui, 170.0);
                        }
                    });
            });
    }

    fn top_cards(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        if width >= 1050.0 {
            let card_width = (width - 16.0) / 3.0;
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(card_width, 210.0),
                    Layout::top_down(Align::Min),
                    |ui| self.signal_metrics(ui),
                );
                ui.allocate_ui_with_layout(
                    vec2(card_width, 210.0),
                    Layout::top_down(Align::Min),
                    |ui| self.peak_meters(ui),
                );
                ui.allocate_ui_with_layout(
                    vec2(card_width, 210.0),
                    Layout::top_down(Align::Min),
                    |ui| self.phase_scope(ui),
                );
            });
        } else if width >= 700.0 {
            let card_width = (width - 8.0) / 2.0;
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(card_width, 210.0),
                    Layout::top_down(Align::Min),
                    |ui| self.signal_metrics(ui),
                );
                ui.allocate_ui_with_layout(
                    vec2(card_width, 210.0),
                    Layout::top_down(Align::Min),
                    |ui| self.peak_meters(ui),
                );
            });
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(width, 210.0), Layout::top_down(Align::Min), |ui| {
                self.phase_scope(ui)
            });
        } else {
            ui.allocate_ui_with_layout(vec2(width, 210.0), Layout::top_down(Align::Min), |ui| {
                self.signal_metrics(ui)
            });
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(width, 210.0), Layout::top_down(Align::Min), |ui| {
                self.peak_meters(ui)
            });
            ui.add_space(8.0);
            ui.allocate_ui_with_layout(vec2(width, 210.0), Layout::top_down(Align::Min), |ui| {
                self.phase_scope(ui)
            });
        }
    }

    fn signal_metrics(&mut self, ui: &mut egui::Ui) {
        widgets::module(
            ui,
            "SIGNAL_METRICS",
            false,
            208.0,
            |_| {},
            |ui| {
                ui.columns(3, |columns| {
                    widgets::metric(
                        &mut columns[0],
                        "MEAS RMS",
                        Some(self.measurement_level.rms_dbfs),
                        "dBFS",
                        CYAN,
                    );
                    widgets::metric(
                        &mut columns[1],
                        "REF RMS",
                        self.reference_level.map(|level| level.rms_dbfs),
                        "dBFS",
                        GREEN,
                    );
                    widgets::metric(
                        &mut columns[2],
                        "CORRELATION",
                        self.phase_correlation,
                        "-1 / +1",
                        GREEN_BRIGHT,
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("MEAS PEAK")
                            .font(theme::bold(8.0))
                            .color(TEXT_MUTED),
                    );
                    ui.colored_label(
                        CYAN,
                        format!("{:.1} dBFS", self.measurement_level.peak_dbfs),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("REF PEAK")
                            .font(theme::bold(8.0))
                            .color(TEXT_MUTED),
                    );
                    ui.colored_label(
                        GREEN,
                        self.reference_level.map_or_else(
                            || "N/A".to_owned(),
                            |level| format!("{:.1} dBFS", level.peak_dbfs),
                        ),
                    );
                });
            },
        );
    }

    fn peak_meters(&mut self, ui: &mut egui::Ui) {
        widgets::module(
            ui,
            "PEAK_LEVEL_METERS",
            false,
            208.0,
            |_| {},
            |ui| {
                ui.horizontal_centered(|ui| {
                    widgets::level_meter(
                        ui,
                        self.reference_level.map_or(-120.0, |level| level.peak_dbfs),
                        self.reference_held_dbfs,
                        "REF",
                        GREEN,
                    );
                    ui.add_space(22.0);
                    widgets::level_meter(
                        ui,
                        self.measurement_level.peak_dbfs,
                        self.measurement_held_dbfs,
                        "MEAS",
                        CYAN,
                    );
                    ui.vertical(|ui| {
                        for db in [0, -6, -12, -18, -24, -36, -48, -60] {
                            ui.label(
                                RichText::new(db.to_string())
                                    .font(theme::mono(8.0))
                                    .color(TEXT_MUTED),
                            );
                        }
                    });
                });
            },
        );
    }

    fn phase_scope(&mut self, ui: &mut egui::Ui) {
        widgets::module(
            ui,
            "PHASE_CORRELATION",
            true,
            208.0,
            |_| {},
            |ui| {
                if self.has_reference {
                    widgets::vectorscope(ui, &self.scope_points, self.phase_correlation);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("N/A — REFERENCE DISABLED").color(TEXT_MUTED));
                    });
                }
            },
        );
    }

    fn analysis_module(&mut self, ui: &mut egui::Ui, height: f32) {
        let view = self.analysis_view;
        let hold_active = self.display_hold.active();
        let sample_rate = self.runtime.info().sample_rate;
        let mut toggle_hold = false;
        widgets::module(
            ui,
            "PRECISION_ANALYSIS",
            false,
            height,
            |ui| {
                if widgets::chip(ui, "HOLD", hold_active).clicked() {
                    toggle_hold = true;
                }
                let _ = widgets::chip(ui, "FAST", true);
                ui.label(
                    RichText::new(format!("SR: {sample_rate} Hz"))
                        .font(theme::bold(9.0))
                        .color(TEXT_MUTED),
                );
            },
            |ui| {
                ui.horizontal(|ui| {
                    for candidate in AnalysisView::ALL {
                        if widgets::chip(ui, candidate.label(), candidate == view).clicked() {
                            self.analysis_view = candidate;
                        }
                    }
                });
                ui.add_space(4.0);
                self.analysis_plot(ui, height - 82.0);
            },
        );
        if toggle_hold {
            self.display_hold.toggle();
        }
    }

    fn analysis_plot(&self, ui: &mut egui::Ui, height: f32) {
        if !self.has_reference && self.analysis_view != AnalysisView::Spectrum {
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(RichText::new("N/A — REFERENCE DISABLED").color(TEXT_MUTED));
                },
            );
            return;
        }

        let nyquist = self.runtime.info().sample_rate as f64 / 2.0;
        let (points, y_min, y_max, unit) = match self.analysis_view {
            AnalysisView::Spectrum => (&self.measurement_points, -120.0, 6.0, "dBFS"),
            AnalysisView::Transfer => (&self.transfer_points, -60.0, 20.0, "dB"),
            AnalysisView::Phase => (&self.phase_points, -180.0, 180.0, "deg"),
            AnalysisView::Coherence => (&self.coherence_points, 0.0, 1.0, ""),
        };
        let mut plot = Plot::new("precision_analysis_plot")
            .height(height.max(220.0))
            .allow_drag(true)
            .allow_zoom(true)
            .show_grid(true)
            .include_x(20.0_f64.log10())
            .include_x(nyquist.log10())
            .include_y(y_min)
            .include_y(y_max)
            .x_axis_formatter(|mark, _| format_frequency(10.0_f64.powf(mark.value)))
            .y_axis_formatter(move |mark, _| {
                if unit.is_empty() {
                    format!("{:.2}", mark.value)
                } else {
                    format!("{:.0} {unit}", mark.value)
                }
            });
        if self.analysis_view == AnalysisView::Spectrum {
            plot = plot.legend(Legend::default());
        }
        plot.show(ui, |plot_ui| {
            let measurement = Line::new(PlotPoints::from_iter(points.iter().copied()))
                .color(CYAN)
                .width(1.8)
                .name("Measurement");
            plot_ui.line(measurement);
            if self.analysis_view == AnalysisView::Spectrum && self.has_reference {
                plot_ui.line(
                    Line::new(PlotPoints::from_iter(self.reference_points.iter().copied()))
                        .color(GREEN.gamma_multiply(0.55))
                        .width(1.1)
                        .name("Reference"),
                );
            }
        });
    }

    fn timeline_module(&self, ui: &mut egui::Ui, height: f32) {
        widgets::module(
            ui,
            "TIMELINE_HISTORY",
            false,
            height,
            |ui| {
                ui.label(
                    RichText::new("BUFFER: 300s")
                        .font(theme::bold(9.0))
                        .color(GREEN),
                );
            },
            |ui| widgets::history_plot(ui, self.history.points(), height - 58.0),
        );
    }

    fn routing_window(&mut self, ctx: &egui::Context) {
        if !self.show_routing {
            return;
        }
        let mut open = self.show_routing;
        egui::Window::new("INPUT ROUTING")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let channels = self.runtime.info().channels;
                let previous = ChannelRoute {
                    reference: self.reference_channel,
                    measurement: self.measurement_channel,
                };
                egui::ComboBox::from_label("Reference")
                    .selected_text(self.reference_channel.map_or_else(
                        || "Disabled".to_owned(),
                        |channel| format!("Input {}", channel + 1),
                    ))
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
                egui::ComboBox::from_label("Measurement")
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
                let route = ChannelRoute {
                    reference: self.reference_channel,
                    measurement: self.measurement_channel,
                };
                if route != previous {
                    self.route_error = self.runtime.set_route(route).err();
                    self.history.clear();
                    self.scope_points.clear();
                }
                if let Some(error) = &self.route_error {
                    ui.colored_label(RED, error);
                }
            });
        self.show_routing = open;
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        egui::Window::new("DISPLAY SETTINGS")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.show_top_cards, "Show signal cards");
                ui.checkbox(&mut self.show_timeline, "Show timeline history");
                ui.separator();
                ui.label("FFT: 2048 points / 50% overlap");
                ui.label("Spectrum response: FAST");
            });
        self.show_settings = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = self.show_about;
        egui::Window::new("ABOUT")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("PRO AUDIO ANALYZER")
                        .font(theme::bold(18.0))
                        .color(GREEN_BRIGHT),
                );
                ui.label("Synchronous two-channel precision analysis");
                ui.label("JetBrains Mono licensed under SIL Open Font License 1.1");
            });
        self.show_about = open;
    }
}

impl eframe::App for AnalyzerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(snapshot) = self.runtime.take_latest() {
            let now = ctx.input(|input| input.time);
            let discontinuities = self.runtime.stats().discontinuities.load(Ordering::Relaxed);
            self.update_snapshot(snapshot, now, discontinuities);
        }

        self.top_bar(ctx);
        self.status_bar(ctx);
        self.side_bar(ctx);
        self.dashboard(ctx);
        self.routing_window(ctx);
        self.settings_window(ctx);
        self.about_window(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn format_frequency(frequency: f64) -> String {
    if frequency >= 1_000.0 {
        format!("{:.1}k", frequency / 1_000.0)
    } else {
        format!("{frequency:.0}")
    }
}
