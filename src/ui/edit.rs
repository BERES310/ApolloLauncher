use super::theme::{self, BORDER, DANGER, TEXT, TEXT_SECONDARY};
use crate::model::AppEntry;
use crate::util::{format_hex_color, parse_hex_color};
use eframe::egui::{
    self, Align2, Context, CursorIcon, Sense, Stroke, StrokeKind, TextEdit, Window, vec2,
};

/// Working copy of the editable fields of one app while the edit dialog is open.
pub struct EditState {
    pub id: String,
    pub title: String,
    pub name: String,
    pub description: String,
    pub args: String,
    pub icon: String,
    pub accent: Option<[u8; 3]>,
    accent_text: String,
    focus_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditEvent {
    Save,
    Cancel,
    BrowseIcon,
}

impl EditState {
    pub fn from_entry(entry: &AppEntry) -> Self {
        Self {
            id: entry.id.clone(),
            title: entry.name.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            args: entry.args.clone(),
            icon: entry
                .custom_icon
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            accent: entry.accent,
            accent_text: entry.accent.map(format_hex_color).unwrap_or_default(),
            focus_pending: true,
        }
    }

    fn set_accent(&mut self, accent: Option<[u8; 3]>) {
        self.accent = accent;
        self.accent_text = accent.map(format_hex_color).unwrap_or_default();
    }
}

fn field(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label.to_uppercase())
            .size(11.0)
            .color(TEXT_SECONDARY),
    );
}

pub fn show(ctx: &Context, state: &mut EditState) -> Option<EditEvent> {
    let mut event = None;
    let mut open = true;
    Window::new(format!("Edit {}", state.title))
        .id(egui::Id::unique("edit-dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .fixed_size(vec2(460.0, 0.0))
        .show(ctx, |ui| {
            field(ui, "Name");
            let name = ui.add(TextEdit::singleline(&mut state.name).desired_width(f32::INFINITY));
            if state.focus_pending {
                name.request_focus();
                state.focus_pending = false;
            }
            field(ui, "Description");
            ui.add(
                TextEdit::multiline(&mut state.description)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            field(ui, "Arguments");
            ui.add(TextEdit::singleline(&mut state.args).desired_width(f32::INFINITY));
            field(
                ui,
                "Icon (png, jpg, bmp, gif, webp, tiff, ico or exe) - empty = automatic",
            );
            ui.horizontal(|ui| {
                if ui.button("Browse\u{2026}").clicked() {
                    event = Some(EditEvent::BrowseIcon);
                }
                if ui.button("Clear").clicked() {
                    state.icon.clear();
                }
                ui.add(TextEdit::singleline(&mut state.icon).desired_width(f32::INFINITY));
            });
            field(ui, "Accent colour");
            ui.horizontal(|ui| {
                for rgb in theme::accent_palette() {
                    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                    let selected = state.accent == Some(*rgb);
                    ui.painter()
                        .rect_filled(rect, 0.0, theme::accent_color(*rgb));
                    if selected {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            0.0,
                            Stroke::new(1.5, TEXT),
                            StrokeKind::Outside,
                        );
                    }
                    if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        state.set_accent(Some(*rgb));
                    }
                }
                let response = ui.add(
                    TextEdit::singleline(&mut state.accent_text)
                        .hint_text("#RRGGBB")
                        .desired_width(90.0),
                );
                if response.changed() {
                    state.accent = parse_hex_color(&state.accent_text);
                }
                if ui.button("Default").clicked() {
                    state.set_accent(None);
                }
            });
            if !state.accent_text.trim().is_empty() && parse_hex_color(&state.accent_text).is_none()
            {
                ui.label(
                    egui::RichText::new("Not a valid colour; use #RRGGBB")
                        .size(12.0)
                        .color(DANGER),
                );
            }
            ui.add_space(6.0);
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, BORDER);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    event = Some(EditEvent::Save);
                }
                if ui.button("Cancel").clicked() {
                    event = Some(EditEvent::Cancel);
                }
            });
        });
    if !open {
        event = Some(EditEvent::Cancel);
    }
    if event.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        event = Some(EditEvent::Cancel);
    }
    event
}
