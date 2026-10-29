pub mod card;
pub mod edit;
pub mod glyphs;
pub mod list_row;
pub mod settings;
pub mod tabs;
pub mod theme;

use crate::model::AppEntry;
use crate::util::{hash_value, split_name};
use eframe::egui::{
    self, Align2, Button, Color32, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind,
    TextureHandle, Ui, text::LayoutJob, text::TextFormat, vec2,
};
use std::sync::Arc;
use theme::{BORDER, CARD, HOVER_SECONDS, TEXT, TEXT_SECONDARY};

/// Something the user asked to do with an app; applied by the application after drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Launch { id: String, admin: bool },
    Reveal(String),
    SetCategory { id: String, category: String },
    Edit(String),
    ToggleFavorite(String),
    Hide(String),
    Remove(String),
}

/// Drag-and-drop payload: an app being dragged towards a tab.
#[derive(Clone, Debug)]
pub struct DragApp(pub String);

/// Drag-and-drop payload: a category tab being reordered.
#[derive(Clone, Debug)]
pub struct DragTab(pub String);

/// Everything a card or row needs to draw one app.
pub struct EntryView<'a> {
    pub entry: &'a AppEntry,
    pub icon: Option<&'a TextureHandle>,
    pub icon_pending: bool,
    pub accent: Color32,
    pub categories: &'a [String],
    pub now: i64,
}

pub fn dim(color: Color32, entry: &AppEntry) -> Color32 {
    if entry.missing {
        color.gamma_multiply(0.45)
    } else {
        color
    }
}

/// Lays out `text` on at most `rows` rows no wider than `width`, ending in an ellipsis when cut.
pub fn fit_text(
    painter: &Painter,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            ..TextFormat::default()
        },
    );
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = rows == 1;
    job.wrap.overflow_character = Some('\u{2026}');
    painter.layout_job(job)
}

/// Draws bold text by overdrawing it one pixel apart (only light fonts are bundled).
pub fn bold_text(
    painter: &Painter,
    pos: egui::Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    color: Color32,
) -> Rect {
    painter.text(pos + vec2(0.6, 0.0), anchor, text, font.clone(), color);
    painter.text(pos, anchor, text, font, color)
}

fn tile_color(name: &str) -> Color32 {
    let hue = (hash_value(name) % 360) as f32;
    let (s, l) = (0.42_f32, 0.36_f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(to_byte(r), to_byte(g), to_byte(b))
}

/// Paints the app icon (or a generated letter tile while loading / when none exists).
pub fn paint_icon(painter: &Painter, rect: Rect, view: &EntryView) {
    let entry = view.entry;
    let rounding = (rect.height() * 0.2).round();
    let tint = dim(Color32::WHITE, entry);
    match view.icon {
        Some(texture) => {
            let inner = rect.shrink(rect.height() * 0.04);
            painter.image(
                texture.id(),
                inner,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
        None => {
            let base = if view.icon_pending {
                CARD
            } else {
                tile_color(&entry.name)
            };
            painter.rect_filled(rect, rounding, dim(base, entry));
            if !view.icon_pending {
                let letter = entry
                    .name
                    .chars()
                    .find(|c| c.is_alphanumeric())
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".to_owned());
                bold_text(
                    painter,
                    rect.center(),
                    Align2::CENTER_CENTER,
                    &letter,
                    theme::small(rect.height() * 0.5),
                    dim(Color32::WHITE, entry),
                );
            }
        }
    }
}

/// Paints an app name with its second half in the accent colour ("CAD" + "Craft").
pub fn paint_name(
    painter: &Painter,
    pos: egui::Pos2,
    entry: &AppEntry,
    font: FontId,
    accent: Color32,
    max_width: f32,
) {
    let (first, second) = split_name(&entry.name);
    let mut job = LayoutJob::default();
    let format = |color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        ..TextFormat::default()
    };
    job.append(&first, 0.0, format(dim(TEXT, entry)));
    if !second.is_empty() {
        job.append(&second, 0.0, format(dim(accent, entry)));
    }
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    let galley = painter.layout_job(job);
    painter.galley(pos + vec2(0.5, 0.0), galley.clone(), TEXT);
    painter.galley(pos, galley, TEXT);
}

/// A flat rectangular button with the shared 120 ms hover transition.
pub fn flat_button(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    accent: Color32,
    enabled: bool,
) -> Response {
    let id = ui.scope_id().with(("flat", id_salt));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let t =
        ui.ctx()
            .animate_bool_with_time(id.with("t"), enabled && response.hovered(), HOVER_SECONDS);
    let painter = ui.painter();
    let base = if enabled {
        accent
    } else {
        TEXT_SECONDARY.gamma_multiply(0.5)
    };
    let fill = theme::mix(CARD, base, t);
    let border = theme::mix(base.gamma_multiply(0.55), base, t);
    painter.rect(
        rect,
        0.0,
        fill,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    let text_color = theme::mix(base, theme::BG, t);
    bold_text(
        painter,
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        theme::small(13.0),
        text_color,
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A square button drawing a glyph through `draw`.
pub fn glyph_button(
    ui: &mut Ui,
    rect: Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    active: bool,
    draw: impl FnOnce(&Painter, egui::Pos2, Color32),
) -> Response {
    let id = ui.scope_id().with(("glyph", id_salt));
    let response = ui.interact(rect, id, Sense::click());
    let t =
        ui.ctx()
            .animate_bool_with_time(id.with("t"), response.hovered() || active, HOVER_SECONDS);
    let painter = ui.painter();
    let border = theme::mix(BORDER, TEXT_SECONDARY, t);
    painter.rect(
        rect,
        0.0,
        theme::mix(CARD, theme::CARD_HOVER, t),
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    draw(painter, rect.center(), theme::mix(TEXT_SECONDARY, TEXT, t));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The shared content of the `...` button menu and the right-click menu of an app.
pub fn entry_menu(ui: &mut Ui, entry: &AppEntry, categories: &[String]) -> Option<Action> {
    let mut action = None;
    let id = entry.id.clone();
    ui.set_min_width(190.0);
    if ui
        .add_enabled(!entry.missing, Button::new("Open"))
        .clicked()
    {
        action = Some(Action::Launch {
            id: id.clone(),
            admin: false,
        });
    }
    if ui
        .add_enabled(!entry.missing, Button::new("Run as administrator"))
        .clicked()
    {
        action = Some(Action::Launch {
            id: id.clone(),
            admin: true,
        });
    }
    if ui.button("Open file location").clicked() {
        action = Some(Action::Reveal(id.clone()));
    }
    ui.separator();
    ui.menu_button("Change category", |ui| {
        ui.set_min_width(150.0);
        for name in categories {
            let current = *name == entry.category;
            let label = if current {
                format!("\u{2022} {name}")
            } else {
                name.clone()
            };
            if ui.button(label).clicked() && !current {
                action = Some(Action::SetCategory {
                    id: id.clone(),
                    category: name.clone(),
                });
            }
        }
    });
    if ui.button("Edit\u{2026}").clicked() {
        action = Some(Action::Edit(id.clone()));
    }
    let favorite_label = if entry.favorite {
        "Remove from favorites"
    } else {
        "Add to favorites"
    };
    if ui.button(favorite_label).clicked() {
        action = Some(Action::ToggleFavorite(id.clone()));
    }
    ui.separator();
    if ui.button("Hide").clicked() {
        action = Some(Action::Hide(id.clone()));
    }
    if ui.button("Remove from launcher").clicked() {
        action = Some(Action::Remove(id));
    }
    if action.is_some() {
        ui.close();
    }
    action
}
