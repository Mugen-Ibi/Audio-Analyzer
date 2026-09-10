use std::collections::VecDeque;

use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, Frame, Layout, Pos2, Rect, Response, RichText, Sense,
    Stroke, TextureHandle, Ui, Vec2, pos2, vec2,
};

use crate::model::StereoFrame;

use super::{
    state::{HISTORY_SPECTRUM_BANDS, HistoryPoint},
    theme::{
        self, BACKGROUND, CYAN, GREEN, GREEN_BRIGHT, OUTLINE, RED, SURFACE, SURFACE_HIGH,
        SURFACE_LOW, TEXT, TEXT_MUTED, YELLOW,
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
    let stroke = Stroke::new(1.0, if accent { CYAN } else { OUTLINE });
    Frame::none()
        .fill(SURFACE)
        .stroke(stroke)
        .rounding(12.0)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            ui.set_min_height(min_height);
            Frame::none()
                .fill(SURFACE_LOW)
                .rounding(egui::Rounding {
                    nw: 12.0,
                    ne: 12.0,
                    sw: 0.0,
                    se: 0.0,
                })
                .inner_margin(egui::Margin::symmetric(16.0, 11.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(title)
                                .font(theme::bold(11.0))
                                .color(if accent { TEXT } else { TEXT_MUTED }),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), controls);
                    });
                });
            ui.separator();
            Frame::none()
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, body);
        });
}

pub fn chip(ui: &mut Ui, label: &str, active: bool) -> Response {
    let text = RichText::new(label)
        .font(theme::bold(9.0))
        .color(if active { TEXT } else { TEXT_MUTED });
    let button = egui::Button::new(text)
        .fill(if active {
            Color32::from_rgba_unmultiplied(91, 168, 255, 38)
        } else {
            BACKGROUND
        })
        .stroke(Stroke::new(1.0, if active { CYAN } else { OUTLINE }))
        .rounding(7.0);
    ui.add(button)
}

pub fn summary_metric(
    ui: &mut Ui,
    label: &str,
    value: Option<f32>,
    unit: &str,
    detail: &str,
    color: Color32,
) {
    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, OUTLINE))
        .rounding(12.0)
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(78.0);
            ui.label(
                RichText::new(label)
                    .font(theme::bold(9.0))
                    .color(TEXT_MUTED),
            );
            ui.horizontal(|ui| {
                let value = value.map_or_else(|| "N/A".to_owned(), |value| format!("{value:.1}"));
                ui.label(RichText::new(value).font(theme::mono(25.0)).color(color));
                ui.label(RichText::new(unit).font(theme::bold(8.0)).color(TEXT_MUTED));
            });
            ui.label(
                RichText::new(detail)
                    .font(theme::mono(8.0))
                    .color(TEXT_MUTED),
            );
        });
}

pub fn nav_item(ui: &mut Ui, glyph: &str, label: &str, selected: bool, enabled: bool) -> Response {
    let color = if selected { CYAN } else { TEXT_MUTED };
    let button = egui::Button::new(
        RichText::new(format!("{glyph}   {label}"))
            .font(theme::bold(11.0))
            .color(color),
    )
    .fill(if selected {
        Color32::from_rgba_unmultiplied(91, 168, 255, 38)
    } else {
        Color32::TRANSPARENT
    })
    .stroke(Stroke::NONE)
    .rounding(7.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(vec2(ui.available_width() - 16.0, 44.0), button)
    })
    .inner
}

pub fn level_meter(ui: &mut Ui, current_dbfs: f32, held_dbfs: f32, label: &str, color: Color32) {
    let (rect, response) = ui.allocate_exact_size(vec2(58.0, 158.0), Sense::hover());
    response.on_hover_text(format!(
        "現在値: {current_dbfs:.1} dBFS\n保持値: {held_dbfs:.1} dBFS"
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
        if segment_low >= normalized {
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
        let segment_color = if segment == segments - 1 {
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
    response.on_hover_text("300秒の音量履歴（RMS、10 Hz更新）");
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
            "履歴データを待っています",
            theme::bold(9.0),
            TEXT_MUTED,
        );
        return;
    };
    let end = last.seconds;
    let start = end - 300.0;
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
        } else {
            previous_reference = None;
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

pub fn spectrogram_image(points: &VecDeque<HistoryPoint>, width: usize) -> ColorImage {
    let mut image = ColorImage::new([width, HISTORY_SPECTRUM_BANDS], BACKGROUND);
    if width == 0 {
        return image;
    }
    let Some(last) = points.back() else {
        return image;
    };
    let start = last.seconds - 300.0;
    let mut levels = vec![-120.0_f32; width * HISTORY_SPECTRUM_BANDS];
    let mut gap_columns = vec![false; width];
    for point in points
        .iter()
        .filter(|point| point.gap && point.seconds >= start)
    {
        let x = (((point.seconds - start) / 300.0) * width as f64)
            .floor()
            .clamp(0.0, (width - 1) as f64) as usize;
        gap_columns[x] = true;
    }
    for point in points {
        if point.seconds < start {
            continue;
        }
        let x = (((point.seconds - start) / 300.0) * width as f64)
            .floor()
            .clamp(0.0, (width - 1) as f64) as usize;
        if gap_columns[x] {
            continue;
        }
        for (band, dbfs) in point.spectrum_dbfs.iter().copied().enumerate() {
            let y = HISTORY_SPECTRUM_BANDS - 1 - band;
            let index = y * width + x;
            levels[index] = levels[index].max(dbfs);
        }
    }
    for (pixel, level) in image.pixels.iter_mut().zip(levels) {
        *pixel = spectrum_color(level);
    }
    image
}

pub fn spectrogram(ui: &mut Ui, texture: Option<&TextureHandle>, sample_rate: u32, height: f32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    response.on_hover_text("過去300秒の周波数分布。明るい色ほど信号レベルが高いことを示します。");
    ui.painter().rect_filled(rect, 0.0, BACKGROUND);

    let graph = Rect::from_min_max(
        pos2(rect.left() + 48.0, rect.top() + 6.0),
        pos2(rect.right() - 8.0, rect.bottom() - 20.0),
    );
    if let Some(texture) = texture {
        ui.painter().image(
            texture.id(),
            graph,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        ui.painter().text(
            graph.center(),
            Align2::CENTER_CENTER,
            "履歴データを待っています",
            theme::bold(9.0),
            TEXT_MUTED,
        );
    }

    let nyquist = sample_rate as f64 * 0.5;
    for frequency in history_frequency_ticks(nyquist) {
        let fraction = (frequency / 20.0).ln() / (nyquist / 20.0).ln();
        let y = egui::lerp(graph.bottom()..=graph.top(), fraction as f32);
        ui.painter().line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(1.0, OUTLINE.gamma_multiply(0.35)),
        );
        ui.painter().text(
            pos2(graph.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            format_frequency(frequency),
            theme::mono(8.0),
            TEXT_MUTED,
        );
    }
    for (fraction, label) in [(0.0, "-300秒"), (0.5, "-150秒"), (1.0, "現在")] {
        let x = egui::lerp(graph.left()..=graph.right(), fraction);
        ui.painter().text(
            pos2(x, graph.bottom() + 5.0),
            if fraction == 0.0 {
                Align2::LEFT_TOP
            } else if fraction == 1.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::CENTER_TOP
            },
            label,
            theme::mono(8.0),
            TEXT_MUTED,
        );
    }
}

fn history_frequency_ticks(nyquist: f64) -> Vec<f64> {
    if !nyquist.is_finite() || nyquist <= 20.0 {
        return Vec::new();
    }
    let mut frequencies: Vec<f64> = [
        20.0, 100.0, 1_000.0, 10_000.0, 20_000.0, 50_000.0, 100_000.0,
    ]
    .into_iter()
    .filter(|frequency| *frequency <= nyquist)
    .collect();
    if frequencies
        .last()
        .is_none_or(|last| (last - nyquist).abs() > nyquist * 0.001)
    {
        frequencies.push(nyquist);
    }
    frequencies
}

fn spectrum_color(dbfs: f32) -> Color32 {
    let amount = ((dbfs.clamp(-100.0, 0.0) + 100.0) / 100.0).powf(1.35);
    if amount < 0.55 {
        mix_color(BACKGROUND, Color32::from_rgb(0, 65, 72), amount / 0.55)
    } else if amount < 0.82 {
        mix_color(Color32::from_rgb(0, 65, 72), CYAN, (amount - 0.55) / 0.27)
    } else {
        mix_color(CYAN, GREEN_BRIGHT, (amount - 0.82) / 0.18)
    }
}

fn mix_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount) as u8;
    Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
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

fn db_normalized(dbfs: f32) -> f32 {
    ((dbfs.clamp(-60.0, 0.0) + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrogram_aggregates_peaks_and_blanks_discontinuities() {
        let point = HistoryPoint {
            seconds: 1.0,
            measurement_rms: -6.0,
            reference_rms: None,
            spectrum_dbfs: [-3.0; HISTORY_SPECTRUM_BANDS],
            gap: false,
        };
        let mut points = VecDeque::from([point]);
        let image = spectrogram_image(&points, 600);
        assert_eq!(image.pixels[599], spectrum_color(-3.0));
        assert_eq!(image.pixels[0], BACKGROUND);
        points.push_back(HistoryPoint {
            seconds: 1.1,
            gap: true,
            ..point
        });
        assert!(
            spectrogram_image(&points, 600)
                .pixels
                .iter()
                .all(|pixel| *pixel == BACKGROUND)
        );
        assert!(spectrogram_image(&points, 0).pixels.is_empty());
    }

    #[test]
    fn widgets_render_without_audio_at_minimum_and_desktop_sizes() {
        for width in [760.0, 1440.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 900.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        summary_metric(ui, "測定", None, "dBFS", "Peak N/A", CYAN);
                        level_meter(ui, -120.0, -120.0, "測定", CYAN);
                        history_plot(ui, &VecDeque::new(), 100.0);
                        spectrogram(ui, None, 48_000, 100.0);
                        vectorscope(ui, &[], None);
                    });
                },
            );
            assert!(!output.shapes.is_empty());
        }
    }
}
