mod state;
mod theme;
mod widgets;

use std::{sync::atomic::Ordering, time::Duration};

use eframe::egui::{
    self, Align, CentralPanel, Frame, Layout, RichText, ScrollArea, SidePanel, Stroke,
    TextureHandle, TextureOptions, TopBottomPanel, ViewportCommand, vec2,
};
use egui_plot::{GridMark, Legend, Line, Plot, PlotBounds, PlotPoints};

use crate::{
    controller::{AnalyzerController, ConnectionState},
    model::{AnalysisSnapshot, ChannelRoute, FFT_SIZE, SignalLevel, StereoFrame},
};

use self::{
    state::{
        AnalysisView, DEFAULT_NO_SIGNAL_THRESHOLD_DBFS, DisplayHold, HistoryBuffer, HistoryView,
        HistoryViewport, PeakHold, SignalGate,
    },
    theme::{
        BACKGROUND, CYAN, GREEN, GREEN_BRIGHT, OUTLINE, RED, SURFACE, SURFACE_LOW, TEXT,
        TEXT_MUTED, YELLOW,
    },
};

pub struct AnalyzerApp {
    runtime: AnalyzerController,
    display_connection: ConnectionState,
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
    history_view: HistoryView,
    history_viewport: HistoryViewport,
    spectrogram_texture: Option<TextureHandle>,
    display_hold: DisplayHold,
    analysis_view: AnalysisView,
    has_reference: bool,
    last_snapshot_time: Option<f64>,
    measurement_signal: SignalGate,
    reference_signal: SignalGate,
    no_signal_threshold_dbfs: f32,
    show_routing: bool,
    show_settings: bool,
    show_about: bool,
    show_top_cards: bool,
    show_timeline: bool,
    history_only: bool,
    reset_scroll: bool,
    fullscreen: bool,
}

impl AnalyzerApp {
    pub fn new(
        runtime: AnalyzerController,
        creation_context: &eframe::CreationContext<'_>,
    ) -> Self {
        theme::install(&creation_context.egui_ctx);
        Self::with_controller(runtime)
    }

    fn with_controller(runtime: AnalyzerController) -> Self {
        let route = runtime
            .info()
            .map_or(ChannelRoute::default_for_channels(1), |info| {
                info.initial_route
            });
        Self {
            runtime,
            display_connection: ConnectionState::Stopped,
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
            history_view: HistoryView::default(),
            history_viewport: HistoryViewport::default(),
            spectrogram_texture: None,
            display_hold: DisplayHold::default(),
            analysis_view: AnalysisView::Spectrum,
            has_reference: route.reference.is_some(),
            last_snapshot_time: None,
            measurement_signal: SignalGate::default(),
            reference_signal: SignalGate::default(),
            no_signal_threshold_dbfs: DEFAULT_NO_SIGNAL_THRESHOLD_DBFS,
            show_routing: false,
            show_settings: false,
            show_about: false,
            show_top_cards: true,
            show_timeline: true,
            history_only: false,
            reset_scroll: true,
            fullscreen: false,
        }
    }

    fn update_snapshot(
        &mut self,
        ctx: &egui::Context,
        snapshot: AnalysisSnapshot,
        now: f64,
        discontinuities: u64,
    ) {
        self.last_snapshot_time = Some(now);
        let measurement_was_active = self.measurement_signal.active();
        let measurement_active = self.measurement_signal.update(
            snapshot.measurement_level.rms_dbfs,
            now,
            self.no_signal_threshold_dbfs,
        );
        let reference_was_active = self.reference_signal.active();
        let reference_active = snapshot.reference_level.is_some_and(|level| {
            self.reference_signal
                .update(level.rms_dbfs, now, self.no_signal_threshold_dbfs)
        });
        if measurement_was_active && !measurement_active {
            self.measurement_peak_hold = PeakHold::default();
            self.measurement_held_dbfs = -120.0;
        }
        if reference_was_active && !reference_active {
            self.reference_peak_hold = PeakHold::default();
            self.reference_held_dbfs = -120.0;
        }
        if self.history.push(&snapshot, discontinuities) {
            let image = widgets::spectrogram_image(self.history.points(), 600);
            if let Some(texture) = &mut self.spectrogram_texture {
                texture.set(image, TextureOptions::NEAREST);
            } else {
                self.spectrogram_texture =
                    Some(ctx.load_texture("frequency_history", image, TextureOptions::NEAREST));
            }
        }
        self.measurement_level = snapshot.measurement_level;
        self.reference_level = snapshot.reference_level;
        self.phase_correlation = snapshot.phase_correlation;
        if measurement_active {
            self.measurement_held_dbfs = self
                .measurement_peak_hold
                .update(snapshot.measurement_level.peak_dbfs, now);
        }
        if reference_active {
            self.reference_held_dbfs = snapshot.reference_level.map_or(-120.0, |level| {
                self.reference_peak_hold.update(level.peak_dbfs, now)
            });
        }
        self.scope_points.clear();
        self.scope_points
            .extend_from_slice(&snapshot.scope_points[..snapshot.scope_len]);

        if self.display_hold.accepts_plot_update() {
            self.has_reference = snapshot.has_reference;
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
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        TopBottomPanel::top("instrument_top_bar")
            .exact_height(68.0)
            .frame(
                Frame::none()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(18.0);
                    Frame::none()
                        .fill(CYAN)
                        .rounding(8.0)
                        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("SA")
                                    .font(theme::bold(15.0))
                                    .color(BACKGROUND),
                            );
                        });
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("SPECTRA ANALYZER")
                                .font(theme::bold(16.0))
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new("REAL-TIME AUDIO WORKSPACE")
                                .font(theme::mono(8.0))
                                .color(TEXT_MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(18.0);
                        if ui
                            .button(
                                RichText::new("終了")
                                    .color(TEXT_MUTED)
                                    .font(theme::bold(10.0)),
                            )
                            .on_hover_text("アプリケーションを終了")
                            .clicked()
                        {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui
                            .button(
                                RichText::new(if self.fullscreen {
                                    "全画面を解除"
                                } else {
                                    "全画面"
                                })
                                .color(CYAN)
                                .font(theme::bold(10.0)),
                            )
                            .clicked()
                        {
                            self.fullscreen = !self.fullscreen;
                            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
                        }
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(if self.history_only {
                                    "履歴"
                                } else {
                                    "概要"
                                })
                                .font(theme::bold(14.0))
                                .color(TEXT),
                            );
                            ui.label(
                                RichText::new(if self.history_only {
                                    "過去300秒の信号を確認"
                                } else {
                                    "入力信号をリアルタイム解析"
                                })
                                .font(theme::mono(8.0))
                                .color(TEXT_MUTED),
                            );
                        });
                    });
                });
            });
    }

    fn side_bar(&mut self, ctx: &egui::Context) {
        SidePanel::left("instrument_side_bar")
            .exact_width(188.0)
            .resizable(false)
            .frame(
                Frame::none()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("WORKSPACE")
                            .font(theme::bold(9.0))
                            .color(TEXT_MUTED),
                    );
                    ui.add_space(10.0);
                    if widgets::nav_item(ui, "01", "概要", !self.history_only, true).clicked() {
                        self.history_only = false;
                        self.reset_scroll = true;
                    }
                    if widgets::nav_item(ui, "02", "履歴", self.history_only, true).clicked() {
                        self.history_only = true;
                        self.reset_scroll = true;
                    }
                    ui.add_space(18.0);
                    ui.separator();
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new("SETUP")
                            .font(theme::bold(9.0))
                            .color(TEXT_MUTED),
                    );
                    ui.add_space(10.0);
                    if widgets::nav_item(ui, "IN", "入力ルーティング", false, true).clicked()
                    {
                        self.show_routing = true;
                    }
                    if widgets::nav_item(ui, "UI", "表示設定", false, true).clicked() {
                        self.show_settings = true;
                    }
                    if widgets::nav_item(ui, "?", "このアプリについて", false, true).clicked()
                    {
                        self.show_about = true;
                    }
                });
            });
    }

    fn status_bar(&self, ctx: &egui::Context) {
        TopBottomPanel::bottom("instrument_status_bar")
            .exact_height(32.0)
            .frame(
                Frame::none()
                    .fill(BACKGROUND)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.0);
                    let stats = self.runtime.stats();
                    let stream_errors = stats.stream_errors.load(Ordering::Relaxed);
                    let dropped_audio = stats.dropped_audio_frames.load(Ordering::Relaxed);
                    let receiving = self
                        .last_snapshot_time
                        .is_some_and(|last| ctx.input(|input| input.time) - last < 1.0);
                    ui.colored_label(
                        if stream_errors == 0 && receiving {
                            GREEN
                        } else {
                            RED
                        },
                        if stream_errors != 0 {
                            "● STREAM ERROR"
                        } else if receiving {
                            "● LIVE"
                        } else {
                            "● IDLE"
                        },
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Audio drops  {dropped_audio}"))
                            .font(theme::mono(9.0))
                            .color(TEXT_MUTED),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Display drops  {}",
                            stats.dropped_results.load(Ordering::Relaxed)
                        ))
                        .font(theme::mono(9.0))
                        .color(TEXT_MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(14.0);
                        if let Some(info) = self.runtime.info() {
                            ui.colored_label(
                                CYAN,
                                format!("{} Hz / {} ch", info.sample_rate, info.channels),
                            );
                            ui.label(&info.device_name);
                        } else {
                            ui.label("入力未接続");
                        }
                    });
                });
            });
    }

    fn dashboard(&mut self, ctx: &egui::Context) {
        CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                let mut scroll = ScrollArea::vertical().auto_shrink([false, false]);
                if self.reset_scroll {
                    scroll = scroll.vertical_scroll_offset(0.0);
                    self.reset_scroll = false;
                }
                scroll.show(ui, |ui| {
                    ui.allocate_ui_with_layout(
                        vec2(ui.available_width(), 46.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(if self.history_only {
                                        "信号履歴"
                                    } else {
                                        "解析ダッシュボード"
                                    })
                                    .font(theme::bold(21.0))
                                    .color(TEXT),
                                );
                                ui.label(
                                    RichText::new(if self.history_only {
                                        "レベルと周波数分布を最大300秒さかのぼって確認できます。"
                                    } else {
                                        "測定入力と基準入力の状態をひとつの画面で監視します。"
                                    })
                                    .font(theme::mono(10.0))
                                    .color(TEXT_MUTED),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let receiving = self
                                    .last_snapshot_time
                                    .is_some_and(|last| ctx.input(|input| input.time) - last < 1.0);
                                let (label, color) = if receiving {
                                    ("● REAL-TIME", GREEN)
                                } else {
                                    ("● STANDBY", TEXT_MUTED)
                                };
                                ui.label(RichText::new(label).font(theme::bold(10.0)).color(color));
                            });
                        },
                    );
                    ui.add_space(18.0);

                    if self.history_only {
                        self.history_module(ui, ui.available_height().max(520.0), true);
                        return;
                    }

                    if self.show_top_cards {
                        self.top_cards(ui);
                        ui.add_space(14.0);
                    }

                    let content_height = 472.0;
                    if ui.available_width() >= 980.0 {
                        let analysis_width = (ui.available_width() - 318.0).max(560.0);
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                vec2(analysis_width, content_height),
                                Layout::top_down(Align::Min),
                                |ui| self.analysis_module(ui, content_height),
                            );
                            ui.vertical(|ui| {
                                ui.set_width(304.0);
                                ui.allocate_ui_with_layout(
                                    vec2(304.0, 226.0),
                                    Layout::top_down(Align::Min),
                                    |ui| self.peak_meters(ui),
                                );
                                ui.add_space(12.0);
                                ui.allocate_ui_with_layout(
                                    vec2(304.0, 234.0),
                                    Layout::top_down(Align::Min),
                                    |ui| self.phase_scope(ui),
                                );
                            });
                        });
                    } else {
                        self.analysis_module(ui, 420.0);
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let card_width = (ui.available_width() - 12.0) / 2.0;
                            ui.allocate_ui_with_layout(
                                vec2(card_width, 226.0),
                                Layout::top_down(Align::Min),
                                |ui| self.peak_meters(ui),
                            );
                            ui.allocate_ui_with_layout(
                                vec2(card_width, 226.0),
                                Layout::top_down(Align::Min),
                                |ui| self.phase_scope(ui),
                            );
                        });
                    }
                    if self.show_timeline {
                        ui.add_space(14.0);
                        self.history_module(ui, 190.0, false);
                    }
                });
            });
    }

    fn top_cards(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let columns = if width >= 920.0 {
            4
        } else if width >= 520.0 {
            2
        } else {
            1
        };
        let card_width = (width - 12.0 * (columns - 1) as f32) / columns as f32;
        let measurement_active = self.measurement_signal.active();
        let reference_active = self.has_reference && self.reference_signal.active();
        let sample_rate = self
            .runtime
            .info()
            .map(|info| info.sample_rate as f32 / 1_000.0);
        let cards = [
            (
                "MEASUREMENT RMS",
                measurement_active.then_some(self.measurement_level.rms_dbfs),
                "dBFS",
                if measurement_active {
                    format!("Peak  {:.1} dBFS", self.measurement_level.peak_dbfs)
                } else {
                    "NO SIGNAL".to_owned()
                },
                CYAN,
            ),
            (
                "REFERENCE RMS",
                self.reference_level
                    .filter(|_| reference_active)
                    .map(|level| level.rms_dbfs),
                "dBFS",
                if !self.has_reference {
                    "Reference disabled".to_owned()
                } else if !reference_active {
                    "NO SIGNAL".to_owned()
                } else {
                    format!(
                        "Peak  {:.1} dBFS",
                        self.reference_level.map_or(-120.0, |level| level.peak_dbfs)
                    )
                },
                GREEN,
            ),
            (
                "PHASE CORRELATION",
                (measurement_active && reference_active)
                    .then_some(self.phase_correlation)
                    .flatten(),
                "",
                if measurement_active && reference_active {
                    "Range  -1.0 / +1.0".to_owned()
                } else {
                    "NO SIGNAL".to_owned()
                },
                GREEN_BRIGHT,
            ),
            (
                "INPUT SAMPLE RATE",
                sample_rate,
                "kHz",
                self.runtime.info().map_or_else(
                    || "Input disconnected".to_owned(),
                    |info| format!("{} ch / FFT {FFT_SIZE}", info.channels),
                ),
                YELLOW,
            ),
        ];
        for row in cards.chunks(columns) {
            ui.horizontal(|ui| {
                for (label, value, unit, detail, color) in row {
                    ui.allocate_ui_with_layout(
                        vec2(card_width, 112.0),
                        Layout::top_down(Align::Min),
                        |ui| widgets::summary_metric(ui, label, *value, unit, detail, *color),
                    );
                }
            });
            if columns < 4 {
                ui.add_space(10.0);
            }
        }
    }

    fn peak_meters(&mut self, ui: &mut egui::Ui) {
        let measurement_active = self.measurement_signal.active();
        let reference_active = self.has_reference && self.reference_signal.active();
        widgets::module(
            ui,
            "PEAK_LEVEL_METERS / ピークメーター",
            false,
            208.0,
            |_| {},
            |ui| {
                ui.horizontal_centered(|ui| {
                    widgets::level_meter(
                        ui,
                        self.reference_level
                            .filter(|_| reference_active)
                            .map_or(-120.0, |level| level.peak_dbfs),
                        if reference_active {
                            self.reference_held_dbfs
                        } else {
                            -120.0
                        },
                        "基準",
                        GREEN,
                    );
                    ui.add_space(22.0);
                    widgets::level_meter(
                        ui,
                        if measurement_active {
                            self.measurement_level.peak_dbfs
                        } else {
                            -120.0
                        },
                        if measurement_active {
                            self.measurement_held_dbfs
                        } else {
                            -120.0
                        },
                        "測定",
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
            "PHASE_CORRELATION / 位相相関",
            true,
            208.0,
            |_| {},
            |ui| {
                if self.has_reference
                    && self.measurement_signal.active()
                    && self.reference_signal.active()
                {
                    widgets::vectorscope(ui, &self.scope_points, self.phase_correlation);
                } else {
                    ui.centered_and_justified(|ui| {
                        let message = if self.has_reference {
                            "N/A — 信号を検出していません"
                        } else {
                            "N/A — 基準入力が無効です"
                        };
                        ui.label(RichText::new(message).color(TEXT_MUTED));
                    });
                }
            },
        );
    }

    fn analysis_module(&mut self, ui: &mut egui::Ui, height: f32) {
        let view = self.analysis_view;
        let hold_active = self.display_hold.active();
        let sample_rate = self.runtime.info().map_or(0, |info| info.sample_rate);
        let input_range = if sample_rate == 0 {
            "入力未接続".to_owned()
        } else {
            format!(
                "範囲: 20 Hz–{}  |  SR: {sample_rate} Hz",
                format_frequency(sample_rate as f64 * 0.5)
            )
        };
        let mut toggle_hold = false;
        widgets::module(
            ui,
            "PRECISION_ANALYSIS / 周波数解析",
            false,
            height,
            |ui| {
                if widgets::chip(ui, "保持 / HOLD", hold_active).clicked() {
                    toggle_hold = true;
                }
                let _ = widgets::chip(ui, "高速 / FAST", true);
                ui.label(
                    RichText::new(input_range)
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
        let pair_active = self.has_reference
            && self.measurement_signal.active()
            && self.reference_signal.active();
        if !pair_active && self.analysis_view != AnalysisView::Spectrum {
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    let message = if self.has_reference {
                        "N/A — 信号を検出していません"
                    } else {
                        "N/A — 基準入力が無効です"
                    };
                    ui.label(RichText::new(message).color(TEXT_MUTED));
                },
            );
            return;
        }

        let nyquist = self
            .runtime
            .info()
            .map_or(24_000.0, |info| info.sample_rate as f64 / 2.0);
        let (points, y_min, y_max, unit) = match self.analysis_view {
            AnalysisView::Spectrum => (&self.measurement_points, -120.0, 6.0, "dBFS"),
            AnalysisView::Transfer => (&self.transfer_points, -60.0, 20.0, "dB"),
            AnalysisView::Phase => (&self.phase_points, -180.0, 180.0, "deg"),
            AnalysisView::Coherence => (&self.coherence_points, 0.0, 1.0, ""),
        };
        let mut plot = Plot::new("precision_analysis_plot")
            .height(height.max(220.0))
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .show_grid(true)
            .include_x(20.0_f64.log10())
            .include_x(nyquist.log10())
            .include_y(y_min)
            .include_y(y_max)
            .x_grid_spacer(move |_| frequency_grid_marks(nyquist))
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
            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                [20.0_f64.log10(), y_min],
                [nyquist.log10(), y_max],
            ));
            let measurement = Line::new(PlotPoints::from_iter(points.iter().copied()))
                .color(CYAN)
                .width(1.8)
                .name("測定");
            plot_ui.line(measurement);
            if self.analysis_view == AnalysisView::Spectrum && self.has_reference {
                plot_ui.line(
                    Line::new(PlotPoints::from_iter(self.reference_points.iter().copied()))
                        .color(GREEN.gamma_multiply(0.55))
                        .width(1.1)
                        .name("基準"),
                );
            }
        });
    }

    fn history_module(&mut self, ui: &mut egui::Ui, height: f32, accent: bool) {
        let current_view = self.history_view;
        let mut selected_view = current_view;
        let viewport = self.history_viewport;
        let mut zoom_in = false;
        let mut zoom_out = false;
        let mut pan_older = false;
        let mut pan_newer = false;
        let mut jump_live = false;
        let points = self.history.points();
        let texture = self.spectrogram_texture.as_ref();
        let sample_rate = self.runtime.info().map_or(0, |info| info.sample_rate);
        widgets::module(
            ui,
            "TIMELINE_HISTORY / 300秒履歴",
            accent,
            height,
            |ui| {
                if current_view == HistoryView::Spectrum {
                    jump_live =
                        widgets::chip(ui, "現在へ", viewport.offset_from_live() == 0.0).clicked();
                    pan_newer = widgets::chip(ui, "新しい →", false).clicked();
                    pan_older = widgets::chip(ui, "← 過去", false).clicked();
                    zoom_out = widgets::chip(ui, "縮小 −", false).clicked();
                    zoom_in = widgets::chip(ui, "拡大 +", false).clicked();
                    ui.label(
                        RichText::new(format!("表示: {:.0}秒", viewport.visible_seconds()))
                            .font(theme::bold(9.0))
                            .color(CYAN),
                    );
                }
                for view in HistoryView::ALL.into_iter().rev() {
                    if widgets::chip(ui, view.label(), view == current_view).clicked() {
                        selected_view = view;
                    }
                }
                ui.label(
                    RichText::new("記録: 300秒")
                        .font(theme::bold(9.0))
                        .color(GREEN),
                );
            },
            |ui| match current_view {
                HistoryView::Level => widgets::history_plot(ui, points, height - 58.0),
                HistoryView::Spectrum => {
                    widgets::spectrogram(ui, texture, sample_rate, viewport, height - 58.0)
                }
            },
        );
        self.history_view = selected_view;
        if zoom_in {
            self.history_viewport.zoom_in();
        }
        if zoom_out {
            self.history_viewport.zoom_out();
        }
        if pan_older {
            self.history_viewport.pan_older();
        }
        if pan_newer {
            self.history_viewport.pan_newer();
        }
        if jump_live {
            self.history_viewport.jump_live();
        }
    }

    fn routing_window(&mut self, ctx: &egui::Context) {
        if !self.show_routing {
            return;
        }
        let mut open = self.show_routing;
        egui::Window::new("入力ルーティング / INPUT ROUTING")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let Some(info) = self.runtime.info() else {
                    ui.label("入力を接続するとチャンネルを設定できます。");
                    return;
                };
                let channels = info.channels;
                let previous = ChannelRoute {
                    reference: self.reference_channel,
                    measurement: self.measurement_channel,
                };
                egui::ComboBox::from_label("基準入力 / Reference")
                    .selected_text(self.reference_channel.map_or_else(
                        || "無効".to_owned(),
                        |channel| format!("入力 {}", channel + 1),
                    ))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.reference_channel, None, "無効");
                        for channel in 0..channels {
                            ui.selectable_value(
                                &mut self.reference_channel,
                                Some(channel),
                                format!("入力 {}", channel + 1),
                            );
                        }
                    });
                egui::ComboBox::from_label("測定入力 / Measurement")
                    .selected_text(format!("入力 {}", self.measurement_channel + 1))
                    .show_ui(ui, |ui| {
                        for channel in 0..channels {
                            ui.selectable_value(
                                &mut self.measurement_channel,
                                channel,
                                format!("入力 {}", channel + 1),
                            );
                        }
                    });
                let route = ChannelRoute {
                    reference: self.reference_channel,
                    measurement: self.measurement_channel,
                };
                if route != previous {
                    self.route_error = self.runtime.set_route(route).err();
                    if self.route_error.is_none() {
                        self.reset_display(route);
                    } else {
                        self.reference_channel = previous.reference;
                        self.measurement_channel = previous.measurement;
                    }
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
        egui::Window::new("表示設定 / DISPLAY SETTINGS")
            .open(&mut open)
            .resizable(false)
            .default_width(390.0)
            .show(ctx, |ui| {
                ui.checkbox(&mut self.show_top_cards, "信号カードを表示");
                ui.checkbox(&mut self.show_timeline, "履歴パネルを表示");
                ui.separator();
                ui.label("FFT: 2048点 / 50%オーバーラップ");
                ui.label("スペクトラム応答: 高速 / FAST");
                ui.add_space(8.0);
                egui::CollapsingHeader::new("高度な設定")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label("無信号閾値");
                        let previous = self.no_signal_threshold_dbfs;
                        ui.add(
                            egui::Slider::new(
                                &mut self.no_signal_threshold_dbfs,
                                -120.0..=-40.0,
                            )
                            .suffix(" dBFS")
                            .step_by(1.0),
                        );
                        ui.label(
                            RichText::new(
                                "この値を下回る状態が続くと、数値・ピーク・相関表示を N/A に固定します。復帰には6 dBのヒステリシスがあります。",
                            )
                            .small()
                            .color(TEXT_MUTED),
                        );
                        if ui.button("既定値（-90 dBFS）に戻す").clicked() {
                            self.no_signal_threshold_dbfs =
                                DEFAULT_NO_SIGNAL_THRESHOLD_DBFS;
                        }
                        if self.no_signal_threshold_dbfs != previous {
                            self.measurement_signal = SignalGate::default();
                            self.reference_signal = SignalGate::default();
                            self.measurement_peak_hold = PeakHold::default();
                            self.reference_peak_hold = PeakHold::default();
                            self.measurement_held_dbfs = -120.0;
                            self.reference_held_dbfs = -120.0;
                        }
                    });
            });
        self.show_settings = open;
    }

    fn reset_display(&mut self, route: ChannelRoute) {
        self.reference_channel = route.reference;
        self.measurement_channel = route.measurement;
        self.route_error = None;
        self.history.clear();
        self.history_viewport = HistoryViewport::default();
        self.spectrogram_texture = None;
        self.scope_points.clear();
        self.reference_points.clear();
        self.measurement_points.clear();
        self.transfer_points.clear();
        self.phase_points.clear();
        self.coherence_points.clear();
        self.measurement_level = SignalLevel::default();
        self.reference_level = None;
        self.phase_correlation = None;
        self.measurement_peak_hold = PeakHold::default();
        self.reference_peak_hold = PeakHold::default();
        self.measurement_held_dbfs = -120.0;
        self.reference_held_dbfs = -120.0;
        self.last_snapshot_time = None;
        self.measurement_signal = SignalGate::default();
        self.reference_signal = SignalGate::default();
        self.has_reference = route.reference.is_some();
        self.display_hold = DisplayHold::default();
    }

    fn poll_connection(&mut self) {
        self.runtime.poll();
        if self.runtime.state() != &self.display_connection {
            self.display_connection = self.runtime.state().clone();
            let route = self
                .runtime
                .info()
                .map_or(ChannelRoute::default_for_channels(1), |info| {
                    info.initial_route
                });
            self.reset_display(route);
        }
    }

    fn connection_bar(&mut self, ctx: &egui::Context) {
        let state = self.runtime.state().clone();
        let can_start = matches!(
            &state,
            ConnectionState::Stopped | ConnectionState::Failed(_)
        );
        let can_stop = matches!(&state, ConnectionState::Starting | ConnectionState::Running);
        TopBottomPanel::top("input_connection")
            .exact_height(52.0)
            .frame(
                Frame::none()
                    .fill(SURFACE_LOW)
                    .stroke(Stroke::new(1.0, OUTLINE)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.0);
                    let (label, color) = match &state {
                        ConnectionState::Stopped => ("入力は停止しています".to_owned(), TEXT_MUTED),
                        ConnectionState::Starting => ("入力デバイスに接続中…".to_owned(), YELLOW),
                        ConnectionState::Running => ("入力接続済み".to_owned(), GREEN),
                        ConnectionState::Stopping => ("入力を停止中…".to_owned(), YELLOW),
                        ConnectionState::Failed(error) => (format!("接続失敗 — {error}"), RED),
                    };
                    if matches!(
                        &state,
                        ConnectionState::Starting | ConnectionState::Stopping
                    ) {
                        ui.spinner();
                    } else {
                        ui.colored_label(color, "●");
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(label).font(theme::bold(10.0)).color(color));
                        if let Some(info) = self.runtime.info() {
                            ui.label(
                                RichText::new(format!(
                                    "{}  ·  {} Hz  ·  {} channels",
                                    info.device_name, info.sample_rate, info.channels
                                ))
                                .font(theme::mono(8.0))
                                .color(TEXT_MUTED),
                            );
                        } else {
                            ui.label(
                                RichText::new("オーディオ入力を接続すると解析を開始します")
                                    .font(theme::mono(8.0))
                                    .color(TEXT_MUTED),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(14.0);
                        if can_start
                            && ui
                                .add(
                                    egui::Button::new("接続 / 再試行")
                                        .fill(CYAN)
                                        .stroke(Stroke::NONE),
                                )
                                .clicked()
                            && let Err(error) = self.runtime.start()
                        {
                            self.route_error = Some(error);
                        }
                        if can_stop && ui.button("入力を停止").clicked() {
                            self.runtime.stop();
                            self.reset_display(ChannelRoute::default_for_channels(1));
                        }
                        if matches!(&state, ConnectionState::Running)
                            && ui.button("ルーティング").clicked()
                        {
                            self.show_routing = true;
                        }
                    });
                });
            });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = self.show_about;
        egui::Window::new("このアプリについて / ABOUT")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("PRO AUDIO ANALYZER")
                        .font(theme::bold(18.0))
                        .color(GREEN_BRIGHT),
                );
                ui.label("同期2チャンネル精密解析");
                ui.label("JetBrains Mono: SIL Open Font License 1.1");
            });
        self.show_about = open;
    }
}

impl eframe::App for AnalyzerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }
}

impl AnalyzerApp {
    fn tick(&mut self, ctx: &egui::Context) {
        self.poll_connection();
        if let Some(snapshot) = self.runtime.take_latest() {
            let now = ctx.input(|input| input.time);
            let discontinuities = self.runtime.stats().discontinuities.load(Ordering::Relaxed);
            self.update_snapshot(ctx, snapshot, now, discontinuities);
        }

        self.top_bar(ctx);
        self.connection_bar(ctx);
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
        let khz = frequency / 1_000.0;
        if (khz - khz.round()).abs() < 0.01 {
            format!("{khz:.0} kHz")
        } else {
            format!("{khz:.1} kHz")
        }
    } else {
        format!("{frequency:.0} Hz")
    }
}

fn frequency_grid_marks(nyquist: f64) -> Vec<GridMark> {
    let mut frequencies: Vec<f64> = [
        20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0, 50_000.0,
        100_000.0, 200_000.0,
    ]
    .into_iter()
    .filter(|frequency| *frequency <= nyquist)
    .collect();
    if frequencies
        .last()
        .is_some_and(|last| nyquist / last < 1.35 && (last - nyquist).abs() > nyquist * 0.001)
    {
        frequencies.pop();
    }
    if frequencies
        .last()
        .is_none_or(|last| (last - nyquist).abs() > nyquist * 0.001)
    {
        frequencies.push(nyquist);
    }
    frequencies
        .into_iter()
        .map(|frequency| GridMark {
            value: frequency.log10(),
            step_size: if frequency == 20.0 || frequency.log10().fract().abs() < 0.001 {
                1.0
            } else {
                0.3
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::RuntimeStats,
        source::{InputSource, OpenedSource},
    };
    use std::{
        sync::{Arc, atomic::AtomicBool},
        thread,
        time::Instant,
    };

    struct UnavailableInput;
    impl InputSource for UnavailableInput {
        fn open(&mut self, _: Arc<RuntimeStats>, _: &AtomicBool) -> Result<OpenedSource, String> {
            Err("test input unavailable".to_owned())
        }
    }

    #[test]
    fn full_app_renders_disconnected_and_failed_at_supported_sizes() {
        for size in [vec2(760.0, 640.0), vec2(1440.0, 900.0)] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut app = AnalyzerApp::with_controller(
                AnalyzerController::with_source(UnavailableInput).unwrap(),
            );
            let draw = |app: &mut AnalyzerApp| {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ctx| app.tick(ctx),
                );
                assert!(!output.shapes.is_empty());
            };
            draw(&mut app);
            app.runtime.start().unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.poll_connection();
                if matches!(app.runtime.state(), ConnectionState::Failed(_)) {
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            app.show_routing = true;
            app.show_settings = true;
            app.history_view = HistoryView::Spectrum;
            app.history_viewport.zoom_in();
            draw(&mut app);
            assert!(!app.measurement_signal.active());
            assert_eq!(app.measurement_level.peak_dbfs, -120.0);
            assert!(app.history.points().is_empty());
        }
    }
}
