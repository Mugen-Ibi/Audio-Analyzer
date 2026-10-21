//! Native egui meshes: one frequency/level/time chart, with a matching 2D view.
use super::theme::{self, CYAN, GREEN, OUTLINE, RED, TEXT_MUTED};
use crate::{
    history::{HISTORY_SPECTRUM_BANDS, HistoryPoint},
    visualizer::{self, Camera, SpectrumReference, ViewMode},
};
use eframe::egui::{
    self, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, TextureOptions, Ui,
    Vec2, pos2, vec2,
};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct Pick {
    pos: Pos2,
    seconds: f64,
    band: usize,
    distance: f32,
}
#[derive(Clone, Copy)]
struct Projection {
    camera: Camera,
    rect: Rect,
    center: Pos2,
    scale: Vec2,
}
impl Projection {
    fn new(camera: Camera, rect: Rect) -> Self {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for x in [-1.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [0.0, 1.25] {
                    let q = camera.project([x, y, z]);
                    for i in 0..2 {
                        lo[i] = lo[i].min(q[i]);
                        hi[i] = hi[i].max(q[i]);
                    }
                }
            }
        }
        // Axis units are independent (Hz, dBFS, seconds); fit the fixed world box,
        // never fit data amplitudes. Wide displays retain readable frequency spacing.
        let scale = vec2(
            rect.width() / (hi[0] - lo[0]),
            rect.height() / (hi[1] - lo[1]),
        ) * camera.zoom;
        let center = rect.center()
            + vec2(
                -(hi[0] + lo[0]) * 0.5 * scale.x,
                (hi[1] + lo[1]) * 0.5 * scale.y,
            );
        Self {
            camera,
            rect,
            center,
            scale,
        }
    }
    fn at(self, x: f32, y: f32, z: f32) -> Pos2 {
        let p = self.camera.project([x, y, z]);
        self.center + vec2(p[0] * self.scale.x, -p[1] * self.scale.y)
    }
}
fn line(mesh: &mut egui::Mesh, a: Pos2, b: Pos2, width: f32, color: Color32) {
    let d = b - a;
    if d.length_sq() < 0.001 {
        return;
    }
    let n = vec2(-d.y, d.x).normalized() * width * 0.5;
    let i = mesh.vertices.len() as u32;
    for p in [a - n, a + n, b + n, b - n] {
        mesh.colored_vertex(p, color);
    }
    mesh.indices
        .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
}
#[allow(clippy::too_many_arguments)] // Geometry and visual style of one independent time slice.
fn profile(
    mesh: &mut egui::Mesh,
    project: Projection,
    bands: &[f32],
    z: f32,
    minimum: f32,
    color: Color32,
    fill: bool,
    dashed: bool,
) {
    let mut last = None;
    for (band, db) in bands.iter().take(HISTORY_SPECTRUM_BANDS).enumerate() {
        let x = band as f32 / (HISTORY_SPECTRUM_BANDS - 1) as f32 * 2.0 - 1.0;
        let p = project.at(x, visualizer::level_height(*db, minimum), z);
        let above = *db > minimum.max(-120.0) + 0.01;
        if let Some((a, base, previous_above)) = last {
            let bottom = project.at(x, 0.0, z);
            if fill && (above || previous_above) {
                let i = mesh.vertices.len() as u32;
                for v in [a, p, bottom, base] {
                    mesh.colored_vertex(v, color.gamma_multiply(0.12));
                }
                mesh.indices
                    .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
            }
            if (above || previous_above) && (!dashed || band % 3 != 0) {
                line(mesh, a, p, if dashed { 1.5 } else { 1.0 }, color);
            }
        }
        last = Some((p, project.at(x, 0.0, z), above));
    }
}

#[derive(Default)]
pub struct Waterfall {
    key: Vec<u64>,
    mesh: Option<egui::Mesh>,
    picks: Vec<Pick>,
    textures: [Option<TextureHandle>; 2],
    rows: Vec<Option<usize>>,
    pub rebuild_ms: f64,
    pub vertices: usize,
}
pub struct View<'a> {
    pub points: &'a VecDeque<HistoryPoint>,
    pub start: f64,
    pub end: f64,
    pub minimum: f32,
    pub channels: [bool; 2],
    pub mode: ViewMode,
    pub live: Option<&'a HistoryPoint>,
    pub baseline: Option<&'a SpectrumReference>,
}
impl Waterfall {
    pub fn invalidate(&mut self) {
        self.key.clear();
    }
    pub fn show(
        &mut self,
        ui: &mut Ui,
        view: View<'_>,
        camera: &mut Camera,
        selected: &mut Option<f64>,
        band: &mut usize,
        height: f32,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width().max(1.0), height.max(120.0)),
            Sense::click_and_drag(),
        );
        if rect.width() < 10.0 || !ui.is_rect_visible(rect) {
            return;
        }
        ui.painter().rect_filled(rect, 5.0, theme::SURFACE);
        if view.mode == ViewMode::Waterfall {
            if response.dragged_by(egui::PointerButton::Primary) {
                let d = ui.input(|i| i.pointer.delta());
                camera.orbit(d.x, d.y);
            }
            if response.double_clicked() {
                *camera = Camera::default();
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    camera.zoom = (camera.zoom * (scroll * 0.002).exp()).clamp(0.65, 2.0);
                    ui.input_mut(|i| {
                        i.smooth_scroll_delta = Vec2::ZERO;
                        i.raw_scroll_delta = Vec2::ZERO;
                    });
                }
            }
        }
        let graph = Rect::from_min_max(rect.min + vec2(64.0, 24.0), rect.max - vec2(86.0, 47.0));
        if graph.width() <= 1.0 || graph.height() <= 1.0 {
            return;
        }
        let project = Projection::new(*camera, graph);
        let end = view.end;
        let start = view.start;
        let key = vec![
            view.points.front().map_or(0, |p| p.seconds.to_bits()),
            view.points
                .iter()
                .rev()
                .find(|p| p.seconds <= end)
                .map_or(0, |p| p.seconds.to_bits()),
            start.to_bits(),
            end.to_bits(),
            view.minimum.to_bits() as u64,
            camera.yaw.to_bits() as u64,
            camera.pitch.to_bits() as u64,
            camera.zoom.to_bits() as u64,
            rect.left().to_bits() as u64,
            rect.top().to_bits() as u64,
            rect.width().to_bits() as u64,
            rect.height().to_bits() as u64,
            view.channels[0] as u64,
            view.channels[1] as u64,
            view.mode as u64,
        ];
        if self.key != key {
            let began = std::time::Instant::now();
            self.picks.clear();
            if view.mode == ViewMode::Waterfall {
                let mut mesh = egui::Mesh::default();
                for index in visualizer::slice_indices(view.points, start, end) {
                    let p = &view.points[index];
                    let z = visualizer::depth(p.seconds, start, end);
                    for (enabled, bands, color) in [
                        (view.channels[0], Some(&p.spectrum_dbfs), CYAN),
                        (view.channels[1], p.reference_spectrum_dbfs.as_ref(), GREEN),
                    ] {
                        if !enabled {
                            continue;
                        }
                        let Some(bands) = bands else {
                            continue;
                        };
                        profile(
                            &mut mesh,
                            project,
                            bands,
                            z,
                            view.minimum,
                            color.gamma_multiply(0.75),
                            true,
                            false,
                        );
                        for (band, db) in bands.iter().enumerate() {
                            let x = band as f32 / 95.0 * 2.0 - 1.0;
                            self.picks.push(Pick {
                                pos: project.at(x, visualizer::level_height(*db, view.minimum), z),
                                seconds: p.seconds,
                                band,
                                distance: z,
                            });
                        }
                    }
                }
                self.vertices = mesh.vertices.len();
                self.mesh = Some(mesh);
            } else {
                let h = (graph.height().round() as usize).clamp(32, 512);
                self.rows = visualizer::sample_indices(view.points, start, end, h);
                for channel in 0..2 {
                    let mut image = egui::ColorImage::new([96, h], theme::BACKGROUND);
                    for (row, index) in self.rows.iter().enumerate() {
                        if let Some(index) = index {
                            let p = &view.points[*index];
                            let bands = if channel == 0 {
                                Some(&p.spectrum_dbfs)
                            } else {
                                p.reference_spectrum_dbfs.as_ref()
                            };
                            if let Some(bands) = bands {
                                for (b, db) in bands.iter().enumerate() {
                                    image[(b, row)] = super::widgets::spectrum_color(
                                        visualizer::level_height(*db, view.minimum) * 120.0 - 120.0,
                                    );
                                }
                            }
                        }
                    }
                    if let Some(t) = &mut self.textures[channel] {
                        t.set(image, TextureOptions::NEAREST);
                    } else {
                        self.textures[channel] = Some(ui.ctx().load_texture(
                            format!("visualizer_heatmap_{channel}"),
                            image,
                            TextureOptions::NEAREST,
                        ));
                    }
                }
                self.vertices = 0;
            }
            self.rebuild_ms = began.elapsed().as_secs_f64() * 1000.0;
            self.key = key;
        }
        let painter = ui.painter().with_clip_rect(rect);
        if view.mode == ViewMode::Waterfall {
            self.axes(&painter, project, view.minimum, start, end);
            if let Some(mesh) = &self.mesh {
                // egui 0.30 owns Mesh; projection is cached, submission copies its buffer.
                painter.add(Shape::Mesh(mesh.clone()));
            }
            let mut overlay = egui::Mesh::default();
            if let Some(p) = view
                .live
                .filter(|p| !p.gap && p.seconds >= start && p.seconds <= view.end + 0.1)
            {
                let z = visualizer::depth(p.seconds, start, view.end);
                if view.channels[0] {
                    profile(
                        &mut overlay,
                        project,
                        &p.spectrum_dbfs,
                        z,
                        view.minimum,
                        CYAN,
                        false,
                        false,
                    );
                }
                if view.channels[1]
                    && let Some(b) = p.reference_spectrum_dbfs.as_ref()
                {
                    profile(
                        &mut overlay,
                        project,
                        b,
                        z,
                        view.minimum,
                        GREEN,
                        false,
                        false,
                    );
                }
            }
            if let Some(base) = view.baseline {
                if view.channels[0] {
                    profile(
                        &mut overlay,
                        project,
                        &base.measurement,
                        0.0,
                        view.minimum,
                        CYAN,
                        false,
                        true,
                    );
                }
                if view.channels[1]
                    && let Some(b) = base.reference.as_ref()
                {
                    profile(
                        &mut overlay,
                        project,
                        b,
                        0.0,
                        view.minimum,
                        GREEN,
                        false,
                        true,
                    );
                }
            }
            if let Some(time) = selected
                && *time >= start
                && *time <= end
                && let Some(p) = view
                    .points
                    .iter()
                    .enumerate()
                    .find(|(i, p)| {
                        !p.gap
                            && *time >= p.seconds
                            && *time
                                < crate::history::history_interval_end(p, view.points.get(i + 1))
                    })
                    .map(|(_, p)| p)
            {
                let z = visualizer::depth(p.seconds, start, end);
                let x = *band as f32 / 95.0 * 2.0 - 1.0;
                for (show, bands, color) in [
                    (view.channels[0], Some(&p.spectrum_dbfs), CYAN),
                    (view.channels[1], p.reference_spectrum_dbfs.as_ref(), GREEN),
                ] {
                    if show && let Some(b) = bands {
                        let pt = project.at(x, visualizer::level_height(b[*band], view.minimum), z);
                        painter.circle_stroke(pt, 4.0, Stroke::new(1.5_f32, color));
                        line(&mut overlay, project.at(x, 0.0, z), pt, 0.8, Color32::WHITE);
                    }
                }
            }
            painter.add(Shape::mesh(overlay));
            if response.hovered()
                && !response.dragged()
                && let Some(pos) = response.hover_pos()
            {
                let picked = self
                    .picks
                    .iter()
                    .filter(|p| p.pos.distance_sq(pos) < 100.0)
                    .min_by(|a, b| {
                        a.pos
                            .distance_sq(pos)
                            .total_cmp(&b.pos.distance_sq(pos))
                            .then(a.distance.total_cmp(&b.distance))
                    });
                if let Some(p) = picked {
                    *selected = Some(p.seconds);
                    *band = p.band;
                }
            }
            for p in view.points.iter().filter(|p| {
                p.seconds >= start
                    && p.seconds <= end
                    && (p.gap || p.measurement_clipped || p.reference_clipped)
            }) {
                let z = visualizer::depth(p.seconds, start, end);
                painter.line_segment(
                    [project.at(-1.0, 0.0, z), project.at(-0.94, 0.0, z)],
                    Stroke::new(2.0_f32, RED),
                );
            }
        } else {
            let enabled: Vec<_> = (0..2).filter(|i| view.channels[*i]).collect();
            for (slot, channel) in enabled.iter().copied().enumerate() {
                let w = (graph.width() - 16.0 * (enabled.len().saturating_sub(1)) as f32)
                    / enabled.len() as f32;
                let panel = Rect::from_min_size(
                    graph.min + vec2(slot as f32 * (w + 16.0), 0.0),
                    vec2(w, graph.height()),
                );
                if let Some(texture) = &self.textures[channel] {
                    painter.image(
                        texture.id(),
                        panel,
                        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                painter.text(
                    panel.left_top() - vec2(0.0, 8.0),
                    Align2::LEFT_BOTTOM,
                    if channel == 0 {
                        "青入力"
                    } else {
                        "緑入力"
                    },
                    theme::mono(12.0),
                    if channel == 0 { CYAN } else { GREEN },
                );
                for hz in [20.0_f64, 100.0, 1000.0, 10000.0, 20000.0] {
                    let x =
                        panel.left() + ((hz / 20.0).ln() / 1000.0_f64.ln()) as f32 * panel.width();
                    painter.text(
                        pos2(x, panel.bottom() + 8.0),
                        if hz == 20.0 {
                            Align2::LEFT_TOP
                        } else if hz == 20000.0 {
                            Align2::RIGHT_TOP
                        } else {
                            Align2::CENTER_TOP
                        },
                        format_hz(hz),
                        theme::mono(10.0),
                        TEXT_MUTED,
                    );
                }
                if response.hovered()
                    && let Some(pos) = response.hover_pos().filter(|p| panel.contains(*p))
                {
                    let row = (((pos.y - panel.top()) / panel.height()) * self.rows.len() as f32)
                        .floor()
                        .clamp(0.0, (self.rows.len().saturating_sub(1)) as f32)
                        as usize;
                    *selected = self
                        .rows
                        .get(row)
                        .and_then(|v| *v)
                        .map(|i| view.points[i].seconds)
                        .or(Some(
                            start + (end - start) * ((pos.y - panel.top()) / panel.height()) as f64,
                        ));
                    *band = (((pos.x - panel.left()) / panel.width()) * 95.0)
                        .round()
                        .clamp(0.0, 95.0) as usize;
                }
                if let Some(t) = selected.filter(|t| *t >= start && *t <= end) {
                    let y = panel.top() + ((t - start) / (end - start)) as f32 * panel.height();
                    painter.line_segment(
                        [pos2(panel.left(), y), pos2(panel.right(), y)],
                        Stroke::new(1.0_f32, Color32::WHITE),
                    );
                }
                for p in view.points.iter().filter(|p| {
                    p.seconds >= start
                        && p.seconds <= end
                        && (p.gap || p.measurement_clipped || p.reference_clipped)
                }) {
                    let y =
                        panel.top() + ((p.seconds - start) / (end - start)) as f32 * panel.height();
                    painter.line_segment(
                        [pos2(panel.left(), y), pos2(panel.left() + 6.0, y)],
                        Stroke::new(2.0_f32, RED),
                    );
                }
            }
            for (f, label) in [(0.0, start), (0.5, (start + end) * 0.5), (1.0, end)] {
                painter.text(
                    pos2(graph.left() - 8.0, graph.top() + f * graph.height()),
                    Align2::RIGHT_CENTER,
                    format!("{label:.1}s"),
                    theme::mono(11.0),
                    TEXT_MUTED,
                );
            }
            painter.text(
                pos2(graph.center().x, rect.bottom() - 3.0),
                Align2::CENTER_BOTTOM,
                "周波数 · Hz（対数） / 下が最新 / 色は帯域レベル dBFS",
                theme::mono(11.0),
                TEXT_MUTED,
            );
        }
        if view.points.is_empty() {
            painter.text(
                graph.center(),
                Align2::CENTER_CENTER,
                "入力待ち · 合成デモまたは入力開始後に履歴を表示",
                theme::mono(13.0),
                TEXT_MUTED,
            );
        }
        if !view.channels.iter().any(|v| *v) {
            painter.text(
                graph.center(),
                Align2::CENTER_CENTER,
                "表示する入力を選択してください",
                theme::mono(13.0),
                TEXT_MUTED,
            );
        }
    }
    fn axes(&self, p: &egui::Painter, pr: Projection, minimum: f32, start: f64, end: f64) {
        let grid = Stroke::new(0.8_f32, OUTLINE.gamma_multiply(0.7));
        for hz in [
            20.0_f64, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
        ] {
            let x = ((hz / 20.0).ln() / 1000.0_f64.ln()) as f32 * 2.0 - 1.0;
            p.line_segment([pr.at(x, 0.0, 0.0), pr.at(x, 0.0, 1.25)], grid);
            if pr.rect.width() > 700.0 || [20.0, 100.0, 1000.0, 10000.0, 20000.0].contains(&hz) {
                p.text(
                    pr.at(x, 0.0, 0.0) + vec2(0.0, 8.0),
                    Align2::CENTER_TOP,
                    format_hz(hz),
                    theme::mono(11.0),
                    TEXT_MUTED,
                );
            }
        }
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let z = frac * 1.25;
            p.line_segment([pr.at(-1.0, 0.0, z), pr.at(1.0, 0.0, z)], grid);
            p.text(
                pr.at(1.0, 0.0, z) + vec2(8.0, 0.0),
                Align2::LEFT_CENTER,
                format!("{:.1}s", end - (end - start) * frac as f64),
                theme::mono(11.0),
                TEXT_MUTED,
            );
        }
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            p.line_segment([pr.at(-1.0, frac, 0.0), pr.at(-1.0, frac, 1.25)], grid);
            p.text(
                pr.at(-1.0, frac, 0.0) - vec2(8.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{:.0}", minimum * (1.0 - frac)),
                theme::mono(11.0),
                TEXT_MUTED,
            );
        }
        p.line_segment(
            [pr.at(-1.0, 0.0, 0.0), pr.at(-1.0, 1.0, 0.0)],
            Stroke::new(1.2_f32, TEXT_MUTED),
        );
        p.text(
            pr.at(-1.0, 1.0, 0.0) - vec2(5.0, 10.0),
            Align2::RIGHT_BOTTOM,
            "帯域 dBFS",
            theme::mono(11.0),
            TEXT_MUTED,
        );
        p.text(
            pr.at(0.0, 0.0, 0.0) + vec2(0.0, 29.0),
            Align2::CENTER_TOP,
            "対数周波数 Hz · 手前が最新",
            theme::mono(11.0),
            TEXT_MUTED,
        );
        p.text(
            pr.at(1.0, 0.0, 1.25) + vec2(8.0, -12.0),
            Align2::LEFT_BOTTOM,
            "過去",
            theme::mono(11.0),
            TEXT_MUTED,
        );
    }
}
fn format_hz(hz: f64) -> String {
    if hz >= 1000.0 {
        format!("{}k", hz / 1000.0)
    } else {
        format!("{hz:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        chart: Waterfall,
        points: VecDeque<HistoryPoint>,
        camera: Camera,
        selected: Option<f64>,
        band: usize,
        channels: [bool; 2],
    }
    impl Fixture {
        fn new() -> Self {
            let points = (0..6000)
                .map(|i| {
                    let mut p = HistoryPoint {
                        seconds: i as f64 * 0.1,
                        measurement_rms: -15.0,
                        reference_rms: Some(-9.0),
                        spectrum_dbfs: [-110.0; 96],
                        reference_spectrum_dbfs: Some([-110.0; 96]),
                        gap: (2000..2010).contains(&i),
                        segment: 0,
                        config: crate::config::AnalysisConfig::FAST,
                        sample_rate: 48000,
                        measurement_clipped: false,
                        reference_clipped: false,
                    };
                    p.spectrum_dbfs[59] = -12.0;
                    p.reference_spectrum_dbfs.as_mut().unwrap()[50] = -6.0;
                    p
                })
                .collect();
            Self {
                chart: Waterfall::default(),
                points,
                camera: Camera::default(),
                selected: None,
                band: 0,
                channels: [true, true],
            }
        }
        fn frame(
            &mut self,
            ctx: &egui::Context,
            mode: ViewMode,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 700.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        self.chart.show(
                            ui,
                            View {
                                points: &self.points,
                                start: 0.0,
                                end: 600.0,
                                minimum: -120.0,
                                channels: self.channels,
                                mode,
                                live: None,
                                baseline: None,
                            },
                            &mut self.camera,
                            &mut self.selected,
                            &mut self.band,
                            400.0,
                        )
                    });
                },
            )
        }
    }
    #[test]
    fn full_retention_geometry_is_bounded_cached_finite_and_input_toggle_is_real() {
        let ctx = egui::Context::default();
        let mut f = Fixture::new();
        let out = f.frame(&ctx, ViewMode::Waterfall, vec![]);
        assert!(f.chart.vertices <= 64 * 2 * 95 * 8);
        let first_count = f.chart.vertices;
        let first_ms = f.chart.rebuild_ms;
        assert!(first_count > 50000);
        for primitive in ctx.tessellate(out.shapes, out.pixels_per_point) {
            if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                assert!(
                    mesh.vertices
                        .iter()
                        .all(|v| v.pos.x.is_finite() && v.pos.y.is_finite())
                );
                assert!(mesh.is_valid());
            }
        }
        let _ = f.frame(&ctx, ViewMode::Waterfall, vec![]);
        assert_eq!(f.chart.rebuild_ms, first_ms);
        f.channels[1] = false;
        let _ = f.frame(&ctx, ViewMode::Waterfall, vec![]);
        assert_eq!(f.chart.vertices, first_count / 2);
        assert!(
            f.chart
                .picks
                .iter()
                .all(|p| p.seconds < 200.0 || p.seconds >= 201.0)
        );
        let pick = *f
            .chart
            .picks
            .iter()
            .find(|p| p.band == 59 && p.seconds > 300.0)
            .unwrap();
        let _ = f.frame(
            &ctx,
            ViewMode::Waterfall,
            vec![egui::Event::PointerMoved(pick.pos)],
        );
        assert_eq!(f.band, 59);
        assert!(f.selected.is_some());
        println!(
            "Visualizer 6000 records / 2 inputs: initial projection {first_ms:.3} ms, {first_count} vertices; CPU only"
        );
    }
    #[test]
    fn pointer_drag_scroll_and_double_click_control_only_camera_and_2d_uses_original_rows() {
        let ctx = egui::Context::default();
        let mut f = Fixture::new();
        let at = pos2(500.0, 200.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = f.frame(
            &ctx,
            ViewMode::Waterfall,
            vec![egui::Event::PointerMoved(at)],
        );
        let _ = f.frame(
            &ctx,
            ViewMode::Waterfall,
            vec![egui::Event::PointerMoved(at), button(at, true)],
        );
        let to = at + vec2(80.0, 25.0);
        let _ = f.frame(
            &ctx,
            ViewMode::Waterfall,
            vec![egui::Event::PointerMoved(to)],
        );
        assert_ne!(f.camera, Camera::default());
        let _ = f.frame(&ctx, ViewMode::Waterfall, vec![button(to, false)]);
        let before = f.camera.zoom;
        let _ = f.frame(
            &ctx,
            ViewMode::Waterfall,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 50.0),
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(f.camera.zoom > before);
        // A full click twice at the same coordinates sends egui's double-click.
        for _ in 0..2 {
            let _ = f.frame(&ctx, ViewMode::Waterfall, vec![button(to, true)]);
            let _ = f.frame(&ctx, ViewMode::Waterfall, vec![button(to, false)]);
        }
        assert_eq!(f.camera, Camera::default());
        let _ = f.frame(&ctx, ViewMode::Heatmap, vec![egui::Event::PointerMoved(at)]);
        assert!(f.selected.is_some());
        assert!(!f.chart.rows.is_empty());
        assert!(f.chart.rows.iter().flatten().all(|&i| !f.points[i].gap));
        assert_eq!(f.points.len(), 6000);
        assert_eq!(f.camera, Camera::default());
    }
}
