use super::theme::{self, BORDER, CARD, CARD_HOVER, HOVER_SECONDS, TEXT_SECONDARY, WARNING};
use super::{
    Action, DragApp, EntryView, dim, entry_menu, fit_text, flat_button, glyph_button, glyphs,
    paint_icon, paint_name,
};
use crate::util::relative_time;
use eframe::egui::{Align2, Popup, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

pub const ROW_HEIGHT: f32 = 54.0;
const ICON: f32 = 34.0;
const PAD: f32 = 14.0;
const BUTTON_HEIGHT: f32 = 30.0;

pub fn show(ui: &mut Ui, rect: Rect, view: &EntryView) -> Option<Action> {
    let entry = view.entry;
    let mut action = None;
    let id = ui.scope_id().with(("row", &entry.id));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let hovered = ui.rect_contains_pointer(rect);
    let t = ui
        .ctx()
        .animate_bool_with_time(id.with("hover"), hovered, HOVER_SECONDS);
    response.dnd_set_drag_payload(DragApp(entry.id.clone()));

    let painter = ui.painter().clone();
    painter.rect(
        rect,
        0.0,
        theme::mix(CARD, CARD_HOVER, t),
        Stroke::new(1.0, dim(theme::mix(BORDER, view.accent, t), entry)),
        StrokeKind::Inside,
    );

    let dots_rect = Rect::from_min_size(
        pos2(
            rect.right() - PAD - BUTTON_HEIGHT,
            rect.center().y - BUTTON_HEIGHT / 2.0,
        ),
        vec2(BUTTON_HEIGHT, BUTTON_HEIGHT),
    );
    let open_rect = Rect::from_min_size(
        pos2(dots_rect.left() - 8.0 - 96.0, dots_rect.top()),
        vec2(96.0, BUTTON_HEIGHT),
    );

    let icon_rect = Rect::from_min_size(
        pos2(rect.left() + PAD, rect.center().y - ICON / 2.0),
        vec2(ICON, ICON),
    );
    paint_icon(&painter, icon_rect, view);

    // Columns shrink with the window: last launched first, then category.
    let mut right = open_rect.left() - PAD;
    let show_launched = rect.width() > 560.0;
    let show_category = rect.width() > 420.0;
    if show_launched {
        let text = match entry.last_launched {
            Some(_) => relative_time(entry.last_launched, view.now),
            None => "Never launched".to_owned(),
        };
        let left = right - 130.0;
        painter.text(
            pos2(left, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            theme::small(12.5),
            dim(TEXT_SECONDARY, entry),
        );
        right = left - PAD;
    }
    if show_category {
        let left = right - 140.0;
        let galley = fit_text(
            &painter,
            &entry.category,
            theme::small(13.0),
            dim(TEXT_SECONDARY, entry),
            136.0,
            1,
        );
        painter.galley(
            pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            TEXT_SECONDARY,
        );
        right = left - PAD;
    }
    let name_left = icon_rect.right() + PAD;
    let mut name_width = (right - name_left).max(30.0);
    let mut name_x = name_left;
    if entry.missing {
        glyphs::warning(
            &painter,
            pos2(name_left + 7.0, rect.center().y),
            13.0,
            WARNING,
        );
        name_x += 22.0;
        name_width = (name_width - 22.0).max(30.0);
    }
    paint_name(
        &painter,
        pos2(name_x, rect.center().y - 9.0),
        entry,
        theme::small(16.0),
        view.accent,
        name_width,
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
