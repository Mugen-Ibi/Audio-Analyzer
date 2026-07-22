use std::collections::VecDeque;

use eframe::egui::{
    self, Align, Align2, Color32, Frame, Layout, Pos2, Rect, Response, RichText, Sense, Stroke, Ui,
    Vec2, pos2, vec2,
};

use crate::model::StereoFrame;

use super::{
    state::HistoryPoint,
    theme::{
        self, BACKGROUND, CYAN, GREEN, GREEN_BRIGHT, OUTLINE, RED, SURFACE, SURFACE_HIGH, TEXT,
        TEXT_MUTED, YELLOW,
    },
};

pub fn module(
    ui: &mut Ui,
    title: &str,
    accent: bool,
    min_height: f32,
    controls: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) {
    let stroke = Stroke::new(1.0, if accent { GREEN } else { OUTLINE });
    Frame::none()
        .fill(SURFACE)
        .stroke(stroke)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            ui.set_min_height(min_height);
            Frame::none()
                .fill(BACKGROUND)
                .inner_margin(egui::Margin::symmetric(12.0, 7.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(title)
                                .font(theme::bold(10.0))
                                .color(if accent { GREEN_BRIGHT } else { TEXT_MUTED }),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), controls);
                    });
                });
            ui.separator();
            Frame::none()
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, body);
        });
}

pub fn chip(ui: &mut Ui, label: &str, active: bool) -> Response {
    let text = RichText::new(label)
        .font(theme::bold(9.0))
        .color(if active { CYAN } else { TEXT_MUTED });
    let button = egui::Button::new(text)
        .fill(if active {
            Color32::from_rgba_unmultiplied(0, 219, 233, 25)
        } else {
            BACKGROUND
        })
        .stroke(Stroke::new(1.0, if active { CYAN } else { OUTLINE }))
        .rounding(0.0);
    ui.add(button)
}

pub fn metric(ui: &mut Ui, label: &str, value: Option<f32>, unit: &str, color: Color32) {
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(label)
                .font(theme::bold(9.0))
                .color(TEXT_MUTED),
        );
        let value = value.map_or_else(|| "N/A".to_owned(), |value| format!("{value:.1}"));
        ui.label(RichText::new(value).font(theme::mono(32.0)).color(color));
        ui.label(RichText::new(unit).font(theme::bold(8.0)).color(TEXT_MUTED));
    });
}

pub fn nav_item(ui: &mut Ui, glyph: &str, label: &str, selected: bool, enabled: bool) -> Response {
    let desired = vec2(ui.available_width(), 58.0);
    let (rect, response) = ui.allocate_exact_size(desired, Sense::click());
    let response = if enabled {
        response
    } else {
        response.on_disabled_hover_text("Planned for a future release")
    };
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 219, 233, 18));
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            CYAN,
        );
    }
    let color = if !enabled {
        TEXT_MUTED.gamma_multiply(0.35)
    } else if selected {
        CYAN
    } else {
        TEXT_MUTED
    };
    ui.painter().text(
        pos2(rect.center().x, rect.top() + 20.0),
        Align2::CENTER_CENTER,
        glyph,
        theme::bold(17.0),
        color,
    );
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 12.0),
        Align2::CENTER_CENTER,
        label,
        theme::bold(8.0),
        color,
    );
    response
}

pub fn level_meter(ui: &mut Ui, current_dbfs: f32, held_dbfs: f32, label: &str, color: Color32) {
    let (rect, response) = ui.allocate_exact_size(vec2(58.0, 158.0), Sense::hover());
    response.on_hover_text(format!(
        "Current: {current_dbfs:.1} dBFS\nHeld: {held_dbfs:.1} dBFS"
    ));
    let meter = Rect::from_min_max(
        pos2(rect.left() + 13.0, rect.top() + 2.0),
        pos2(rect.right() - 13.0, rect.bottom() - 22.0),
    );
    ui.painter().rect_filled(meter, 0.0, SURFACE_HIGH);
    ui.painter()
        .rect_stroke(meter, 0.0, Stroke::new(1.0, OUTLINE));

    let normalized = db_normalized(current_dbfs);
    let segments = 24;
    for segment in 0..segments {
        let segment_low = segment as f32 / segments as f32;
        if segment_low > normalized {
            continue;
        }
        let bottom = egui::lerp(meter.bottom()..=meter.top(), segment_low);
        let top = egui::lerp(
            meter.bottom()..=meter.top(),
            (segment as f32 + 0.72) / segments as f32,
        );
        let segment_rect = Rect::from_min_max(
            pos2(meter.left() + 2.0, top),
            pos2(meter.right() - 2.0, bottom),
        );
        let segment_color = if segment_low > 0.98 {
            RED
        } else if segment_low > 0.9 {
            YELLOW
        } else {
            color
        };
        ui.painter().rect_filled(segment_rect, 0.0, segment_color);
    }

    let held_y = egui::lerp(meter.bottom()..=meter.top(), db_normalized(held_dbfs));
    ui.painter().line_segment(
        [pos2(meter.left(), held_y), pos2(meter.right(), held_y)],
        Stroke::new(2.0, TEXT),
    );
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_CENTER,
        label,
        theme::bold(9.0),
        TEXT,
    );
}

pub fn vectorscope(ui: &mut Ui, points: &[StereoFrame], correlation: Option<f32>) {
    let available = ui.available_size();
    let side = available.x.min(available.y - 30.0).max(80.0);
    let (rect, _) = ui.allocate_exact_size(vec2(available.x, side + 28.0), Sense::hover());
    let scope = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + side / 2.0),
        Vec2::splat(side),
    );
    let radius = side * 0.45;
    ui.painter()
        .circle_stroke(scope.center(), radius, Stroke::new(1.0, OUTLINE));
    ui.painter().line_segment(
        [
            pos2(scope.center().x - radius, scope.center().y),
            pos2(scope.center().x + radius, scope.center().y),
        ],
        Stroke::new(1.0, OUTLINE.gamma_multiply(0.5)),
    );
    ui.painter().line_segment(
        [
            pos2(scope.center().x, scope.center().y - radius),
            pos2(scope.center().x, scope.center().y + radius),
        ],
        Stroke::new(1.0, OUTLINE.gamma_multiply(0.5)),
    );
    for point in points {
        let x = point.reference.clamp(-1.0, 1.0);
        let y = point.measurement.clamp(-1.0, 1.0);
        ui.painter().circle_filled(
            pos2(scope.center().x + x * radius, scope.center().y - y * radius),
            1.25,
            CYAN.gamma_multiply(0.55),
        );
    }

    let bar = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.bottom() - 12.0),
        pos2(rect.right() - 8.0, rect.bottom() - 8.0),
    );
    ui.painter().rect_filled(bar, 0.0, SURFACE_HIGH);
    let center_x = bar.center().x;
    ui.painter().line_segment(
        [
            pos2(center_x, bar.top() - 3.0),
            pos2(center_x, bar.bottom() + 3.0),
        ],
        Stroke::new(1.0, TEXT),
    );
    if let Some(correlation) = correlation {
        let x = egui::lerp(bar.left()..=bar.right(), (correlation + 1.0) * 0.5);
        ui.painter().line_segment(
            [pos2(x - 9.0, bar.center().y), pos2(x + 9.0, bar.center().y)],
            Stroke::new(4.0, GREEN),
        );
    }
}

pub fn history_plot(ui: &mut Ui, points: &VecDeque<HistoryPoint>, height: f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    response.on_hover_text("300 second RMS history at 10 Hz");
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);
    for step in 0..=4 {
        let y = egui::lerp(rect.top()..=rect.bottom(), step as f32 / 4.0);
        ui.painter().line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, OUTLINE.gamma_multiply(0.25)),
        );
    }
    let Some(last) = points.back() else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "WAITING FOR HISTORY DATA",
            theme::bold(9.0),
            TEXT_MUTED,
        );
        return;
    };
    let end = last.seconds;
    let start = (end - 300.0).max(0.0);
    let map = |seconds: f64, db: f32| {
        pos2(
            egui::remap_clamp(
                seconds as f32,
                start as f32..=end.max(start + 0.1) as f32,
                rect.x_range(),
            ),
            egui::remap_clamp(db, -60.0..=0.0, rect.bottom()..=rect.top()),
        )
    };
    let mut previous_measurement: Option<Pos2> = None;
    let mut previous_reference: Option<Pos2> = None;
    for point in points.iter().filter(|point| point.seconds >= start) {
        if point.gap {
            previous_measurement = None;
            previous_reference = None;
        }
        let measurement = map(point.seconds, point.measurement_rms);
        if let Some(previous) = previous_measurement {
            ui.painter()
                .line_segment([previous, measurement], Stroke::new(1.5, CYAN));
        }
        previous_measurement = Some(measurement);
        if let Some(reference_db) = point.reference_rms {
            let reference = map(point.seconds, reference_db);
            if let Some(previous) = previous_reference {
                ui.painter().line_segment(
                    [previous, reference],
                    Stroke::new(1.0, GREEN.gamma_multiply(0.55)),
                );
            }
            previous_reference = Some(reference);
        }
    }
    for (db, align) in [
        (0, Align2::RIGHT_TOP),
        (-30, Align2::RIGHT_CENTER),
        (-60, Align2::RIGHT_BOTTOM),
    ] {
        let y = egui::remap_clamp(db as f32, -60.0..=0.0, rect.bottom()..=rect.top());
        ui.painter().text(
            pos2(rect.right() - 4.0, y),
            align,
            format!("{db} dB"),
            theme::mono(8.0),
            TEXT_MUTED,
        );
    }
}

fn db_normalized(dbfs: f32) -> f32 {
    ((dbfs.clamp(-60.0, 0.0) + 60.0) / 60.0).clamp(0.0, 1.0)
}
