use super::theme::{
    self, BORDER, CARD, CARD_HOVER, FAVORITE, HOVER_SECONDS, TEXT_SECONDARY, WARNING,
};
use super::{
    Action, DragApp, EntryView, dim, entry_menu, fit_text, flat_button, glyph_button, glyphs,
    paint_icon, paint_name,
};
use crate::model::AppEntry;
use eframe::egui::{Align2, Popup, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

pub const CARD_HEIGHT: f32 = 200.0;
pub const CARD_MIN_WIDTH: f32 = 440.0;
pub const GAP: f32 = 16.0;
const HEADER: f32 = 34.0;
const ICON: f32 = 74.0;
const PAD: f32 = 14.0;
const BUTTON_HEIGHT: f32 = 32.0;

/// Number of grid columns that fit in `available` pixels (at least one).
pub fn columns(available: f32) -> usize {
    (((available + GAP) / (CARD_MIN_WIDTH + GAP)).floor() as usize).max(1)
}

fn description(entry: &AppEntry) -> String {
    if !entry.description.trim().is_empty() {
        return entry.description.clone();
    }
    entry
        .target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn show(ui: &mut Ui, rect: Rect, view: &EntryView) -> Option<Action> {
    let entry = view.entry;
    let mut action = None;
    let id = ui.scope_id().with(("card", &entry.id));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let hovered = ui.rect_contains_pointer(rect);
    let t = ui
        .ctx()
        .animate_bool_with_time(id.with("hover"), hovered, HOVER_SECONDS);
    response.dnd_set_drag_payload(DragApp(entry.id.clone()));

    let painter = ui.painter().clone();
    let border = theme::mix(BORDER, view.accent, t);
    painter.rect(
        rect,
        0.0,
        theme::mix(CARD, CARD_HOVER, t),
        Stroke::new(1.0, dim(border, entry)),
        StrokeKind::Inside,
    );

    // Header: category on the left, favourite marker on the right.
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
    painter.line_segment(
        [
            header.left_bottom() + vec2(1.0, 0.0),
            header.right_bottom() - vec2(1.0, 0.0),
        ],
        Stroke::new(1.0, BORDER),
    );
    let category = entry.category.to_uppercase();
    let galley = fit_text(
        &painter,
        &category,
        theme::small(11.5),
        dim(TEXT_SECONDARY, entry),
        rect.width() * 0.6,
        1,
    );
    painter.galley(
        pos2(
            header.left() + PAD,
            header.center().y - galley.size().y / 2.0,
        ),
        galley,
        TEXT_SECONDARY,
    );

    let star_rect = Rect::from_min_max(
        pos2(rect.right() - 120.0, header.top() + 1.0),
        pos2(rect.right() - 1.0, header.bottom() - 1.0),
    );
    let star_response = ui.interact(star_rect, id.with("star"), Sense::click());
    let star_hover = star_response.hovered();
    if entry.favorite || star_hover || hovered {
        let color = if entry.favorite {
            FAVORITE
        } else {
            TEXT_SECONDARY
        };
        let color = if star_hover && !entry.favorite {
            theme::TEXT
        } else {
            color
        };
        let star_center = pos2(rect.right() - PAD - 6.0, header.center().y);
        glyphs::star(
            &painter,
            star_center,
            7.0,
            dim(color, entry),
            entry.favorite,
        );
        if entry.favorite {
            painter.text(
                pos2(star_center.x - 14.0, header.center().y),
                Align2::RIGHT_CENTER,
                "FAVORITE",
                theme::small(11.5),
                dim(FAVORITE, entry),
            );
        }
    }
    if star_response.clicked() {
        action = Some(Action::ToggleFavorite(entry.id.clone()));
    }
    star_response.on_hover_cursor(eframe::egui::CursorIcon::PointingHand);

    // Body: icon, name, description.
    let icon_rect = Rect::from_min_size(
        pos2(rect.left() + PAD, header.bottom() + PAD),
        vec2(ICON, ICON),
    );
    paint_icon(&painter, icon_rect, view);
    let text_left = icon_rect.right() + PAD;
    let text_width = (rect.right() - PAD - text_left).max(20.0);
    paint_name(
        &painter,
        pos2(text_left, icon_rect.top() + 2.0),
        entry,
        theme::small(20.0),
        view.accent,
        text_width,
    );
    if entry.missing {
        glyphs::warning(
            &painter,
            pos2(text_left + 7.0, icon_rect.top() + 40.0),
            13.0,
            WARNING,
        );
        painter.text(
            pos2(text_left + 20.0, icon_rect.top() + 40.0),
            Align2::LEFT_CENTER,
            "Target not found",
            theme::small(12.5),
            WARNING,
        );
        let galley = fit_text(
            &painter,
            &description(entry),
            theme::small(12.0),
            dim(TEXT_SECONDARY, entry),
            text_width,
            1,
        );
        painter.galley(
            pos2(text_left, icon_rect.top() + 54.0),
            galley,
            TEXT_SECONDARY,
        );
    } else {
        let galley = fit_text(
            &painter,
            &description(entry),
            theme::small(13.0),
            TEXT_SECONDARY,
            text_width,
            3,
        );
        painter.galley(
            pos2(text_left, icon_rect.top() + 30.0),
            galley,
            TEXT_SECONDARY,
        );
    }

    // Footer: OPEN and the `...` menu button.
    let footer_top = rect.bottom() - PAD - BUTTON_HEIGHT;
    let dots_rect = Rect::from_min_size(
        pos2(rect.right() - PAD - BUTTON_HEIGHT, footer_top),
        vec2(BUTTON_HEIGHT, BUTTON_HEIGHT),
    );
    let open_rect = Rect::from_min_max(
        pos2(rect.left() + PAD, footer_top),
        pos2(dots_rect.left() - 8.0, footer_top + BUTTON_HEIGHT),
    );
    let open = flat_button(
        ui,
        open_rect,
        ("open", &entry.id),
        "OPEN",
        view.accent,
        !entry.missing,
    );
    if open.clicked() {
        action = Some(Action::Launch {
            id: entry.id.clone(),
            admin: false,
        });
    }
    let dots = glyph_button(ui, dots_rect, ("dots", &entry.id), false, |p, c, color| {
        glyphs::dots(p, c, color);
    });
    Popup::menu(&dots).show(|ui| {
        if let Some(chosen) = entry_menu(ui, entry, view.categories) {
            action = Some(chosen);
        }
    });

    if response.double_clicked() && !entry.missing {
        action = Some(Action::Launch {
            id: entry.id.clone(),
            admin: false,
        });
    }
    response.context_menu(|ui| {
        if let Some(chosen) = entry_menu(ui, entry, view.categories) {
            action = Some(chosen);
        }
    });
    action
}

/// Paints the label shown next to the pointer while an app is dragged.
pub fn paint_drag_ghost(ui: &Ui, name: &str) {
    if let Some(pos) = ui.ctx().pointer_latest_pos() {
        let layer = eframe::egui::LayerId::new(
            eframe::egui::Order::Tooltip,
            eframe::egui::Id::unique("drag-ghost"),
        );
        let painter = ui.ctx().layer_painter(layer);
        let galley = painter.layout_no_wrap(name.to_owned(), theme::small(13.0), theme::TEXT);
        let rect = Rect::from_min_size(pos + vec2(14.0, 10.0), galley.size() + vec2(20.0, 12.0));
        painter.rect(
            rect,
            0.0,
            CARD_HOVER,
            Stroke::new(1.0, TEXT_SECONDARY),
            StrokeKind::Inside,
        );
        painter.galley(rect.min + vec2(10.0, 6.0), galley, theme::TEXT);
    }
}
