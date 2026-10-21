use std::sync::Arc;

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle, Vec2,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x10, 0x14, 0x1b);
pub const SURFACE: Color32 = Color32::from_rgb(0x16, 0x1c, 0x25);
pub const SURFACE_LOW: Color32 = Color32::from_rgb(0x1d, 0x25, 0x31);
pub const SURFACE_HIGH: Color32 = Color32::from_rgb(0x2a, 0x36, 0x47);
pub const OUTLINE: Color32 = Color32::from_rgb(0x35, 0x43, 0x56);
pub const TEXT: Color32 = Color32::from_rgb(0xf1, 0xf5, 0xfb);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0xa6, 0xb3, 0xc5);
pub const GREEN: Color32 = Color32::from_rgb(0x3d, 0xd6, 0x8c);
pub const GREEN_BRIGHT: Color32 = Color32::from_rgb(0x76, 0xe8, 0xaf);
pub const CYAN: Color32 = Color32::from_rgb(0x5b, 0xa8, 0xff);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x6b, 0x72);
pub const YELLOW: Color32 = Color32::from_rgb(0xff, 0xc8, 0x57);

const BOLD_FAMILY: &str = "JetBrains Mono Bold";

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let japanese_font = [
        r"C:\Windows\Fonts\BIZ-UDGothicR.ttc",
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
    ]
    .into_iter()
    .find_map(|path| std::fs::read(path).ok());
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
    if let Some(font) = japanese_font {
        fonts.font_data.insert(
            "japanese-system".to_owned(),
            Arc::new(FontData::from_owned(font)),
        );
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let family_fonts = fonts
            .families
            .get_mut(&family)
            .expect("default font family must exist");
        if fonts.font_data.contains_key("japanese-system") {
            family_fonts.insert(0, "japanese-system".to_owned());
        }
        // Proportional UI text stays readable; reserve fixed-width faces for data.
        if family == FontFamily::Monospace {
            family_fonts.insert(0, "jetbrains-regular".to_owned());
        }
    }
    let mut bold_fonts = vec!["jetbrains-bold".to_owned()];
    if fonts.font_data.contains_key("japanese-system") {
        bold_fonts.push("japanese-system".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Name(BOLD_FAMILY.into()), bold_fonts);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.window_margin = egui::Margin::same(16.0);
    style.spacing.interact_size.y = 30.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(16.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = SURFACE_LOW;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.window_rounding = Rounding::same(8.0);
    style.visuals.menu_rounding = Rounding::same(5.0);
    style.visuals.window_stroke = Stroke::new(1.0_f32, OUTLINE);
    style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(91, 168, 255, 45);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, CYAN);

    for visuals in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visuals.rounding = Rounding::same(4.0);
    }
    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
    style.visuals.widgets.inactive.bg_fill = SURFACE_LOW;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
    style.visuals.widgets.hovered.bg_fill = SURFACE_HIGH;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, CYAN);
    style.visuals.widgets.active.bg_fill = SURFACE_LOW;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, CYAN);
    ctx.set_style(style);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
