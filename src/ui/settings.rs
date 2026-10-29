use super::theme::{BORDER, DANGER, TEXT, TEXT_SECONDARY};
use crate::model::{Config, UNCATEGORIZED};
use eframe::egui::{
    self, Align2, Button, ComboBox, Context, RichText, ScrollArea, Stroke, Ui, Window, vec2,
};

/// Things the settings window asks the application to do beyond editing the config in place.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsEvent {
    /// The config was edited; save it and rebuild the view. `rescan` when sources or scan options changed.
    Changed {
        rescan: bool,
    },
    AddFolder,
    AddApp,
    RenameCategory(String),
    DeleteCategory(String),
    Restore(String),
    Autostart(bool),
    ClearIcons,
    ResetConfig,
    OpenDataFolder,
    OpenShortcuts,
}

#[derive(Default)]
pub struct SettingsState {
    pub open: bool,
    new_category: String,
    confirm_reset: bool,
}

pub struct HiddenApp {
    pub id: String,
    pub name: String,
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.5)
            .color(TEXT_SECONDARY),
    );
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, BORDER);
}

pub fn show(
    ctx: &Context,
    state: &mut SettingsState,
    config: &mut Config,
    hidden: &[HiddenApp],
    unreadable: &[std::path::PathBuf],
    config_path: &str,
) -> Vec<SettingsEvent> {
    let mut events = Vec::new();
    if !state.open {
        return events;
    }
    let mut open = true;
    Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .default_height(560.0)
        .min_width(360.0)
        .max_width(900.0)
        .constrain(true)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    general(ui, config, &mut events);
                    sources(ui, config, unreadable, &mut events);
                    categories(ui, state, config, &mut events);
                    hidden_apps(ui, hidden, &mut events);
                    data(ui, state, config_path, &mut events);
                });
        });
    if !open {
        state.open = false;
        state.confirm_reset = false;
    }
    events
}

fn general(ui: &mut Ui, config: &mut Config, events: &mut Vec<SettingsEvent>) {
    heading(ui, "General");
    let settings = &mut config.settings;
    if ui
        .checkbox(
            &mut settings.close_on_launch,
            "Close ApolloLauncher after launching an app",
        )
        .changed()
    {
        events.push(SettingsEvent::Changed { rescan: false });
    }
    let mut autostart = settings.start_with_windows;
    if ui.checkbox(&mut autostart, "Start with Windows").changed() {
        events.push(SettingsEvent::Autostart(autostart));
    }
    if ui
        .checkbox(&mut settings.special_tabs, "Show Favorites and Recent tabs")
        .changed()
    {
        events.push(SettingsEvent::Changed { rescan: false });
    }
    if ui
        .checkbox(&mut settings.filter_noise, "Hide installer noise")
        .on_hover_text("unins000.exe, uninstall*, crashhandler*, vcredist*, *setup*")
        .changed()
    {
        events.push(SettingsEvent::Changed { rescan: true });
    }
    if ui
        .checkbox(&mut settings.scan_recursive, "Scan sub-folders")
        .changed()
    {
        events.push(SettingsEvent::Changed { rescan: true });
    }
    if settings.scan_recursive {
        ui.horizontal(|ui| {
            ui.label("Maximum depth");
            let mut depth = settings.scan_depth;
            if ui
                .add(egui::DragValue::new(&mut depth).range(1..=12))
                .changed()
            {
                settings.scan_depth = depth;
                events.push(SettingsEvent::Changed { rescan: true });
            }
        });
    }
}

fn category_names(config: &Config) -> Vec<String> {
    std::iter::once(UNCATEGORIZED.to_owned())
        .chain(config.categories.iter().map(|c| c.name.clone()))
        .collect()
}

fn sources(
    ui: &mut Ui,
    config: &mut Config,
    unreadable: &[std::path::PathBuf],
    events: &mut Vec<SettingsEvent>,
) {
    heading(ui, "Sources");
    if config.sources.is_empty() {
        ui.label(RichText::new("No folders are being scanned yet.").color(TEXT_SECONDARY));
    }
    ui.label(
        RichText::new("Only shortcuts (.lnk) you put in these folders are listed.")
            .size(12.0)
            .color(TEXT_SECONDARY),
    );
    let names = category_names(config);
    let mut remove = None;
    for (index, source) in config.sources.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            if ui.add(Button::new("Remove")).clicked() {
                remove = Some(index);
            }
            ComboBox::from_id_salt(("source-category", index))
                .selected_text(source.default_category.clone())
                .width(130.0)
                .show_ui(ui, |ui| {
                    for name in &names {
                        if ui
                            .selectable_label(source.default_category == *name, name)
                            .clicked()
                            && source.default_category != *name
                        {
                            source.default_category = name.clone();
                            events.push(SettingsEvent::Changed { rescan: true });
                        }
                    }
                });
            let path = source.path.display().to_string();
            let missing = unreadable.contains(&source.path);
            let label = ui.add(
                egui::Label::new(RichText::new(&path).color(if missing { DANGER } else { TEXT }))
                    .truncate(),
            );
            if missing {
                label.on_hover_text(format!("{path}\nThis folder cannot be read"));
            } else {
                label.on_hover_text(path);
            }
        });
        if unreadable.contains(&source.path) {
            ui.label(
                RichText::new("Folder is missing or unreadable.")
                    .size(12.0)
                    .color(DANGER),
            );
        }
    }
    if let Some(index) = remove {
        config.sources.remove(index);
        events.push(SettingsEvent::Changed { rescan: true });
    }
    ui.horizontal(|ui| {
        if ui.button("Open shortcuts folder").clicked() {
            events.push(SettingsEvent::OpenShortcuts);
        }
        if ui.button("Add folder\u{2026}").clicked() {
            events.push(SettingsEvent::AddFolder);
        }
    });
}

fn categories(
    ui: &mut Ui,
    state: &mut SettingsState,
    config: &mut Config,
    events: &mut Vec<SettingsEvent>,
) {
    heading(ui, "Categories");
    let count = config.categories.len();
    let mut swap = None;
    for index in 0..count {
        let name = config.categories[index].name.clone();
        ui.horizontal(|ui| {
            ui.add_sized([180.0, 24.0], egui::Label::new(name.clone()).truncate());
            if ui.add_enabled(index > 0, Button::new("Up")).clicked() {
                swap = Some((index, index - 1));
            }
            if ui
                .add_enabled(index + 1 < count, Button::new("Down"))
                .clicked()
            {
                swap = Some((index, index + 1));
            }
            if ui.button("Rename").clicked() {
                events.push(SettingsEvent::RenameCategory(name.clone()));
            }
            if ui.button("Delete").clicked() {
                events.push(SettingsEvent::DeleteCategory(name));
            }
        });
    }
    if let Some((from, to)) = swap {
        config.move_category(from, to);
        events.push(SettingsEvent::Changed { rescan: false });
    }
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.new_category)
                .hint_text("New category")
                .desired_width(180.0),
        );
        let submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button("Add").clicked() || submit) && config.add_category(&state.new_category) {
            state.new_category.clear();
            events.push(SettingsEvent::Changed { rescan: false });
        }
    });
}

fn hidden_apps(ui: &mut Ui, hidden: &[HiddenApp], events: &mut Vec<SettingsEvent>) {
    heading(ui, "Hidden apps");
    if hidden.is_empty() {
        ui.label(RichText::new("Nothing is hidden.").color(TEXT_SECONDARY));
    }
    for app in hidden {
        ui.horizontal(|ui| {
            if ui.button("Restore").clicked() {
                events.push(SettingsEvent::Restore(app.id.clone()));
            }
            ui.label(&app.name);
        });
    }
}

fn data(
    ui: &mut Ui,
    state: &mut SettingsState,
    config_path: &str,
    events: &mut Vec<SettingsEvent>,
) {
    heading(ui, "Apps and data");
    ui.horizontal_wrapped(|ui| {
        if ui.button("Add app\u{2026}").clicked() {
            events.push(SettingsEvent::AddApp);
        }
        if ui.button("Clear icon cache").clicked() {
            events.push(SettingsEvent::ClearIcons);
        }
        if ui.button("Open data folder").clicked() {
            events.push(SettingsEvent::OpenDataFolder);
        }
    });
    ui.label(
        RichText::new(format!("Config: {config_path}"))
            .size(12.0)
            .color(TEXT_SECONDARY),
    );
    ui.add_space(6.0);
    if state.confirm_reset {
        ui.label(
            RichText::new("This deletes all sources, categories and per-app edits.").color(DANGER),
        );
        ui.horizontal(|ui| {
            if ui
                .add(
                    Button::new(RichText::new("Yes, reset").color(DANGER))
                        .stroke(Stroke::new(1.0, DANGER)),
                )
                .clicked()
            {
                state.confirm_reset = false;
                events.push(SettingsEvent::ResetConfig);
            }
            if ui.button("Cancel").clicked() {
                state.confirm_reset = false;
            }
        });
    } else if ui.button("Reset configuration\u{2026}").clicked() {
        state.confirm_reset = true;
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new(format!("ApolloLauncher {}", env!("CARGO_PKG_VERSION")))
            .size(11.5)
            .color(TEXT_SECONDARY),
    );
}
