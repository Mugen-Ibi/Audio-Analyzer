mod endurance;
mod theme;
mod waterfall;
mod widgets;
use crate::{
    audio::{self, InputDeviceDescription},
    config::{AnalysisConfig, Settings, WindowFunction},
    controller::{AnalyzerController, ConnectionState},
    history::{HISTORY_SPECTRUM_BANDS, HistoryBuffer, HistoryViewport},
    model::{AnalysisSnapshot, ChannelRoute, SignalLevel},
};
use eframe::egui::{self, RichText};
use egui_plot::{Line, Plot, PlotPoints, Points};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};
use theme::{CYAN, GREEN, RED, TEXT_MUTED, YELLOW};
type Discovery = (Vec<InputDeviceDescription>, Vec<String>);
#[derive(Default)]
pub struct AppOptions {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub initial_notice: String,
    pub demo: bool,
    /// Internal native framebuffer QA, only with synthetic/no-input mode.
    pub screenshot: Option<PathBuf>,
    pub open_settings: bool,
    pub qa_hold: bool,
    pub qa_history: bool,
    pub qa_settings_page: usize,
    pub qa_zoom: f32,
    pub qa_delay_seconds: f64,
    pub qa_2d: bool,
    pub qa_orbit: bool,
    pub qa_reference: bool,
    pub qa_cursor: bool,
    pub qa_duration: f64,
    pub qa_minimum_dbfs: Option<f32>,
    pub qa_endurance: Option<PathBuf>,
}
pub struct AnalyzerApp {
    runtime: AnalyzerController,
    settings: Settings,
    settings_path: Option<PathBuf>,
    notice: String,
    show_settings: bool,
    settings_page: usize,
    demo: bool,
    devices: Vec<InputDeviceDescription>,
    discovery_errors: Vec<String>,
    discovery: Option<mpsc::Receiver<Discovery>>,
    connection: ConnectionState,
    live: Option<Box<AnalysisSnapshot>>,
    held: Option<Box<AnalysisSnapshot>>,
    held_end: Option<f64>,
    anchor: Option<f64>,
    live_time: f64,
    held_sample_time: Option<f64>,
    last_valid_time: Option<f64>,
    history: HistoryBuffer,
    viewport: HistoryViewport,
    cursor: Option<f64>,
    cursor_band: usize,
    waterfall: waterfall::Waterfall,
    baseline: Option<crate::visualizer::SpectrumReference>,
    show_spectrum: bool,
    show_reference: bool,
    show_measurement: bool,
    advanced: usize,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    qa_capture_size: Option<[u32; 2]>,
    qa_stable_capture_frames: u8,
    qa_hold: bool,
    qa_history: bool,
    qa_reference: bool,
    qa_cursor: bool,
    qa_delay_seconds: f64,
    launched: Instant,
    last_tick: Option<Instant>,
    tick_intervals: Vec<f64>,
    endurance: Option<endurance::Trace>,
}
impl AnalyzerApp {
    pub fn new(
        runtime: AnalyzerController,
        cc: &eframe::CreationContext<'_>,
        options: AppOptions,
    ) -> Self {
        theme::install(&cc.egui_ctx);
        if options.screenshot.is_some() && options.qa_zoom.is_finite() && options.qa_zoom > 0.0 {
            cc.egui_ctx.set_zoom_factor(options.qa_zoom.clamp(0.8, 2.0));
        }
        Self::with_options(runtime, options)
    }
    fn with_options(runtime: AnalyzerController, options: AppOptions) -> Self {
        let mut history = HistoryBuffer::default();
        history.set_retention(options.settings.history_seconds);
        let mut app = Self {
            runtime,
            settings: options.settings,
            settings_path: options.settings_path,
            notice: options.initial_notice,
            show_settings: options.open_settings,
            settings_page: options.qa_settings_page.min(3),
            demo: options.demo,
            devices: Vec::new(),
            discovery_errors: Vec::new(),
            discovery: None,
            connection: ConnectionState::Stopped,
            live: None,
            held: None,
            held_end: None,
            anchor: None,
            live_time: 0.0,
            held_sample_time: None,
            last_valid_time: None,
            history,
            viewport: HistoryViewport::default(),
            cursor: None,
            cursor_band: 48,
            waterfall: Default::default(),
            baseline: None,
            show_spectrum: false,
            show_reference: true,
            show_measurement: true,
            advanced: 0,
            screenshot: options.screenshot,
            screenshot_requested: false,
            qa_capture_size: None,
            qa_stable_capture_frames: 0,
            qa_hold: options.qa_hold,
            qa_history: options.qa_history,
            qa_reference: options.qa_reference,
            qa_cursor: options.qa_cursor,
            qa_delay_seconds: if options.qa_delay_seconds.is_finite()
                && options.qa_delay_seconds > 0.0
            {
                options.qa_delay_seconds.clamp(1.0, 90.0)
            } else {
                5.0
            },
            launched: Instant::now(),
            last_tick: None,
            tick_intervals: Vec::with_capacity(4096),
            endurance: options
                .qa_endurance
                .as_ref()
                .filter(|_| options.demo)
                .map(|path| {
                    endurance::Trace::open(path).expect("cannot create fresh endurance trace files")
                }),
        };
        if app.screenshot.is_some() {
            if let Some(minimum) = options
                .qa_minimum_dbfs
                .filter(|v| v.is_finite() && (-160.0..=-20.0).contains(v))
            {
                app.settings.minimum_dbfs = minimum;
            }
            if options.qa_2d {
                app.settings.visualizer_mode = crate::visualizer::ViewMode::Heatmap;
            }
            if options.qa_orbit {
                app.settings.camera.orbit(-100.0, 40.0);
                app.settings.camera.zoom = 1.15;
            }
            while app.viewport.visible_seconds() > options.qa_duration.max(10.0) {
                app.viewport.zoom_in();
            }
        }
        if !app.demo && app.screenshot.is_none() {
            app.discover();
        }
        app
    }
    fn discover(&mut self) {
        if self.discovery.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name("input-discovery".into())
            .spawn(move || {
                let _ = tx.send(audio::enumerate_inputs());
            }) {
            Ok(_) => self.discovery = Some(rx),
            Err(e) => self.notice = format!("入力一覧を取得できません: {e}"),
        }
    }
    fn segment(&mut self, reason: &str) {
        if self.held_end.is_some() {
            self.notice = "測定条件が変わったため表示停止を解除しました。旧結果は履歴の条件境界から確認できます。".into();
        }
        let route = self
            .settings
            .input
            .route
            .unwrap_or(ChannelRoute::default_for_channels(1));
        let device = self
            .runtime
            .info()
            .map_or("未接続", |i| i.device_name.as_str());
        self.history.begin_segment(format!(
            "{reason} / {device} / 青 ch{}・緑 {} / {:?} / {}",
            route.measurement + 1,
            route
                .reference
                .map_or("なし".into(), |c| format!("ch{}", c + 1)),
            self.settings.analysis,
            self.settings.condition_notes
        ));
        self.live = None;
        self.last_valid_time = None;
        self.held = None;
        self.held_end = None;
        self.cursor = None;
        self.waterfall.invalidate();
    }
    fn poll(&mut self, now: f64) {
        if let Some(rx) = &self.discovery {
            match rx.try_recv() {
                Ok((d, e)) => {
                    self.devices = d;
                    self.discovery_errors = e;
                    self.discovery = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.notice =
                        "入力一覧の取得スレッドが終了しました。再取得してください。".into();
                    self.discovery = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.runtime.poll();
        if self.connection != *self.runtime.state() {
            self.connection = self.runtime.state().clone();
            if self.connection == ConnectionState::Running {
                self.settings.input.route = self.runtime.info().map(|i| i.initial_route);
                self.anchor = None;
                self.segment("入力開始 / 新しい条件");
            } else if matches!(
                self.connection,
                ConnectionState::Stopped | ConnectionState::Stopping | ConnectionState::Failed(_)
            ) && self.live.is_some()
                && self.held_end.is_none()
            {
                self.freeze(now);
            }
        }
        if let Some(snapshot) = self.runtime.take_latest() {
            let rate = snapshot.sample_rate as f64;
            let end_frame = snapshot.window_start_frame + snapshot.config.fft_size as u64;
            let age = snapshot.processed_at.elapsed().as_secs_f64()
                + snapshot.queue_lag_frames as f64 / rate;
            let anchor = *self
                .anchor
                .get_or_insert(now - end_frame as f64 / rate - age);
            self.live_time = anchor
                + (snapshot.window_start_frame as f64 + snapshot.config.fft_size as f64 * 0.5)
                    / rate;
            if snapshot.invalid_samples == 0 {
                self.last_valid_time = Some(self.live_time);
            }
            self.history
                .push_at(&snapshot, snapshot.discontinuity_id, self.live_time);
            self.live = Some(Box::new(snapshot));
        }
        self.history.expire(now);
    }
    fn freeze(&mut self, now: f64) {
        self.held = self.live.as_ref().map(|s| Box::new((**s).clone()));
        self.held_end = Some(now);
        self.held_sample_time = self.live.as_ref().map(|_| self.live_time);
        self.waterfall.invalidate();
    }
    fn displayed(&self) -> Option<&AnalysisSnapshot> {
        if self.held_end.is_some() {
            self.held.as_deref()
        } else {
            self.live.as_deref()
        }
    }
    fn freshness(&self) -> Option<f64> {
        self.live.as_ref().map(|s| {
            s.processed_at.elapsed().as_secs_f64()
                + s.queue_lag_frames as f64 / s.sample_rate as f64
        })
    }
    fn transport(&mut self, now: f64) {
        match self.connection {
            ConnectionState::Running | ConnectionState::Starting => {
                self.runtime.stop();
                self.freeze(now);
            }
            ConnectionState::Stopped | ConnectionState::Failed(_) => {
                self.notice = self
                    .runtime
                    .start_configured(self.settings.input.clone(), self.settings.analysis)
                    .err()
                    .unwrap_or_default();
            }
            ConnectionState::Stopping => {}
        }
    }
    fn return_live(&mut self) {
        self.held_end = None;
        self.held = None;
        self.viewport.jump_live();
        self.waterfall.invalidate();
    }
    fn toggle_hold(&mut self, now: f64) {
        if self.held_end.is_some() {
            self.return_live();
        } else {
            self.freeze(now);
            self.waterfall.invalidate();
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context, now: f64) {
        // Do not steal letters from notes or repeat a held key into transport actions.
        let pressed = |key| {
            ctx.input(|i| {
                i.events.iter().any(|e|
            matches!(e, egui::Event::Key { key: k, pressed: true, repeat: false, modifiers, .. }
                if *k == key && !modifiers.any()))
            })
        };
        if pressed(egui::Key::Escape) {
            self.show_settings = false;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        if pressed(egui::Key::F1) {
            self.show_settings = !self.show_settings;
        }
        if self.show_settings {
            return;
        }
        if pressed(egui::Key::F5) {
            self.transport(now);
        }
        if pressed(egui::Key::H) && self.displayed().is_some() {
            self.toggle_hold(now);
        }
        if pressed(egui::Key::L) {
            self.return_live();
        }
    }
    fn header(&mut self, ctx: &egui::Context, now: f64) {
        egui::TopBottomPanel::top("controls")
            .frame(egui::Frame::none().fill(theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(16.0, 10.0)))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("SPECTRA · VISUALIZER").font(theme::bold(18.0)).color(theme::GREEN_BRIGHT));
                    let (status, color) = match &self.connection {
                        ConnectionState::Running if self.freshness().is_some_and(|a| a > 0.25) => ("STALE · 滞留", RED),
                        ConnectionState::Running if self.live.is_none() => ("入力待ち", YELLOW),
                        ConnectionState::Running if self.held_end.is_some() => ("HOLD · 表示固定", YELLOW),
                        ConnectionState::Running => ("LIVE · 受信中", GREEN),
                        ConnectionState::Starting => ("接続中", YELLOW),
                        ConnectionState::Stopping => ("停止処理中", YELLOW),
                        ConnectionState::Failed(_) => ("接続失敗", RED),
                        ConnectionState::Stopped => ("STOPPED", TEXT_MUTED),
                    };
                    ui.label(RichText::new(status).strong().color(color))
                        .on_hover_text("LIVEは受信状態。HOLDは表示だけを固定し、入力と履歴の更新は続きます。STALEは結果末尾が250ms以上古い状態です。");
                    ui.separator();
                    let action = match self.connection {
                        ConnectionState::Running => "入力停止",
                        ConnectionState::Starting => "接続を取消",
                        ConnectionState::Stopping => "停止処理中…",
                        ConnectionState::Failed(_) => "再接続",
                        ConnectionState::Stopped => "測定を開始",
                    };
                    if ui.add_enabled(self.connection != ConnectionState::Stopping,
                        egui::Button::new(action).fill(if matches!(self.connection, ConnectionState::Stopped | ConnectionState::Failed(_)) { theme::SURFACE_HIGH } else { theme::SURFACE_LOW }))
                        .on_hover_text("F5 · 選択した入力を開始／停止します。停止中はマイクを使用しません。")
                        .clicked() { self.transport(now); }
                    if ui.add_enabled(self.displayed().is_some(), egui::Button::new(
                        if self.held_end.is_some() { "LIVEへ戻る" } else { "HOLD" }))
                        .on_hover_text("H · 表示を固定／解除。入力停止とは別の操作です。")
                        .clicked() { self.toggle_hold(now); }
                    if ui.button("設定").on_hover_text("F1 · 入力、解析、比較条件、診断").clicked() { self.show_settings = true; }
                    if self.demo { ui.colored_label(YELLOW, "合成入力 · 機器不使用"); }
                });
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    if let Some(info) = self.runtime.info() {
                        let source = if self.demo { "テスト信号 · 750 / 1500 Hz".to_owned() }
                            else { format!("{} · {}", info.host_name, info.device_name) };
                        ui.add(egui::Label::new(RichText::new(source).color(TEXT_MUTED)).truncate())
                            .on_hover_text(format!("{} / {}", info.host_name, info.device_name));
                        ui.monospace(format!("{} Hz · {} ch · {} frames", info.sample_rate, info.channels,
                            info.buffer_frames.map_or("?".into(), |v| v.to_string())));
                    } else {
                        ui.colored_label(TEXT_MUTED, self.settings.input.device_name.as_deref().unwrap_or("入力：自動選択（ASIO優先）"));
                        ui.small("設定で入力を確認してから開始");
                    }
                    if let Some(age) = self.freshness() {
                        ui.small(format!("結果の古さ {:.0} ms", age * 1000.0))
                            .on_hover_text("結果窓末尾からの推定経過時間です。入力から表示までの総遅延ではありません。");
                    }
                    let stats=self.runtime.stats();
                    let audio_drops=stats.dropped_audio_frames.load(Ordering::Relaxed);
                    let display_drops=stats.dropped_results.load(Ordering::Relaxed);
                    let clips=stats.clipped_frames.load(Ordering::Relaxed);
                    if (audio_drops>0 || display_drops>0 || clips>0)
                        && ui.small_button(RichText::new(format!("欠落 音声 {audio_drops} fr / 表示 {display_drops} · CLIP {clips} fr")).color(YELLOW))
                            .on_hover_text("この接続の累積値。音声欠落と表示キュー欠落は別の値です。クリックで診断を開きます。")
                            .clicked() { self.settings_page=3; self.show_settings=true; }
                });
                let issue = match &self.connection {
                    ConnectionState::Failed(e) => Some(format!("接続できません: {e}。設定を確認し、再接続してください。")),
                    ConnectionState::Running if self.runtime.stats().stream_errors.load(Ordering::Relaxed) > 0 => Some("入力エラーを検出。最終結果の時刻と診断を確認し、再接続してください。".into()),
                    ConnectionState::Running if self.live.as_ref().is_some_and(|s| s.invalid_samples > 0) => Some("無効な入力値を検出。この解析結果は使用できません。".into()),
                    ConnectionState::Running if self.freshness().is_some_and(|a| a > 0.25) => Some("入力停止または滞留。表示中の最終結果は古いため、現在の音として扱えません。".into()),
                    _ => None,
                };
                if let Some(issue) = issue { ui.colored_label(RED, issue); }
                if self.held_end.is_some() {
                    ui.colored_label(YELLOW, format!("表示を固定中 · 対象窓中心 {} s · {}",
                        self.held_sample_time.map_or("なし".into(), |t| format!("{t:.3}")),
                        if self.connection==ConnectionState::Running { "入力と履歴は継続" } else { "入力は停止／停止処理中" }));
                }
                if !self.notice.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("閉じる").clicked() { self.notice.clear(); }
                        ui.colored_label(YELLOW, &self.notice);
                    });
                }
            });
    }
    fn spectrum(&self, ui: &mut egui::Ui, height: f32) {
        let s = self.displayed();
        let advanced = self.advanced;
        let ready =
            s.is_some_and(|s| s.invalid_samples == 0 && (advanced == 0 || s.comparison_ready));
        let (low, high) = match advanced {
            1 => (0.0, 1.0),
            2 => (-60.0, 20.0),
            3 => (-180.0, 180.0),
            _ => (self.settings.minimum_dbfs as f64, 0.0),
        };
        let rate = s.map_or(48000, |s| s.sample_rate);
        let unit = match advanced {
            1 => "0–1",
            2 => "dB",
            3 => "deg",
            _ => "dBFS",
        };
        Plot::new((
            "spectrum",
            advanced,
            s.map(|s| (s.config.fft_size, s.sample_rate)),
        ))
        .height(height)
        .include_x(20.0_f64.log10())
        .include_x((rate as f64 * 0.5).log10())
        .include_y(low)
        .include_y(high)
        .set_margin_fraction(egui::vec2(0.01, 0.02))
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_background(false)
        .show_grid(false)
        .grid_spacing(8.0..=24.0)
        .x_grid_spacer(|_| {
            [
                20.0_f64, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
            ]
            .into_iter()
            .map(|hz| egui_plot::GridMark {
                value: hz.log10(),
                step_size: 1.0,
            })
            .collect()
        })
        .y_grid_spacer(move |_| {
            let step = match advanced {
                1 => 0.2,
                3 => 60.0,
                _ => 20.0,
            };
            ((low / step).ceil() as i32..=(high / step).floor() as i32)
                .map(|i| egui_plot::GridMark {
                    value: i as f64 * step,
                    step_size: step,
                })
                .collect()
        })
        .x_axis_label("周波数 · Hz（対数）")
        .y_axis_label(unit)
        .x_axis_formatter(|mark, _| {
            let hz = 10.0_f64.powf(mark.value);
            if hz >= 999.0 {
                format!("{:.0}k", hz / 1000.0)
            } else {
                format!("{hz:.0}")
            }
        })
        .label_formatter(move |name, value| {
            format!(
                "{name}\n{:.1} Hz · {:.2} {unit}",
                10.0_f64.powf(value.x),
                value.y
            )
        })
        .show(ui, |plot| {
            plot.set_plot_bounds(egui_plot::PlotBounds::from_min_max(
                [20.0_f64.log10(), low],
                [(rate as f64 * 0.5).log10(), high],
            ));
            for hz in [
                20.0_f64, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
            ] {
                plot.vline(
                    egui_plot::VLine::new(hz.log10())
                        .color(theme::OUTLINE.gamma_multiply(0.7))
                        .allow_hover(false),
                );
            }
            let step = match advanced {
                1 => 0.2,
                3 => 60.0,
                _ => 20.0,
            };
            for i in (low / step).ceil() as i32..=(high / step).floor() as i32 {
                plot.hline(
                    egui_plot::HLine::new(i as f64 * step)
                        .color(theme::OUTLINE.gamma_multiply(0.7))
                        .allow_hover(false),
                );
            }
            if advanced == 0
                && let Some(base) = &self.baseline
            {
                for (enabled, bands, color, name) in [
                    (
                        self.show_measurement,
                        Some(&base.measurement),
                        CYAN,
                        "青 基準",
                    ),
                    (
                        self.show_reference,
                        base.reference.as_ref(),
                        GREEN,
                        "緑 基準",
                    ),
                ] {
                    if enabled && let Some(bands) = bands {
                        plot.line(
                            Line::new(PlotPoints::from_iter(bands.iter().enumerate().map(
                                |(i, db)| [crate::visualizer::band_hz(i).log10(), *db as f64],
                            )))
                            .style(egui_plot::LineStyle::dashed_dense())
                            .color(color)
                            .name(name),
                        );
                    }
                }
            }
            if !ready {
                return;
            }
            let s = s.unwrap();
            let values = |reference: bool| {
                if advanced == 0 && self.baseline.is_some() {
                    // Match the stored reference's log-band grid. Overlaying it
                    // on raw FFT-bin centers would imply a frequency shift.
                    let bands = crate::history::history_spectrum(s, reference);
                    return PlotPoints::from_iter(
                        bands
                            .iter()
                            .enumerate()
                            .map(|(i, db)| [crate::visualizer::band_hz(i).log10(), *db as f64]),
                    );
                }
                PlotPoints::from_iter(s.bins[..s.bin_count].iter().enumerate().skip(1).filter_map(
                    |(i, b)| {
                        let hz = i as f64 * s.config.bin_hz(s.sample_rate);
                        if hz < 20.0
                            || (advanced > 0
                                && (b.reference_dbfs < -100.0 || b.measurement_dbfs < -100.0))
                        {
                            return None;
                        }
                        let value = if reference {
                            b.reference_dbfs
                        } else {
                            match advanced {
                                1 => b.coherence,
                                2 => b.transfer_db,
                                3 => b.phase_degrees,
                                _ => b.measurement_dbfs,
                            }
                        };
                        Some([hz.log10(), value as f64])
                    },
                ))
            };
            if self.show_measurement {
                if advanced == 0 {
                    plot.line(
                        Line::new(values(false))
                            .color(CYAN)
                            .width(1.7_f32)
                            .name("青入力"),
                    );
                } else {
                    plot.points(
                        Points::new(values(false))
                            .color(CYAN)
                            .radius(1.5_f32)
                            .name("有効帯域"),
                    );
                }
            }
            if advanced == 0 && self.show_reference && s.has_reference {
                plot.line(
                    Line::new(values(true))
                        .color(GREEN)
                        .width(1.7_f32)
                        .name("緑入力"),
                );
            }
        });
        if !ready {
            let message = if s.is_none() {
                "入力待ち · 開始するとスペクトラムを表示します"
            } else if advanced > 0 {
                "比較未確定 · 同期2入力・十分な信号・8窓以上の平均が必要"
            } else {
                "無効な入力 · この結果は表示できません"
            };
            ui.colored_label(if s.is_none() { TEXT_MUTED } else { YELLOW }, message);
        }
    }
    fn level_channel(&self, ui: &mut egui::Ui, reference: bool, compact: bool) {
        let s = self.displayed();
        let route = self
            .settings
            .input
            .route
            .unwrap_or(ChannelRoute::default_for_channels(1));
        let (name, color, level, clip) = if reference {
            (
                route.reference.map_or("緑 · 入力なし".into(), |c| {
                    format!("緑 · ch{}", c + 1)
                }),
                GREEN,
                s.and_then(|s| s.reference_level),
                s.is_some_and(|s| s.reference_clipped),
            )
        } else {
            (
                format!("青 · ch{}", route.measurement + 1),
                CYAN,
                s.map(|s| s.measurement_level),
                s.is_some_and(|s| s.measurement_clipped),
            )
        };
        ui.horizontal(|ui| {
            ui.colored_label(color, RichText::new(name).strong());
            if clip {
                ui.colored_label(RED, "CLIP")
                    .on_hover_text("サンプルの絶対値が1以上。比較条件は無効です。");
            }
        });
        if let Some(SignalLevel {
            peak_dbfs,
            rms_dbfs,
        }) = level.filter(|_| s.is_some_and(|s| s.invalid_samples == 0))
        {
            if compact {
                ui.monospace(format!("RMS {rms_dbfs:.1}  Peak {peak_dbfs:.1}"));
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{rms_dbfs:.1}"))
                            .monospace()
                            .size(24.0)
                            .color(color),
                    );
                    ui.label("RMS\ndBFS");
                });
                ui.label(RichText::new(format!("Peak  {peak_dbfs:6.1} dBFS")).monospace());
            }
            widgets::level_meter(ui, rms_dbfs, peak_dbfs, color, !compact);
        } else {
            ui.colored_label(TEXT_MUTED, "—  dBFS · 入力なし");
            widgets::level_meter(ui, -120.0, -120.0, color, !compact);
        }
    }
    fn levels(&self, ui: &mut egui::Ui, compact: bool) {
        let previous_spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new("デジタル音量 · dBFS").strong());
        if compact {
            ui.columns(2, |cols| {
                self.level_channel(&mut cols[0], false, true);
                self.level_channel(&mut cols[1], true, true);
            });
        } else {
            self.level_channel(ui, false, ui.ctx().screen_rect().height() < 950.0);
            ui.separator();
            self.level_channel(ui, true, ui.ctx().screen_rect().height() < 950.0);
            ui.small("RMS：窓内実効値\nPeak：サンプルピーク")
                .on_hover_text(
                    "正弦波のRMSはピーク振幅より約3.01dB低くなります。絶対音圧ではありません。",
                );
        }
        ui.spacing_mut().item_spacing.y = previous_spacing;
    }
    fn dashboard(&mut self, ctx: &egui::Context, now: f64) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::BACKGROUND)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.dashboard_content(ui, now);
                    // Target the actual laid-out bottom, including the newly
                    // opened reference panel and any native viewport changes.
                    if self.qa_history && self.launched.elapsed() > Duration::from_secs(1) {
                        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    }
                });
            });
    }
    fn spectrum_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let labels = ["スペクトラム", "コヒーレンス", "H1 · 参考", "位相 · 参考"];
            egui::ComboBox::from_id_salt("view")
                .width(126.0)
                .selected_text(labels[self.advanced])
                .show_ui(ui, |ui| {
                    for (i, label) in labels.into_iter().enumerate() {
                        ui.selectable_value(&mut self.advanced, i, label);
                    }
                });
            ui.checkbox(
                &mut self.show_measurement,
                RichText::new("青入力").color(CYAN),
            );
            ui.checkbox(
                &mut self.show_reference,
                RichText::new("緑入力").color(GREEN),
            );
        });
    }
    fn timeline_controls(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal_wrapped(|ui| {
            ui.label("時間範囲");
            let mut duration=self.viewport.visible_seconds();
            egui::ComboBox::from_id_salt("history_duration").width(90.0).selected_text(format!("{duration:.0} 秒")).show_ui(ui,|ui| {
                for seconds in [10.0,30.0,60.0,120.0,300.0,600.0] {ui.selectable_value(&mut duration,seconds,format!("{seconds:.0} 秒"));}
            });
            while self.viewport.visible_seconds()>duration {self.viewport.zoom_in();}
            while self.viewport.visible_seconds()<duration {self.viewport.zoom_out();}
            if ui.button("← 過去").clicked(){if self.held_end.is_none(){self.freeze(now);}self.viewport.pan_older();}
            if ui.add_enabled(self.viewport.offset_from_live()>0.0,egui::Button::new("新しい →")).clicked(){self.viewport.pan_newer();}
            if ui.button("現在へ").on_hover_text("L · HOLDと過去への移動を解除").clicked(){self.return_live();}
            ui.small(format!("保持 {} 秒 · 原履歴10Hz",self.settings.history_seconds)).on_hover_text("3Dは各時間区間で最大ピークを持つ実際の窓を選び、最大64断面を描画します。原履歴は間引かず保持し、2Dやカーソルで確認できます。音声は保存しません。");
        });
    }
    fn current_point(&self) -> Option<crate::history::HistoryPoint> {
        let s = self.displayed()?;
        Some(crate::history::HistoryPoint {
            seconds: self
                .held_sample_time
                .filter(|_| self.held_end.is_some())
                .unwrap_or(self.live_time),
            measurement_rms: s.measurement_level.rms_dbfs,
            reference_rms: s.reference_level.map(|l| l.rms_dbfs),
            spectrum_dbfs: crate::history::history_spectrum(s, false),
            reference_spectrum_dbfs: s
                .has_reference
                .then(|| crate::history::history_spectrum(s, true)),
            measurement_clipped: s.measurement_clipped,
            reference_clipped: s.reference_clipped,
            gap: s.invalid_samples > 0,
            segment: self.history.points().back().map_or(0, |p| p.segment),
            config: s.config,
            sample_rate: s.sample_rate,
        })
    }
    fn capture_reference(&mut self) {
        if let Some(p) = self
            .current_point()
            .filter(|p| !p.gap && !p.measurement_clipped && !p.reference_clipped)
        {
            self.baseline = Some(crate::visualizer::SpectrumReference {
                version: 1,
                seconds: p.seconds,
                sample_rate: p.sample_rate,
                config: p.config,
                description: format!(
                    "{} / {} / ゲイン・AGC不明",
                    self.history.label(p.segment),
                    self.settings.condition_notes
                )
                .chars()
                .take(4000)
                .collect(),
                measurement: p.spectrum_dbfs.to_vec(),
                reference: p.reference_spectrum_dbfs.map(|b| b.to_vec()),
            });
            self.show_spectrum = true;
        }
    }
    fn level_strip(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("総合レベル · 現在/HOLD窓 · dBFS");
            let s = self.displayed();
            let route = self
                .settings
                .input
                .route
                .unwrap_or(ChannelRoute::default_for_channels(1));
            for (name, level, color) in [
                (
                    format!("青 ch{}", route.measurement + 1),
                    s.map(|s| s.measurement_level),
                    CYAN,
                ),
                (
                    route
                        .reference
                        .map_or("緑なし".into(), |c| format!("緑 ch{}", c + 1)),
                    s.and_then(|s| s.reference_level),
                    GREEN,
                ),
            ] {
                ui.colored_label(color, name);
                if let Some(level) = level.filter(|_| s.is_some_and(|s| s.invalid_samples == 0)) {
                    ui.monospace(format!(
                        "RMS {:.1}  Peak {:.1}",
                        level.rms_dbfs, level.peak_dbfs
                    ));
                } else {
                    ui.colored_label(TEXT_MUTED, "—");
                }
            }
        });
    }
    fn dashboard_content(&mut self, ui: &mut egui::Ui, now: f64) {
        use crate::visualizer::{Camera, ViewMode};
        let compact = ui.ctx().screen_rect().height() < 750.0;
        ui.spacing_mut().item_spacing.y = if compact { 4.0 } else { 8.0 };
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.settings.visualizer_mode,
                ViewMode::Waterfall,
                "3D ウォーターフォール",
            );
            ui.selectable_value(
                &mut self.settings.visualizer_mode,
                ViewMode::Heatmap,
                "2D 周波数×時間",
            );
            let route = self
                .settings
                .input
                .route
                .unwrap_or(ChannelRoute::default_for_channels(1));
            ui.checkbox(
                &mut self.show_measurement,
                RichText::new(format!("青 ch{}", route.measurement + 1)).color(CYAN),
            );
            ui.checkbox(
                &mut self.show_reference,
                RichText::new(
                    route
                        .reference
                        .map_or("緑入力なし".into(), |c| format!("緑 ch{}", c + 1)),
                )
                .color(GREEN),
            )
            .on_hover_text(
                "入力番号はストリーム内の物理入力です。マイク2本を自動でL/Rには割り当てません。",
            );
            ui.label(
                RichText::new(format!(
                    "共通レンジ {:.0}〜0 dBFS",
                    self.settings.minimum_dbfs
                ))
                .monospace(),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(self.settings.visualizer_mode == ViewMode::Waterfall, |ui| {
                if ui.button("視点リセット").clicked() {
                    self.settings.camera = Camera::default();
                }
                ui.add(egui::Slider::new(&mut self.settings.camera.yaw, -1.0..=1.0).text("回転"));
                ui.add(egui::Slider::new(&mut self.settings.camera.zoom, 0.65..=2.0).text("拡大"));
            });
            let valid = self
                .current_point()
                .is_some_and(|p| !p.gap && !p.measurement_clipped && !p.reference_clipped);
            if ui
                .add_enabled(valid, egui::Button::new("基準を保持"))
                .on_hover_text(
                    "現在/HOLD窓の帯域スペクトルを保持。クリップ・無効値は基準にしません。",
                )
                .clicked()
            {
                self.capture_reference();
            }
            ui.toggle_value(&mut self.show_spectrum, "断面 / 基準");
        });
        self.timeline_controls(ui, now);
        self.level_strip(ui);
        let config = self
            .displayed()
            .map_or(self.settings.analysis, |s| s.config);
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "FFT {} · {:?} · 平均 {} ms · 高さは帯域内の最大FFTビン振幅、下図は窓内RMS",
                config.fft_size, config.window, config.averaging_ms
            ));
            if ui.small_button("解析条件").clicked() {
                self.settings_page = 1;
                self.show_settings = true;
            }
        });
        let end = self.held_end.unwrap_or((now * 10.0).floor() / 10.0);
        let (start, visible_end) = self.viewport.time_bounds(end);
        let current = self.current_point();
        let height = (ui.ctx().screen_rect().height() - 560.0).clamp(150.0, 400.0);
        self.waterfall.show(
            ui,
            waterfall::View {
                points: self.history.points(),
                start,
                end: visible_end,
                minimum: self.settings.minimum_dbfs,
                channels: [self.show_measurement, self.show_reference],
                mode: self.settings.visualizer_mode,
                live: current
                    .as_ref()
                    .filter(|_| self.viewport.offset_from_live() == 0.0),
                baseline: self.baseline.as_ref(),
            },
            &mut self.settings.camera,
            &mut self.cursor,
            &mut self.cursor_band,
            height,
        );
        ui.horizontal_wrapped(|ui|{
            ui.small("ドラッグで回転 · ホイールで拡大 · ダブルクリックで視点リセット · 曲線/2D上で履歴値を確認");
            ui.colored_label(RED,RichText::new("赤印：条件境界・欠測・CLIP").small());
            if self.settings.visualizer_mode==ViewMode::Heatmap{widgets::color_scale(ui,self.settings.minimum_dbfs);}
        });
        ui.small(
            "96対数帯域 · FFTビンなしは床/背景（値なし） · 解析下限−120 dBFS · 自動正規化なし",
        );
        ui.label(RichText::new("総合レベル履歴 · 窓内RMS dBFS · 上図と共通時刻").strong());
        widgets::history_plot(
            ui,
            self.history.points(),
            self.viewport,
            end,
            &mut self.cursor,
            if compact { 76.0 } else { 94.0 },
            [self.show_measurement, self.show_reference],
        );
        self.selection_controls(ui, start, visible_end);
        if self.show_spectrum {
            ui.separator();
            ui.label(RichText::new("現在 / HOLD窓のスペクトラム・基準比較").strong());
            if self.baseline.is_some() && self.advanced == 0 {
                ui.small("現在と基準を共通の96対数帯域で比較（帯域内の最大FFTビン振幅）");
            }
            self.spectrum_controls(ui);
            self.spectrum(ui, 180.0);
        }
        if let Some(base) = self.baseline.as_ref() {
            let mismatch = current
                .as_ref()
                .is_some_and(|p| p.sample_rate != base.sample_rate || p.config != base.config);
            if mismatch {
                ui.colored_label(YELLOW,"基準と現在のレート／解析条件が異なります。ゲイン・位置・入力処理も手動確認してください。");
            }
            ui.small(format!(
                "基準：{:.3}s / {} Hz / FFT {} / {:?} / 平均 {}ms · 点線は手前に投影",
                base.seconds,
                base.sample_rate,
                base.config.fft_size,
                base.config.window,
                base.config.averaging_ms
            ));
            egui::CollapsingHeader::new("基準の入力・比較条件").show(ui, |ui| {
                ui.label(&base.description);
                ui.small("入力ch・ゲイン・位置・AGCが一致するか手動で確認してください。");
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("基準を消去").clicked() {
                    self.baseline = None;
                }
                if ui
                    .add_enabled(
                        self.settings_path.is_some(),
                        egui::Button::new("基準をローカル保存"),
                    )
                    .clicked()
                    && let (Some(base), Some(path)) = (&self.baseline, self.reference_path())
                {
                    self.notice = base.save(&path).map_or_else(
                        |e| e,
                        |_| format!("解析結果の基準を保存: {}", path.display()),
                    );
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.settings_path.is_some(),
                    egui::Button::new("保存基準を読込"),
                )
                .clicked()
                && let Some(path) = self.reference_path()
            {
                match crate::visualizer::SpectrumReference::load(&path) {
                    Ok(v) => {
                        self.baseline = Some(v);
                        self.show_spectrum = true;
                    }
                    Err(e) => self.notice = e,
                }
            }
            ui.small("基準は解析結果だけ。保存しない保持基準は終了時に消えます。");
        });
    }
    fn reference_path(&self) -> Option<PathBuf> {
        self.settings_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("spectrum-reference.json"))
    }
    fn selection_controls(&mut self, ui: &mut egui::Ui, start: f64, end: f64) {
        let mut time = self.cursor.unwrap_or(end).clamp(start, end);
        ui.horizontal_wrapped(|ui| {
            ui.label("履歴カーソル");
            if ui
                .add(
                    egui::Slider::new(&mut time, start..=end)
                        .text("秒")
                        .fixed_decimals(2),
                )
                .changed()
            {
                self.cursor = Some(time);
            }
            ui.add(
                egui::Slider::new(&mut self.cursor_band, 0..=HISTORY_SPECTRUM_BANDS - 1)
                    .text("帯域"),
            );
            if ui.small_button("解除").clicked() {
                self.cursor = None;
            }
        });
        if let Some(t) = self.cursor {
            if let Some(p) = self.history.at(t) {
                let hz = crate::visualizer::band_hz(self.cursor_band);
                ui.horizontal_wrapped(|ui| {
                    ui.monospace(format!("窓時刻 {:.3}s · 約{hz:.1}Hz帯", p.seconds));
                    if !crate::visualizer::band_has_bins(self.cursor_band, p.config, p.sample_rate)
                    {
                        ui.colored_label(
                            YELLOW,
                            "帯域値なし：この窓のFFTビンがない帯域（Nyquist上限を含む）",
                        );
                        return;
                    }
                    ui.colored_label(
                        CYAN,
                        format!(
                            "帯域最大 {:.1} / RMS {:.1} dBFS",
                            p.spectrum_dbfs[self.cursor_band], p.measurement_rms
                        ),
                    );
                    if let Some(b) = p.reference_spectrum_dbfs {
                        ui.colored_label(
                            GREEN,
                            format!(
                                "帯域最大 {:.1} / RMS {:.1} dBFS",
                                b[self.cursor_band],
                                p.reference_rms.unwrap_or(-120.0)
                            ),
                        );
                    }
                    if p.measurement_clipped || p.reference_clipped {
                        ui.colored_label(RED, "CLIP · 比較不可");
                    }
                });
                egui::CollapsingHeader::new("選択窓の入力・解析条件").show(ui, |ui| {
                    ui.label(self.history.label(p.segment));
                    ui.monospace(format!(
                        "{}Hz / FFT {} / {:?} / 平均 {}ms",
                        p.sample_rate, p.config.fft_size, p.config.window, p.config.averaging_ms
                    ));
                });
            } else {
                ui.colored_label(
                    YELLOW,
                    format!("{t:.3}s · 入力なし／欠測／条件境界／保持範囲外"),
                );
            }
        } else {
            ui.small(
                "上図・RMS履歴・スライダーは同じ履歴を参照します。帯域値と総合RMSは異なる量です。",
            );
        }
    }
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let screen = ctx.screen_rect();
        egui::Window::new("測定設定")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(
                (screen.width() - 48.0).clamp(260.0, 640.0),
                (screen.height() - 96.0).clamp(250.0, 600.0),
            ))
            .show(ctx, |ui| self.settings_content(ui));
        self.show_settings &= open;
    }
    fn save_settings(&mut self) {
        self.notice = match &self.settings_path {
            Some(path) => self
                .settings
                .save(path)
                .map_or_else(|e| e, |_| format!("条件を保存しました: {}", path.display())),
            None => "テストモードでは設定ファイルを書き換えません".into(),
        };
    }
    fn settings_content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["入力", "解析・表示", "比較条件", "診断"]
                .into_iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.settings_page, i, label);
            }
        });
        ui.separator();
        let body_height = (ui.available_height() - 64.0).max(100.0);
        egui::ScrollArea::vertical().id_salt(("settings_body",self.settings_page)).max_height(body_height).min_scrolled_height(body_height).show(ui, |ui| {
            match self.settings_page {
                0 => self.input_page(ui),
                1 => {
                    self.analysis_controls(ui);
                    ui.separator();
                    ui.add(egui::Slider::new(&mut self.settings.minimum_dbfs,-160.0..=-20.0).text("表示下限 · dBFS"));
                    ui.add(egui::Slider::new(&mut self.settings.history_seconds,60..=600).text("履歴保持 · 秒"));
                    self.history.set_retention(self.settings.history_seconds);
                    ui.small("平均化はスペクトル電力と相互スペクトルの指数平均。0は平均なし。観測窓・平均化を長くすると追従は遅くなります。");
                    ui.small("履歴は解析結果のみ。表示幅はメイン画面で10〜600秒に変更できます。");
                }
                2 => self.conditions_page(ui),
                _ => self.diagnostics(ui),
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("条件を保存")
                .on_hover_text("入力や解析条件の設定だけをローカルに保存。生音声は保存しません。")
                .clicked()
            {
                self.save_settings();
            }
            if ui
                .button("閉じる")
                .on_hover_text("Esc · 条件の変更は即時適用。保存は次回起動用です。")
                .clicked()
            {
                self.show_settings = false;
            }
            ui.small("変更は即時適用 · 保存は次回用");
        });
        if !self.notice.is_empty() {
            ui.colored_label(YELLOW, &self.notice);
        }
    }
    fn input_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("入力経路");
        let can_select = matches!(
            self.connection,
            ConnectionState::Stopped | ConnectionState::Failed(_)
        );
        if self.demo {
            ui.colored_label(
                YELLOW,
                "合成入力モード · マイク収録／スピーカー出力はありません",
            );
        } else if !can_select {
            ui.colored_label(
                YELLOW,
                "機器・レート・バッファを変更するには、入力を停止してください。",
            );
        }
        ui.add_enabled_ui(can_select && !self.demo, |ui| self.input_controls(ui));
        ui.small(
            "自動選択の開始時だけASIOからWASAPI共有へ切替可能。明示した機器は自動置換しません。",
        );
        ui.separator();
        ui.heading("入力チャンネル");
        if let Some(info) = self.runtime.info() {
            let channels = info.channels;
            let mut route = self.settings.input.route.unwrap_or(info.initial_route);
            let before = route;
            egui::ComboBox::from_label("青入力")
                .selected_text(format!("ch{}", route.measurement + 1))
                .show_ui(ui, |ui| {
                    for c in 0..channels {
                        ui.selectable_value(&mut route.measurement, c, format!("ch{}", c + 1));
                    }
                });
            egui::ComboBox::from_label("緑入力 · 比較ペア")
                .selected_text(
                    route
                        .reference
                        .map_or("なし · モノラル".into(), |c| format!("ch{}", c + 1)),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut route.reference, None, "なし · モノラル");
                    for c in 0..channels {
                        ui.selectable_value(&mut route.reference, Some(c), format!("ch{}", c + 1));
                    }
                });
            if route != before {
                match self.runtime.set_route(route) {
                    Ok(()) => {
                        self.settings.input.route = Some(route);
                        self.segment("入力ch切替");
                    }
                    Err(e) => self.notice = e,
                }
            }
        } else {
            ui.label("接続後に、そのストリームの入力を割り当てられます。内蔵マイク1入力でも利用できます。");
        }
    }
    fn conditions_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("同条件で比べるために");
        ui.colored_label(
            YELLOW,
            "入力処理（AGC・ノイズ抑制）とアナログゲインは自動取得できず、不明です。",
        );
        ui.label("ゲイン、マイク位置・向き、フィルター、OS処理を確認してください。内蔵マイクとM2は別条件で評価します。");
        ui.label("再現用メモ");
        ui.add(
            egui::TextEdit::multiline(&mut self.settings.condition_notes)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(
                self.connection == ConnectionState::Running,
                egui::Button::new("条件変更の境界を記録"),
            )
            .on_hover_text("ゲイン・位置などの変更を履歴で区切り、平均をリセットします。")
            .clicked()
        {
            match self.runtime.set_analysis(self.settings.analysis) {
                Ok(()) => self.segment("手動の測定条件変更"),
                Err(e) => self.notice = e,
            }
        }
        ui.separator();
        ui.label("観測スペクトルは音源・スピーカー・部屋・マイク・IFを含む系です。部屋単体の応答や校正済み音圧ではありません。");
        ui.label("コヒーレンス・H1・位相は同一ストリームの同期2入力での参考値。異なるデバイスのクロック同期、IR、残響、精密な伝達測定は保証しません。");
        ui.small(
            "生音声の常時保存・外部送信は行いません。信号生成・ループバックは初版対象外です。",
        );
    }
    fn diagnostics(&self, ui: &mut egui::Ui) {
        self.levels(ui, true);
        ui.separator();
        ui.heading("入力とデータの状態");
        if let Some(info) = self.runtime.info() {
            ui.label(format!("{} · {}", info.host_name, info.device_name));
            ui.monospace(format!(
                "適用 {} Hz / {} ch / buffer {:?} frames",
                info.sample_rate, info.channels, info.buffer_frames
            ));
            if !info.selection_notes.is_empty() {
                ui.colored_label(YELLOW, &info.selection_notes);
            }
        }
        let stats = self.runtime.stats();
        for (label, value) in [
            (
                "無効入力 / frames",
                stats.invalid_samples.load(Ordering::Relaxed),
            ),
            (
                "クリップ / frames",
                stats.clipped_frames.load(Ordering::Relaxed),
            ),
            (
                "入力欠落 / frames",
                stats.dropped_audio_frames.load(Ordering::Relaxed),
            ),
            (
                "表示キュー欠落 / results",
                stats.dropped_results.load(Ordering::Relaxed),
            ),
            (
                "最新値へ集約 / results",
                stats.coalesced_results.load(Ordering::Relaxed),
            ),
            (
                "ストリームエラー",
                stats.stream_errors.load(Ordering::Relaxed),
            ),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.monospace(value.to_string());
            });
        }
        ui.monospace(format!(
            "callback {}..{} frames",
            stats.callback_min_frames.load(Ordering::Relaxed),
            stats.callback_max_frames.load(Ordering::Relaxed)
        ));
        if let Some(age) = self.freshness() {
            ui.label(format!(
                "結果末尾の古さ 推定 {:.1} ms · 総入力遅延ではありません",
                age * 1000.0
            ));
        }
        ui.label(format!(
            "最終正常な窓中心 {} s",
            self.last_valid_time
                .map_or("なし".into(), |t| format!("{t:.3}"))
        ));
        ui.small("表示への集約と音声欠落は別の値です。赤い履歴境界や古さと合わせて判断してください。接続開始／取消のドライバー内部待機は強制中断できません。");
        ui.separator();
        ui.label("キーボード：F5 開始/停止 · H 表示固定 · L 現在へ · F1 設定 · Esc 設定を閉じる");
    }
    fn input_controls(&mut self, ui: &mut egui::Ui) {
        let selected = self
            .settings
            .input
            .device_name
            .clone()
            .unwrap_or("自動 / 開始時に選択".into());
        egui::ComboBox::from_label("機器 / バックエンド")
            .selected_text(selected)
            .width((ui.available_width() - 150.0).clamp(100.0, 420.0))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.settings.input.device_id.is_none(), "自動（ASIO優先）")
                    .clicked()
                {
                    self.settings.input.host = None;
                    self.settings.input.device_id = None;
                    self.settings.input.device_name = None;
                    self.settings.input.route = None;
                }
                for d in &self.devices {
                    if ui
                        .selectable_label(
                            self.settings.input.device_id.as_deref() == Some(&d.id),
                            format!("{} / {}", d.host, d.name),
                        )
                        .clicked()
                    {
                        self.settings.input.host = Some(d.host.clone());
                        self.settings.input.device_id = Some(d.id.clone());
                        self.settings.input.device_name = Some(d.name.clone());
                        self.settings.input.route = None;
                    }
                }
            });
        if ui
            .add_enabled(
                self.discovery.is_none(),
                egui::Button::new("入力一覧を再取得（収録しません）"),
            )
            .clicked()
        {
            self.discover();
        }
        if self.discovery.is_some() {
            ui.label("ドライバーの入力一覧を照会中…（画面は操作可能）");
        }
        for e in &self.discovery_errors {
            ui.colored_label(YELLOW, e);
        }
        if let Some(d) = self
            .devices
            .iter()
            .find(|d| Some(&d.id) == self.settings.input.device_id.as_ref())
        {
            egui::CollapsingHeader::new("機器が報告した対応形式").show(ui, |ui| {
                for f in &d.formats {
                    ui.label(f);
                }
            });
        } else if self.settings.input.device_id.is_some() && self.discovery.is_none() {
            ui.colored_label(
                YELLOW,
                "保存済み機器は一覧にありません。別の機器へ自動置換しません。",
            );
        }
        let rate = self
            .settings
            .input
            .sample_rate
            .map_or("機器既定".into(), |r| format!("{r} Hz"));
        egui::ComboBox::from_label("要求レート")
            .selected_text(rate)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.settings.input.sample_rate, None, "機器既定");
                for r in [44100, 48000, 96000, 192000] {
                    ui.selectable_value(
                        &mut self.settings.input.sample_rate,
                        Some(r),
                        format!("{r} Hz"),
                    );
                }
            });
        let buffer = self
            .settings
            .input
            .buffer_frames
            .map_or("機器既定".into(), |b| format!("{b} frames"));
        egui::ComboBox::from_label("要求バッファ")
            .selected_text(buffer)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.settings.input.buffer_frames, None, "機器既定");
                for b in [64, 128, 256, 512, 1024] {
                    ui.selectable_value(
                        &mut self.settings.input.buffer_frames,
                        Some(b),
                        format!("{b} frames"),
                    );
                }
            });
    }
    fn analysis_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("解析条件");
        let before = self.settings.analysis;
        ui.horizontal_wrapped(|ui| {
            if ui.button("高速 / 1024・平均なし").clicked() {
                self.settings.analysis = AnalysisConfig::FAST;
            }
            if ui.button("精密 / 8192・500ms平均").clicked() {
                self.settings.analysis = AnalysisConfig::PRECISE;
            }
        });
        egui::ComboBox::from_label("FFT点数")
            .selected_text(self.settings.analysis.fft_size.to_string())
            .show_ui(ui, |ui| {
                for size in [1024, 2048, 8192] {
                    ui.selectable_value(
                        &mut self.settings.analysis.fft_size,
                        size,
                        size.to_string(),
                    );
                }
            });
        egui::ComboBox::from_label("窓関数")
            .selected_text(format!("{:?}", self.settings.analysis.window))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.settings.analysis.window,
                    WindowFunction::Hann,
                    "Hann",
                );
                ui.selectable_value(
                    &mut self.settings.analysis.window,
                    WindowFunction::Rectangular,
                    "Rectangular",
                );
            });
        ui.add(
            egui::Slider::new(&mut self.settings.analysis.averaging_ms, 0..=2000)
                .text("平均時定数 / ms（0=なし）"),
        );
        if self.settings.analysis != before {
            if self.connection == ConnectionState::Running {
                match self.runtime.set_analysis(self.settings.analysis) {
                    Ok(()) => self.segment("解析条件変更"),
                    Err(e) => {
                        self.notice = e;
                        self.settings.analysis = before;
                    }
                }
            }
            self.waterfall.invalidate();
        }
    }
    fn native_qa(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_none() {
            return;
        }
        let size = ctx.screen_rect().size() * ctx.pixels_per_point();
        let size = [size.x.max(0.0) as u32, size.y.max(0.0) as u32];
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimized || size.contains(&0) {
            self.qa_capture_size = None;
            self.qa_stable_capture_frames = 0;
        } else if self.qa_capture_size == Some(size) {
            self.qa_stable_capture_frames = self.qa_stable_capture_frames.saturating_add(1);
        } else {
            self.qa_capture_size = Some(size);
            self.qa_stable_capture_frames = 0;
        }
        if !self.screenshot_requested
            && self.qa_stable_capture_frames >= 2
            && self.launched.elapsed().as_secs_f64() > self.qa_delay_seconds
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.screenshot_requested = true;
        }
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Screenshot { image, .. } = event
                && let Some(path) = &self.screenshot
            {
                // The native window can minimize between the request and GL read.
                // An empty image is a deferred capture, never a successful PNG.
                if image.size.contains(&0) || image.pixels.is_empty() {
                    self.screenshot_requested = false;
                    self.qa_capture_size = None;
                    self.qa_stable_capture_frames = 0;
                    eprintln!("Empty framebuffer capture deferred until the window is restored");
                    continue;
                }
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match image::save_buffer(
                    path,
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    Ok(()) => {
                        println!(
                            "Native screenshot: {} ({}x{})",
                            path.display(),
                            image.size[0],
                            image.size[1]
                        );
                    }
                    Err(e) => eprintln!("Screenshot failed: {e}"),
                }
                if !self.tick_intervals.is_empty() {
                    self.tick_intervals.sort_by(f64::total_cmp);
                    println!(
                        "UI tick intervals: n={} p95={:.3} ms max={:.3} ms; not end-to-end audio latency",
                        self.tick_intervals.len(),
                        self.tick_intervals[(self.tick_intervals.len() - 1) * 95 / 100],
                        self.tick_intervals.last().unwrap()
                    );
                }
                println!(
                    "QA state: held_end={:?}, displayed_sequence={:?}, live_sequence={:?}, newest_history={:?}",
                    self.held_end,
                    self.displayed().map(|s| s.sequence),
                    self.live.as_ref().map(|s| s.sequence),
                    self.history.points().back().map(|p| p.seconds)
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.launched.elapsed().as_secs_f64() > self.qa_delay_seconds + 15.0 {
            eprintln!("Screenshot timeout");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn endurance_before(&mut self, ctx: &egui::Context, now: f64) {
        use endurance::{Action, SCHEDULE};
        let Some(mut trace) = self.endurance.take() else {
            return;
        };
        let t = trace.elapsed();
        if !trace.complete {
            while trace.next < SCHEDULE.len() && t >= SCHEDULE[trace.next].0 {
                let action = SCHEDULE[trace.next].1;
                trace.next += 1;
                match action {
                    Action::Orbit => trace.event("orbit_start"),
                    Action::Reference => {
                        self.capture_reference();
                        trace.event("reference_capture_and_compare");
                    }
                    Action::Hold => {
                        self.freeze(now);
                        trace.event("hold_on");
                    }
                    Action::Live => {
                        self.return_live();
                        trace.event("hold_off");
                    }
                    Action::Wide => {
                        while self.viewport.visible_seconds() < 600.0 {
                            self.viewport.zoom_out();
                        }
                        trace.event("history_span_600s");
                    }
                    Action::Reset => {
                        self.settings.camera = Default::default();
                        trace.event("camera_reset");
                    }
                    Action::Finish => {}
                }
            }
            if endurance::rotating(t) {
                self.settings.camera.yaw = (t * 0.18).sin() as f32 * 0.7;
                self.settings.camera.pitch = 0.6 + (t * 0.13).sin() as f32 * 0.2;
                self.settings.camera.zoom = 1.0 + (t * 0.11).sin() as f32 * 0.15;
            }
        } else {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event
                    && !image.size.contains(&0)
                {
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    match image::save_buffer(
                        trace.directory.join("final.png"),
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    ) {
                        Ok(()) => {
                            trace.event("screenshot_written_after_measurement");
                            println!("ENDURANCE final PNG written");
                        }
                        Err(e) => eprintln!("ENDURANCE screenshot write failed: {e}"),
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if t > endurance::SECONDS + 15.0 {
                trace.event("post_measurement_screenshot_timeout");
                eprintln!("ENDURANCE screenshot timeout; measurement retained");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.endurance = Some(trace);
    }
    fn endurance_after(&mut self, ctx: &egui::Context, began: Instant, previous_cpu_ms: f64) {
        let Some(mut trace) = self.endurance.take() else {
            return;
        };
        if !trace.complete {
            let instant = began;
            let t = trace.elapsed();
            let wall = endurance::utc_ms();
            let (uptime, awake) = endurance::clocks();
            let (interval, wall_delta, awake_delta, uptime_delta) = trace
                .last
                .replace((instant, wall, awake, uptime))
                .map_or((0.0, 0.0, 0.0, 0.0), |(last, w, a, u)| {
                    (
                        (instant - last).as_secs_f64() * 1000.0,
                        wall - w,
                        awake - a,
                        uptime - u,
                    )
                });
            if t as u64 != trace.gap_second {
                trace.gaps = self.history.points().iter().filter(|p| p.gap).count();
                trace.gap_second = t as u64;
                if (t as u64).is_multiple_of(30) {
                    println!(
                        "ENDURANCE t={t:.1}s history={} gaps={} held={} phase={}",
                        self.history.points().len(),
                        trace.gaps,
                        self.held_end.is_some(),
                        endurance::phase(t)
                    );
                }
            }
            let minimized =
                ctx.input(|i| i.viewport().minimized.map_or(-1.0, |v| u8::from(v) as f64));
            let focused = ctx.input(|i| u8::from(i.focused) as f64);
            let size = ctx.screen_rect().size();
            let stats = self.runtime.stats();
            let row = [
                t,
                wall,
                interval,
                wall_delta,
                awake_delta,
                uptime_delta,
                began.elapsed().as_secs_f64() * 1000.0,
                previous_cpu_ms,
                minimized,
                focused,
                size.x as f64,
                size.y as f64,
                ctx.pixels_per_point() as f64,
                u8::from(self.held_end.is_some()) as f64,
                u8::from(self.baseline.is_some()) as f64,
                self.viewport.visible_seconds(),
                self.history.points().len() as f64,
                self.history.points().front().map_or(-1.0, |p| p.seconds),
                self.history.points().back().map_or(-1.0, |p| p.seconds),
                trace.gaps as f64,
                stats.dropped_audio_frames.load(Ordering::Relaxed) as f64,
                stats.dropped_results.load(Ordering::Relaxed) as f64,
                stats.coalesced_results.load(Ordering::Relaxed) as f64,
                stats.received_audio_frames.load(Ordering::Relaxed) as f64,
                self.waterfall.vertices as f64,
                self.waterfall.rebuild_ms,
                u8::from(endurance::rotating(t)) as f64,
                endurance::phase(t) as f64,
                self.live.as_ref().map_or(-1.0, |s| s.sequence as f64),
                self.displayed().map_or(-1.0, |s| s.sequence as f64),
                self.freshness().map_or(-1.0, |a| a * 1000.0),
            ];
            trace.frame(row);
            if interval > 50.0 {
                trace.event("interval_over_50ms");
            }
            if t >= endurance::SECONDS {
                trace.complete = true;
                trace.event("measurement_end");
                let summary = serde_json::json!({"requested_seconds":endurance::SECONDS,"elapsed_seconds":t,"frames":trace.frames,"trace_rows_dropped":trace.dropped,"history_records":self.history.points().len(),"history_oldest":self.history.points().front().map(|p|p.seconds),"history_newest":self.history.points().back().map(|p|p.seconds),"history_gaps":trace.gaps,"audio_drops":stats.dropped_audio_frames.load(Ordering::Relaxed),"result_drops":stats.dropped_results.load(Ordering::Relaxed),"invalid_samples":stats.invalid_samples.load(Ordering::Relaxed),"stream_errors":stats.stream_errors.load(Ordering::Relaxed),"actions_executed":trace.next,"no_hardware":self.demo});
                let write = || -> Result<(), Box<dyn std::error::Error>> {
                    std::fs::write(
                        trace.directory.join("summary.json"),
                        serde_json::to_vec_pretty(&summary)?,
                    )?;
                    use std::io::Write;
                    let mut out = std::io::BufWriter::new(std::fs::File::create(
                        trace.directory.join("history.csv"),
                    )?);
                    writeln!(
                        out,
                        "seconds,gap,segment,measurement_rms,reference_rms,sample_rate,fft_size"
                    )?;
                    for p in self.history.points() {
                        writeln!(
                            out,
                            "{:.6},{},{},{:.6},{:.6},{},{}",
                            p.seconds,
                            p.gap,
                            p.segment,
                            p.measurement_rms,
                            p.reference_rms.unwrap_or(-120.0),
                            p.sample_rate,
                            p.config.fft_size
                        )?;
                    }
                    out.flush()?;
                    Ok(())
                };
                if let Err(e) = write() {
                    eprintln!("ENDURANCE result write failed: {e}");
                }
                println!("ENDURANCE measurement completed at {t:.3}s");
                trace.event("screenshot_request_after_measurement");
                trace.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        self.endurance = Some(trace);
    }
    fn tick(&mut self, ctx: &egui::Context, previous_cpu_ms: f64) {
        let instant = Instant::now();
        if let Some(last) = self.last_tick.replace(instant)
            && self.tick_intervals.len() < 4096
        {
            self.tick_intervals
                .push(instant.duration_since(last).as_secs_f64() * 1000.0);
        }
        let now = ctx.input(|i| i.time);
        self.poll(now);
        self.endurance_before(ctx, now);
        if self.screenshot.is_some()
            && self.live.is_some()
            && self.launched.elapsed() >= Duration::from_secs(1)
        {
            if self.qa_reference {
                self.capture_reference();
                self.qa_reference = false;
            }
            if self.qa_cursor {
                self.cursor = Some(self.live_time);
                self.cursor_band = 59;
                self.qa_cursor = false;
            }
        }
        if self.qa_hold
            && self.launched.elapsed() >= Duration::from_millis(1500)
            && self.live.is_some()
        {
            self.freeze(now);
            self.qa_hold = false;
        }
        self.shortcuts(ctx, now);
        self.header(ctx, now);
        egui::TopBottomPanel::bottom("measurement_scope").frame(egui::Frame::none().fill(theme::SURFACE).inner_margin(egui::Margin::symmetric(16.0,6.0))).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.small("dBFS · 相対比較").on_hover_text("音源・スピーカー・部屋・マイク・IFを含む観測系です。部屋単体の応答や校正済み音圧ではありません。");
                ui.colored_label(YELLOW,RichText::new("AGC・入力処理：不明").small()).on_hover_text("入力処理・アナログゲインは自動取得できません。ゲイン変更で比較条件が変化します。内蔵マイクとM2は別条件で評価します。");
                if ui.small_button("測定上の注意").clicked() { self.settings_page=2; self.show_settings=true; }
            });
        });
        self.dashboard(ctx, now);
        self.settings_window(ctx);
        self.native_qa(ctx);
        self.endurance_after(ctx, instant, previous_cpu_ms);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
impl eframe::App for AnalyzerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.tick(
            ctx,
            frame.info().cpu_usage.map_or(-1.0, |s| s as f64 * 1000.0),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::RuntimeStats,
        source::{InputSource, OpenedSource},
    };
    use std::sync::{Arc, atomic::AtomicBool};
    struct Unavailable;
    impl InputSource for Unavailable {
        fn open(&mut self, _: Arc<RuntimeStats>, _: &AtomicBool) -> Result<OpenedSource, String> {
            Err("test device unavailable".into())
        }
    }
    #[test]
    fn empty_capture_event_is_deferred_without_writing_or_closing() {
        let path = std::env::temp_dir().join(format!(
            "audio-analyzer-empty-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = AnalyzerApp::with_options(
            AnalyzerController::with_source(Unavailable).unwrap(),
            AppOptions {
                demo: true,
                screenshot: Some(path.clone()),
                ..Default::default()
            },
        );
        app.screenshot_requested = true;
        let ctx = egui::Context::default();
        let out = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Screenshot {
                    viewport_id: egui::ViewportId::ROOT,
                    user_data: Default::default(),
                    image: Arc::new(egui::ColorImage::new([0, 0], egui::Color32::TRANSPARENT)),
                }],
                ..Default::default()
            },
            |ctx| app.native_qa(ctx),
        );
        assert!(!app.screenshot_requested);
        assert_eq!(app.qa_stable_capture_frames, 0);
        assert!(!path.exists());
        assert!(
            !out.viewport_output
                .values()
                .flat_map(|o| &o.commands)
                .any(|cmd| matches!(cmd, egui::ViewportCommand::Close))
        );
    }

    #[test]
    fn minimized_capture_waits_and_resumed_stable_viewport_requests_again() {
        let mut app = AnalyzerApp::with_options(
            AnalyzerController::with_source(Unavailable).unwrap(),
            AppOptions {
                demo: true,
                screenshot: Some(PathBuf::from("unused.png")),
                ..Default::default()
            },
        );
        app.launched = Instant::now() - Duration::from_secs(6);
        let ctx = egui::Context::default();
        for minimized in [true, true, true, false, false, false] {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 640.0),
                )),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .minimized = Some(minimized);
            let _ = ctx.run(input, |ctx| app.native_qa(ctx));
            if minimized {
                assert!(!app.screenshot_requested);
            }
        }
        assert!(
            app.screenshot_requested,
            "a stable restored window must retry the capture"
        );
    }
    #[test]
    fn keyboard_hold_repeat_settings_guard_and_return_live() {
        let mut app = AnalyzerApp::with_options(
            AnalyzerController::with_source(Unavailable).unwrap(),
            AppOptions {
                demo: true,
                ..Default::default()
            },
        );
        app.live = Some(Box::new(AnalysisSnapshot::default()));
        let ctx = egui::Context::default();
        let mut key = |key, repeat| {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: true,
                        repeat,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| app.shortcuts(ctx, 1.0),
            );
        };
        key(egui::Key::H, false);
        assert!(app.held_end.is_some());
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::H,
                    physical_key: None,
                    pressed: true,
                    repeat: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| app.shortcuts(ctx, 2.0),
        );
        assert!(
            app.held_end.is_some(),
            "held keys must not toggle repeatedly"
        );
        app.show_settings = true;
        let run = |app: &mut AnalyzerApp, key| {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![
                        egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| app.shortcuts(ctx, 3.0),
            );
        };
        run(&mut app, egui::Key::L);
        assert!(
            app.held_end.is_some(),
            "settings blocks measurement shortcuts"
        );
        run(&mut app, egui::Key::Escape);
        assert!(!app.show_settings);
        run(&mut app, egui::Key::L);
        assert!(app.held_end.is_none());
    }

    #[test]
    fn text_entry_does_not_toggle_measurement_shortcuts() {
        let mut app = AnalyzerApp::with_options(
            AnalyzerController::with_source(Unavailable).unwrap(),
            AppOptions {
                demo: true,
                ..Default::default()
            },
        );
        app.live = Some(Box::new(AnalysisSnapshot::default()));
        let ctx = egui::Context::default();
        let mut notes = String::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.text_edit_singleline(&mut notes).request_focus();
            });
        });
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::H,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Text("h".into()),
                ],
                ..Default::default()
            },
            |ctx| {
                assert!(ctx.wants_keyboard_input());
                app.shortcuts(ctx, 1.0);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.text_edit_singleline(&mut notes);
                });
            },
        );
        assert_eq!(notes, "h");
        assert!(app.held_end.is_none());
    }
    #[test]
    fn disconnected_and_failed_ui_render_at_supported_sizes() {
        for (w, h) in [(760.0, 640.0), (1440.0, 1000.0)] {
            let controller = AnalyzerController::with_source(Unavailable).unwrap();
            let mut app = AnalyzerApp::with_options(
                controller,
                AppOptions {
                    demo: true,
                    ..Default::default()
                },
            );
            let ctx = egui::Context::default();
            theme::install(&ctx);
            for failed in [false, true] {
                if failed {
                    app.connection = ConnectionState::Failed("Test failure".into());
                }
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(w, h),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        app.header(ctx, 0.0);
                        app.dashboard(ctx, 0.0);
                    },
                );
                assert!(!out.shapes.is_empty());
            }
        }
    }

    #[test]
    fn freeze_retains_values_while_live_history_advances_and_stale_time_keeps_aging() {
        let controller = AnalyzerController::with_source(Unavailable).unwrap();
        let mut app = AnalyzerApp::with_options(
            controller,
            AppOptions {
                demo: true,
                ..Default::default()
            },
        );
        let first = AnalysisSnapshot {
            sample_rate: 48000,
            processed_at: Instant::now() - Duration::from_secs(2),
            queue_lag_frames: 4800,
            measurement_level: SignalLevel {
                peak_dbfs: -6.0,
                rms_dbfs: -9.0,
            },
            ..Default::default()
        };
        app.live = Some(Box::new(first));
        assert!(app.freshness().unwrap() >= 2.1);
        app.freeze(10.0);
        let second = AnalysisSnapshot {
            sample_rate: 48000,
            window_start_frame: 4800,
            measurement_level: SignalLevel {
                peak_dbfs: -20.0,
                rms_dbfs: -23.0,
            },
            ..Default::default()
        };
        app.history.push_at(&second, 0, 10.1);
        app.live = Some(Box::new(second));
        assert_eq!(app.displayed().unwrap().measurement_level.rms_dbfs, -9.0);
        assert_eq!(app.history.points().back().unwrap().measurement_rms, -23.0);
        app.held_end = None;
        assert_eq!(app.displayed().unwrap().measurement_level.rms_dbfs, -23.0);
    }

    #[test]
    fn synthetic_ui_hold_condition_change_and_reconnect_preserve_the_right_epoch() {
        let controller =
            AnalyzerController::with_source(crate::synthetic::SyntheticSource::default()).unwrap();
        let mut app = AnalyzerApp::with_options(
            controller,
            AppOptions {
                demo: true,
                ..Default::default()
            },
        );
        app.runtime
            .start_configured(app.settings.input.clone(), AnalysisConfig::FAST)
            .unwrap();
        let began = Instant::now();
        let until = |app: &mut AnalyzerApp, ready: &dyn Fn(&AnalyzerApp) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ready(app) {
                app.poll(began.elapsed().as_secs_f64());
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        until(&mut app, &|a| a.live.is_some());
        let initial = app.live.as_ref().unwrap();
        assert!((initial.measurement_level.peak_dbfs + 12.0412).abs() < 0.05);
        assert!((initial.measurement_level.rms_dbfs + 15.0515).abs() < 0.05);
        assert!((initial.reference_level.unwrap().peak_dbfs + 6.0206).abs() < 0.05);
        assert!((initial.bins[32].measurement_dbfs + 12.0412).abs() < 0.05);
        assert!((initial.bins[16].reference_dbfs + 6.0206).abs() < 0.05);
        app.capture_reference();
        let base = app.baseline.as_ref().unwrap();
        assert!((base.measurement[59] + 12.0412).abs() < 0.05);
        assert!((base.reference.as_ref().unwrap()[50] + 6.0206).abs() < 0.05);
        base.validate().unwrap();
        app.freeze(began.elapsed().as_secs_f64());
        let held_sequence = app.displayed().unwrap().sequence;
        let held_time = app.held_sample_time;
        until(&mut app, &|a| a.history.points().len() >= 3);
        assert_eq!(app.displayed().unwrap().sequence, held_sequence);
        assert_eq!(app.held_sample_time, held_time);
        assert!(app.live.as_ref().unwrap().sequence > held_sequence);
        assert!(
            (app.history.points().back().unwrap().reference_rms.unwrap() + 9.0309).abs() < 0.05
        );
        assert!((app.history.points().back().unwrap().measurement_rms + 15.0515).abs() < 0.05);

        app.runtime.set_analysis(AnalysisConfig::PRECISE).unwrap();
        app.settings.analysis = AnalysisConfig::PRECISE;
        app.segment("QA analysis change");
        assert!(app.displayed().is_none());
        assert!(app.held_end.is_none());
        assert!(app.notice.contains("表示停止を解除"));
        until(&mut app, &|a| a.live.is_some());
        assert_eq!(app.live.as_ref().unwrap().analysis_generation, 1);
        assert_eq!(app.baseline.as_ref().unwrap().config, AnalysisConfig::FAST);
        let point = app.history.points().back().unwrap();
        assert!(point.gap);
        assert_eq!(point.config, AnalysisConfig::PRECISE);
        assert!(
            app.history
                .points()
                .iter()
                .any(|p| p.config == AnalysisConfig::FAST)
        );

        app.runtime.stop();
        assert!(app.runtime.take_latest().is_none());
        until(&mut app, &|a| a.connection == ConnectionState::Stopped);
        let previous_time = app.live_time;
        app.runtime
            .start_configured(app.settings.input.clone(), AnalysisConfig::PRECISE)
            .unwrap();
        until(&mut app, &|a| {
            a.connection == ConnectionState::Running && a.live.is_some()
        });
        let fresh = app.live.as_ref().unwrap();
        assert_eq!(fresh.analysis_generation, 0);
        assert_eq!(fresh.sequence, 0);
        assert_eq!(fresh.window_start_frame, 0);
        assert!(app.live_time > previous_time);
        assert!(app.held_end.is_none());
        assert!(app.history.points().back().unwrap().gap);
    }
}
