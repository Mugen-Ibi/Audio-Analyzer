//! Shared time axes and gap-aware auxiliary total RMS history.
use super::theme::{self, BACKGROUND, CYAN, GREEN, OUTLINE, RED, TEXT_MUTED};
use crate::history::{HistoryPoint, HistoryViewport, history_interval_end};
use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::collections::VecDeque;

fn graph_rect(rect: Rect) -> Rect {
    Rect::from_min_max(
        pos2(rect.left() + 52.0, rect.top() + 8.0),
        pos2(rect.right() - 12.0, rect.bottom() - 23.0),
    )
}
fn cursor(ui: &Ui, graph: Rect, start: f64, end: f64, selected: Option<f64>) {
    if let Some(time) = selected.filter(|time| *time >= start && *time <= end) {
        let x = egui::remap(
            time,
            start..=end,
            graph.left() as f64..=graph.right() as f64,
        ) as f32;
        ui.painter().line_segment(
            [pos2(x, graph.top()), pos2(x, graph.bottom())],
            Stroke::new(1.0_f32, Color32::WHITE),
        );
    }
}
fn time_axis(ui: &Ui, graph: Rect, start: f64, end: f64) {
    let fractions: &[f32] = if graph.width() > 850.0 {
        &[0.0, 0.25, 0.5, 0.75, 1.0]
    } else {
        &[0.0, 0.5, 1.0]
    };
    for &fraction in fractions {
        ui.painter().text(
            pos2(
                egui::lerp(graph.left()..=graph.right(), fraction),
                graph.bottom() + 5.0,
            ),
            if fraction == 0.0 {
                Align2::LEFT_TOP
            } else if fraction == 1.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::CENTER_TOP
            },
            format!("{:.1} s", start + (end - start) * fraction as f64),
            theme::mono(11.0),
            TEXT_MUTED,
        );
    }
}
pub fn history_plot(
    ui: &mut Ui,
    points: &VecDeque<HistoryPoint>,
    viewport: HistoryViewport,
    end: f64,
    selected: &mut Option<f64>,
    height: f32,
    channels: [bool; 2],
) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let graph = graph_rect(rect);
    let (start, end) = viewport.time_bounds(end);
    ui.painter().rect_filled(graph, 0.0, BACKGROUND);
    for db in [-120, -90, -60, -30, 0] {
        let y = egui::remap(db as f32, -120.0..=0.0, graph.bottom()..=graph.top());
        ui.painter().line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(1.0_f32, OUTLINE),
        );
        ui.painter().text(
            pos2(graph.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            db.to_string(),
            theme::mono(11.0),
            TEXT_MUTED,
        );
    }
    let map = |p: &HistoryPoint, db: f32| {
        pos2(
            egui::remap(
                p.seconds,
                start..=end,
                graph.left() as f64..=graph.right() as f64,
            ) as f32,
            egui::remap_clamp(db, -120.0..=0.0, graph.bottom()..=graph.top()),
        )
    };
    let mut prev: Option<&HistoryPoint> = None;
    for point in points
        .iter()
        .filter(|p| p.seconds >= start && p.seconds <= end)
    {
        if let Some(previous) = prev
            && !point.gap
            && !previous.gap
            && point.segment == previous.segment
            && history_interval_end(previous, Some(point)) >= point.seconds
        {
            if channels[0] {
                ui.painter().line_segment(
                    [
                        map(previous, previous.measurement_rms),
                        map(point, point.measurement_rms),
                    ],
                    Stroke::new(1.5_f32, CYAN),
                );
            }
            if channels[1]
                && let (Some(a), Some(b)) = (previous.reference_rms, point.reference_rms)
            {
                ui.painter().line_segment(
                    [map(previous, a), map(point, b)],
                    Stroke::new(1.5_f32, GREEN),
                );
            }
        }
        if point.gap || point.measurement_clipped || point.reference_clipped {
            let x = map(point, -60.0).x;
            ui.painter().line_segment(
                [pos2(x, graph.top()), pos2(x, graph.top() + 6.0)],
                Stroke::new(2.0_f32, RED),
            );
        }
        prev = Some(point);
    }
    if response.hovered()
        && let Some(pos) = response.hover_pos().filter(|p| graph.contains(*p))
    {
        *selected = Some(egui::remap(
            pos.x as f64,
            graph.left() as f64..=graph.right() as f64,
            start..=end,
        ));
    }
    cursor(ui, graph, start, end, *selected);
    time_axis(ui, graph, start, end);
}

pub(super) fn spectrum_color(db: f32) -> Color32 {
    if db <= -119.0 {
        return BACKGROUND;
    }
    let value = ((db + 120.0) / 120.0).clamp(0.0, 1.0);
    if value < 0.5 {
        Color32::from_rgb(8, (value * 200.0) as u8, (30.0 + value * 400.0) as u8)
    } else {
        Color32::from_rgb(
            ((value - 0.5) * 510.0) as u8,
            (100.0 + value * 140.0) as u8,
            ((1.0 - value) * 450.0) as u8,
        )
    }
}

/// A fixed, explicitly labelled digital scale. A thin white tick is sample peak.
pub fn level_meter(ui: &mut Ui, rms: f32, peak: f32, color: Color32, ticks: bool) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), if ticks { 29.0 } else { 10.0 }),
        Sense::hover(),
    );
    let bar = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 8.0));
    ui.painter().rect_filled(bar, 2.0, theme::SURFACE_LOW);
    let x = |db| egui::remap_clamp(db, -60.0..=0.0, bar.left()..=bar.right());
    if rms > -60.0 {
        ui.painter().rect_filled(
            Rect::from_min_max(bar.min, pos2(x(rms), bar.bottom())),
            2.0,
            color,
        );
    }
    if peak > -60.0 {
        ui.painter().line_segment(
            [
                pos2(x(peak), bar.top() - 1.0),
                pos2(x(peak), bar.bottom() + 1.0),
            ],
            Stroke::new(1.5_f32, Color32::WHITE),
        );
    }
    if ticks {
        for db in [-60.0, -30.0, -12.0, 0.0] {
            ui.painter().text(
                pos2(x(db), bar.bottom() + 4.0),
                if db == -60.0 {
                    Align2::LEFT_TOP
                } else if db == 0.0 {
                    Align2::RIGHT_TOP
                } else {
                    Align2::CENTER_TOP
                },
                format!("{db:.0}"),
                theme::mono(10.0),
                TEXT_MUTED,
            );
        }
    }
}

pub fn color_scale(ui: &mut Ui, minimum: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(185.0, 18.0), Sense::hover());
    let ramp = Rect::from_min_size(pos2(rect.left() + 42.0, rect.top() + 5.0), vec2(94.0, 7.0));
    for i in 0..48 {
        let left = ramp.left() + ramp.width() * i as f32 / 48.0;
        let right = ramp.left() + ramp.width() * (i + 1) as f32 / 48.0;
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(left, ramp.top()), pos2(right, ramp.bottom())),
            0.0,
            spectrum_color(-120.0 + 120.0 * i as f32 / 47.0),
        );
    }
    ui.painter().text(
        rect.left_top(),
        Align2::LEFT_TOP,
        format!("{minimum:.0}"),
        theme::mono(10.0),
        TEXT_MUTED,
    );
    ui.painter().text(
        pos2(ramp.right() + 7.0, rect.top()),
        Align2::LEFT_TOP,
        "0 dBFS",
        theme::mono(10.0),
        TEXT_MUTED,
    );
    response.on_hover_text(
        "共通の固定レンジです。暗い背景は下限以下・FFTビンなし・未記録を含みます。欠測は赤印と履歴カーソルで確認できます。96対数帯域ごとの最大FFTビン振幅です。",
    );
}
