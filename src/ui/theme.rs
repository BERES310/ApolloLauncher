use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x0F, 0x11, 0x14);
pub const CARD: Color32 = Color32::from_rgb(0x11, 0x13, 0x17);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x16, 0x19, 0x1E);
pub const BORDER: Color32 = Color32::from_rgb(0x2A, 0x2D, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF3, 0xF5);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x8A, 0x8F, 0x98);
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x5B, 0x5B);
pub const WARNING: Color32 = Color32::from_rgb(0xF2, 0xB1, 0x34);
pub const FAVORITE: Color32 = Color32::from_rgb(0xFF, 0xC8, 0x3D);

const ACCENTS: [[u8; 3]; 7] = [
    [0x22, 0xD3, 0xEE],
    [0xFF, 0x8A, 0x3D],
    [0x4A, 0xDE, 0x80],
    [0xF4, 0x72, 0xB6],
    [0x5E, 0xEA, 0xD4],
    [0xA7, 0x8B, 0xFA],
    [0xFA, 0xCC, 0x15],
];

pub const HOVER_SECONDS: f32 = 0.12;

pub const SQUARE: CornerRadius = CornerRadius::ZERO;

pub fn accent_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Default accent for the n-th card / category, cycling through the palette.
pub fn default_accent(index: usize) -> [u8; 3] {
    ACCENTS[index % ACCENTS.len()]
}

pub fn accent_palette() -> &'static [[u8; 3]] {
    &ACCENTS
}

/// Blend `from` towards `to` by `t` (0..=1).
pub fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        lerp(from.r(), to.r()),
        lerp(from.g(), to.g()),
        lerp(from.b(), to.b()),
    )
}

pub fn small(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "ubuntu-light".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Ubuntu-Light.ttf"
        ))),
    );
    fonts.font_data.insert(
        "hack".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Hack-Regular.ttf"
        ))),
    );
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "ubuntu-light".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.insert(0, "hack".to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = CARD;
    visuals.override_text_color = Some(TEXT);
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = SQUARE;
    visuals.menu_corner_radius = SQUARE;
    visuals.popup_shadow = egui::Shadow::NONE;
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.selection.bg_fill = Color32::from_rgb(0x1E, 0x3A, 0x44);
    visuals.selection.stroke = Stroke::new(1.0, accent_color(ACCENTS[0]));
    visuals.hyperlink_color = accent_color(ACCENTS[0]);

    let widgets = &mut visuals.widgets;
    for style in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        style.corner_radius = SQUARE;
    }
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.bg_fill = CARD;
    widgets.inactive.weak_bg_fill = CARD;
    widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.hovered.bg_fill = CARD_HOVER;
    widgets.hovered.weak_bg_fill = CARD_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.active.bg_fill = CARD_HOVER;
    widgets.active.weak_bg_fill = CARD_HOVER;
    widgets.active.bg_stroke = Stroke::new(1.0, TEXT);
    widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.open.bg_fill = CARD_HOVER;
    widgets.open.weak_bg_fill = CARD_HOVER;
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.scroll.floating = true;
        style.text_styles = [
            (TextStyle::Heading, small(20.0)),
            (TextStyle::Body, small(14.0)),
            (TextStyle::Button, small(13.0)),
            (TextStyle::Small, small(11.5)),
            (TextStyle::Monospace, mono(12.5)),
        ]
        .into();
    });
}
