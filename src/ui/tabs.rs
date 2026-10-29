use super::theme::{self, BORDER, CARD_HOVER, HOVER_SECONDS, TEXT, TEXT_SECONDARY};
use super::{DragApp, DragTab, glyphs};
use eframe::egui::{
    Align2, CursorIcon, Id, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, pos2,
    scroll_area::ScrollBarVisibility, vec2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tab {
    All,
    Favorites,
    Recent,
    Category(String),
}

pub struct TabItem {
    pub tab: Tab,
    pub label: String,
    pub count: usize,
    /// Real, user-managed categories can be renamed, deleted, reordered and dragged.
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabAction {
    Select(Tab),
    NewCategory,
    Rename(String),
    Delete(String),
    Move { name: String, delta: i32 },
    Recategorize { id: String, category: String },
    Reorder { from: String, to: String },
}

pub const TAB_HEIGHT: f32 = 40.0;

pub fn show(ui: &mut Ui, items: &[TabItem], current: &Tab) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let painter = ui.painter().clone();
    let accent = theme::accent_color(theme::default_accent(0));
    let widths: Vec<f32> = items
        .iter()
        .map(|item| {
            let label = painter.layout_no_wrap(item.label.clone(), theme::small(14.0), TEXT);
            let count = painter.layout_no_wrap(item.count.to_string(), theme::small(11.5), TEXT);
            label.size().x + count.size().x + 38.0
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + TAB_HEIGHT;
    let editable_count = items.iter().filter(|item| item.editable).count();

    ScrollArea::horizontal()
        .id_salt("tabs-scroll")
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            let (strip, _) = ui.allocate_exact_size(
                vec2(total.max(ui.available_width()), TAB_HEIGHT),
                Sense::hover(),
            );
            let painter = ui.painter().clone();
            let mut x = strip.left();
            for (index, item) in items.iter().enumerate() {
                let rect =
                    Rect::from_min_size(pos2(x, strip.top()), vec2(widths[index], TAB_HEIGHT));
                x += widths[index];
                let id = ui.scope_id().with(("tab", &item.label));
                let sense = if item.editable {
                    Sense::click_and_drag()
                } else {
                    Sense::click()
                };
                let response = ui.interact(rect, id, sense);
                let selected = *current == item.tab;

                let dropping_app = matches!(item.tab, Tab::Category(_))
                    && response.dnd_hover_payload::<DragApp>().is_some();
                let dropping_tab =
                    item.editable && response.dnd_hover_payload::<DragTab>().is_some();
                let t = ui.ctx().animate_bool_with_time(
                    id.with("hover"),
                    response.hovered() || dropping_app || dropping_tab,
                    HOVER_SECONDS,
                );
                if t > 0.0 {
                    painter.rect_filled(rect, 0.0, theme::mix(theme::BG, CARD_HOVER, t));
                }
                if dropping_app || dropping_tab {
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        0.0,
                        Stroke::new(1.0, accent),
                        StrokeKind::Inside,
                    );
                }
                let color = if selected {
                    TEXT
                } else {
                    theme::mix(TEXT_SECONDARY, TEXT, t)
                };
                let label = painter.text(
                    pos2(rect.left() + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &item.label,
                    theme::small(14.0),
                    color,
                );
                painter.text(
                    pos2(label.right() + 7.0, rect.center().y + 1.0),
                    Align2::LEFT_CENTER,
                    item.count.to_string(),
                    theme::small(11.5),
                    TEXT_SECONDARY,
                );
                if response.clicked() {
                    actions.push(TabAction::Select(item.tab.clone()));
                }
                if let Tab::Category(name) = &item.tab {
                    if let Some(payload) = response.dnd_release_payload::<DragApp>() {
                        actions.push(TabAction::Recategorize {
                            id: payload.0.clone(),
                            category: name.clone(),
                        });
                    }
                    if item.editable {
                        response.dnd_set_drag_payload(DragTab(name.clone()));
                        if let Some(payload) = response.dnd_release_payload::<DragTab>()
                            && payload.0 != *name
                        {
                            actions.push(TabAction::Reorder {
                                from: payload.0.clone(),
                                to: name.clone(),
                            });
                        }
                    }
                }
                if item.editable
                    && let Tab::Category(name) = &item.tab
                {
                    let position = items.iter().take(index).filter(|i| i.editable).count();
                    response.context_menu(|ui| {
                        ui.set_min_width(150.0);
                        if ui.button("Rename\u{2026}").clicked() {
                            actions.push(TabAction::Rename(name.clone()));
                            ui.close();
                        }
                        if ui
                            .add_enabled(position > 0, eframe::egui::Button::new("Move left"))
                            .clicked()
                        {
                            actions.push(TabAction::Move {
                                name: name.clone(),
                                delta: -1,
                            });
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                position + 1 < editable_count,
                                eframe::egui::Button::new("Move right"),
                            )
                            .clicked()
                        {
                            actions.push(TabAction::Move {
                                name: name.clone(),
                                delta: 1,
                            });
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Delete").clicked() {
                            actions.push(TabAction::Delete(name.clone()));
                            ui.close();
                        }
                    });
                    if response.dragged() {
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    }
                }
                painter.line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    Stroke::new(1.0, BORDER),
                );
                if selected {
                    painter.rect_filled(
                        Rect::from_min_max(
                            pos2(rect.left(), rect.bottom() - 2.0),
                            rect.right_bottom(),
                        ),
                        0.0,
                        accent,
                    );
                }
            }

            let plus_rect = Rect::from_min_size(pos2(x, strip.top()), vec2(TAB_HEIGHT, TAB_HEIGHT));
            let plus = ui.interact(plus_rect, Id::unique("tab-plus"), Sense::click());
            let t = ui.ctx().animate_bool_with_time(
                Id::unique("tab-plus-hover"),
                plus.hovered(),
                HOVER_SECONDS,
            );
            painter.rect_filled(plus_rect, 0.0, theme::mix(theme::BG, CARD_HOVER, t));
            glyphs::plus(
                &painter,
                plus_rect.center(),
                6.0,
                theme::mix(TEXT_SECONDARY, TEXT, t),
            );
            painter.line_segment(
                [plus_rect.left_bottom(), strip.right_bottom()],
                Stroke::new(1.0, BORDER),
            );
            if plus
                .on_hover_text("New category")
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                actions.push(TabAction::NewCategory);
            }
        });
    actions
}
