use std::sync::Arc;

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle, Vec2,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x0d, 0x0e, 0x10);
pub const SURFACE: Color32 = Color32::from_rgb(0x12, 0x13, 0x15);
pub const SURFACE_LOW: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1e);
pub const SURFACE_HIGH: Color32 = Color32::from_rgb(0x29, 0x2a, 0x2c);
pub const OUTLINE: Color32 = Color32::from_rgb(0x3b, 0x4b, 0x37);
pub const TEXT: Color32 = Color32::from_rgb(0xe3, 0xe2, 0xe5);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0xb9, 0xcc, 0xb2);
pub const GREEN: Color32 = Color32::from_rgb(0x00, 0xe6, 0x39);
pub const GREEN_BRIGHT: Color32 = Color32::from_rgb(0x72, 0xff, 0x70);
pub const CYAN: Color32 = Color32::from_rgb(0x00, 0xdb, 0xe9);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x3b, 0x30);
pub const YELLOW: Color32 = Color32::from_rgb(0xff, 0xc1, 0x07);

const BOLD_FAMILY: &str = "JetBrains Mono Bold";

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "jetbrains-regular".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jetbrains-bold".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/JetBrainsMono-Bold.ttf"
        ))),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .get_mut(&family)
            .expect("default font family must exist")
            .insert(0, "jetbrains-regular".to_owned());
    }
    fonts.families.insert(
        FontFamily::Name(BOLD_FAMILY.into()),
        vec!["jetbrains-bold".to_owned()],
    );
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::splat(8.0);
    style.spacing.button_padding = Vec2::new(8.0, 4.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(24.0, FontFamily::Name(BOLD_FAMILY.into())),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13.0, FontFamily::Monospace));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(11.0, FontFamily::Name(BOLD_FAMILY.into())),
    );
    style
        .text_styles
        .insert(TextStyle::Small, FontId::new(10.0, FontFamily::Monospace));
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = SURFACE_LOW;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.window_rounding = Rounding::ZERO;
    style.visuals.menu_rounding = Rounding::ZERO;
    style.visuals.window_stroke = Stroke::new(1.0, OUTLINE);
    style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0, 230, 57, 35);
    style.visuals.selection.stroke = Stroke::new(1.0, GREEN);

    for visuals in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visuals.rounding = Rounding::ZERO;
    }
    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, OUTLINE);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, OUTLINE);
    style.visuals.widgets.hovered.bg_fill = SURFACE_HIGH;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, GREEN);
    style.visuals.widgets.active.bg_fill = SURFACE_LOW;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, GREEN_BRIGHT);
    ctx.set_style(style);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
