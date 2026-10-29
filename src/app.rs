use crate::autostart;
use crate::config;
use crate::icons::IconManager;
use crate::launch;
use crate::model::{AppEntry, Config, SortMode, UNCATEGORIZED, ViewMode};
use crate::scanner::{self, ScanResult, ScannedItem};
use crate::ui::edit::{self, EditEvent, EditState};
use crate::ui::list_row::{self, ROW_HEIGHT};
use crate::ui::settings::{self, HiddenApp, SettingsEvent, SettingsState};
use crate::ui::tabs::{self, Tab, TabAction, TabItem};
use crate::ui::theme::{self, BG, BORDER, CARD, DANGER, TEXT, TEXT_SECONDARY};
use crate::ui::{Action, DragApp, EntryView, card, glyph_button, glyphs};
use crate::util::{format_hex_color, normalize, now_secs, split_name};
use crate::watcher::SourceWatcher;
use eframe::egui::{
    self, Align, Align2, CentralPanel, ComboBox, Context, DragAndDrop, Frame, Id, Layout, Margin,
    Order, Panel, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

const RECENT_LIMIT: usize = 20;
const TOAST_SECONDS: f64 = 4.0;
const ERROR_TOAST_SECONDS: f64 = 7.0;
const SIDE_PADDING: f32 = 20.0;
const SEARCH_ID: &str = "search-field";

struct Toast {
    text: String,
    error: bool,
    created: f64,
}

enum Modal {
    NewCategory(String),
    RenameCategory { old: String, text: String },
}

enum Picked {
    Folder(PathBuf),
    App(PathBuf),
    Icon(PathBuf),
}

pub struct App {
    config: Config,
    items: Vec<ScannedItem>,
    entries: Vec<AppEntry>,
    view: Vec<usize>,
    tab: Tab,
    search: String,
    toasts: Vec<Toast>,
    scan_rx: Option<Receiver<ScanResult>>,
    rescan_queued: bool,
    scanned_once: bool,
    watcher: SourceWatcher,
    icons: IconManager,
    unreadable: Vec<PathBuf>,
    settings: SettingsState,
    edit: Option<EditState>,
    modal: Option<Modal>,
    picker_tx: Sender<Option<Picked>>,
    picker_rx: Receiver<Option<Picked>>,
    picker_busy: bool,
    focus_search: bool,
    close_requested: bool,
}

// ---------------------------------------------------------------------------------------------
// Pure view logic (unit tested below)
// ---------------------------------------------------------------------------------------------

/// Whether every whitespace separated term of `query` occurs in the entry's name, description or category.
pub fn matches_query(entry: &AppEntry, query: &str) -> bool {
    let query = normalize(query);
    query
        .split_whitespace()
        .all(|term| entry.search_key.contains(term))
}

/// Orders `indices` (into `entries`) for the chosen sort mode; ties fall back to the name.
pub fn sort_indices(entries: &[AppEntry], indices: &mut [usize], sort: SortMode) {
    indices.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        let by_name = x.name_key.cmp(&y.name_key).then_with(|| x.id.cmp(&y.id));
        match sort {
            SortMode::NameAsc => by_name,
            SortMode::NameDesc => by_name.reverse(),
            SortMode::LastLaunched => y.last_launched.cmp(&x.last_launched).then(by_name),
            SortMode::MostLaunched => y.launch_count.cmp(&x.launch_count).then(by_name),
            SortMode::Category => normalize(&x.category)
                .cmp(&normalize(&y.category))
                .then(by_name),
        }
    });
}

/// The entries (as indices) shown for a tab, search text and sort order. Hidden apps never appear.
pub fn build_view(entries: &[AppEntry], tab: &Tab, query: &str, sort: SortMode) -> Vec<usize> {
    let mut indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.hidden)
        .filter(|(_, e)| match tab {
            Tab::All => true,
            Tab::Favorites => e.favorite,
            Tab::Recent => e.last_launched.is_some(),
            Tab::Category(name) => e.category == *name,
        })
        .filter(|(_, e)| matches_query(e, query))
        .map(|(i, _)| i)
        .collect();
    if *tab == Tab::Recent {
        sort_indices(entries, &mut indices, SortMode::LastLaunched);
        indices.truncate(RECENT_LIMIT);
    } else {
        sort_indices(entries, &mut indices, sort);
    }
    indices
}

fn is_launchable_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("lnk"))
}

// ---------------------------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------------------------

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let first_run = !config::config_path().exists();
        let outcome = config::load();
        let first_run = first_run || outcome.warning.is_some();
        let (picker_tx, picker_rx) = channel();
        let mut app = Self {
            config: outcome.config,
            items: Vec::new(),
            entries: Vec::new(),
            view: Vec::new(),
            tab: Tab::All,
            search: String::new(),
            toasts: Vec::new(),
            scan_rx: None,
            rescan_queued: false,
            scanned_once: false,
            watcher: SourceWatcher::new(ctx),
            icons: IconManager::new(ctx),
            unreadable: Vec::new(),
            settings: SettingsState::default(),
            edit: None,
            modal: None,
            picker_tx,
            picker_rx,
            picker_busy: false,
            focus_search: false,
            close_requested: false,
        };
        if let Some(warning) = outcome.warning {
            app.toast(warning, true);
        }
        app.config.settings.start_with_windows = autostart::is_enabled();
        if first_run {
            app.register_shortcuts_folder();
        }
        app.ensure_categories();
        app.start_scan(ctx);
        app
    }

    /// Makes ApolloLauncher's own shortcuts folder the (only) source of a fresh library.
    fn register_shortcuts_folder(&mut self) {
        let dir = config::shortcuts_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.toast(
                format!("Could not create the shortcuts folder: {error}"),
                true,
            );
            return;
        }
        if !self.config.sources.iter().any(|s| s.path == dir) {
            self.config
                .sources
                .push(crate::model::Source::new(dir, UNCATEGORIZED));
            self.save_config();
        }
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast {
            text: text.into(),
            error,
            created: f64::NAN,
        });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    fn save_config(&mut self) {
        if let Err(error) = config::save(&self.config) {
            self.toast(format!("Could not save settings: {error}"), true);
        }
    }

    /// Source default categories must exist as real categories so their apps get a tab.
    fn ensure_categories(&mut self) {
        let wanted: Vec<String> = self
            .config
            .sources
            .iter()
            .map(|s| s.default_category.clone())
            .collect();
        for name in wanted {
            if name != UNCATEGORIZED && !self.config.has_category(&name) {
                self.config.add_category(&name);
            }
        }
    }

    // ---- scanning -------------------------------------------------------------------------

    fn start_scan(&mut self, ctx: &Context) {
        if self.scan_rx.is_some() {
            self.rescan_queued = true;
            return;
        }
        let config = self.config.clone();
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        let spawned = thread::Builder::new()
            .name("scanner".into())
            .spawn(move || {
                let result = scanner::scan(&config);
                let _ = tx.send(result);
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => self.scan_rx = Some(rx),
            Err(error) => self.toast(format!("Could not start the scan: {error}"), true),
        }
    }

    fn poll_scan(&mut self, ctx: &Context) {
        let Some(rx) = &self.scan_rx else { return };
        let outcome = rx.try_recv();
        match outcome {
            Ok(result) => {
                self.scan_rx = None;
                self.apply_scan(result);
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.scan_rx = None;
                self.toast("The folder scan failed unexpectedly", true);
            }
        }
        if std::mem::take(&mut self.rescan_queued) {
            self.start_scan(ctx);
        }
    }

    fn apply_scan(&mut self, result: ScanResult) {
        let mut unreadable = result.unreadable_sources;
        let failed = self
            .watcher
            .set_sources(&self.config.sources, self.config.settings.scan_recursive);
        for path in failed {
            if !unreadable.contains(&path) {
                unreadable.push(path);
            }
        }
        let fresh: Vec<String> = unreadable
            .iter()
            .filter(|p| !self.unreadable.contains(p))
            .map(|p| p.display().to_string())
            .collect();
        for path in fresh {
            self.toast(format!("Cannot read folder: {path}"), true);
        }
        self.unreadable = unreadable;
        self.items = result.items;
        self.scanned_once = true;
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.entries = scanner::build_entries(&self.items, &self.config);
        self.refresh_view();
    }

    fn refresh_view(&mut self) {
        if let Tab::Category(name) = &self.tab
            && !self.config.has_category(name)
            && name != UNCATEGORIZED
        {
            self.tab = Tab::All;
        }
        if matches!(self.tab, Tab::Favorites | Tab::Recent) && !self.config.settings.special_tabs {
            self.tab = Tab::All;
        }
        self.view = build_view(
            &self.entries,
            &self.tab,
            &self.search,
            self.config.settings.sort,
        );
    }

    // ---- actions --------------------------------------------------------------------------

    fn entry_index(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    fn apply(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::Launch { id, admin } => self.launch(ctx, &id, admin),
            Action::Reveal(id) => {
                if let Some(index) = self.entry_index(&id)
                    && let Err(error) = launch::reveal(&self.entries[index])
                {
                    self.toast(error, true);
                }
            }
            Action::SetCategory { id, category } => self.set_category(&id, &category),
            Action::Edit(id) => {
                if let Some(index) = self.entry_index(&id) {
                    self.edit = Some(EditState::from_entry(&self.entries[index]));
                }
            }
            Action::ToggleFavorite(id) => {
                let over = self.config.override_mut(&id);
                over.favorite = !over.favorite;
                self.save_config();
                self.rebuild();
            }
            Action::Hide(id) => {
                let name = self
                    .entry_index(&id)
                    .map(|i| self.entries[i].name.clone())
                    .unwrap_or_default();
                self.config.override_mut(&id).hidden = true;
                self.save_config();
                self.rebuild();
                self.toast(format!("Hid {name}. Restore it in Settings."), false);
            }
            Action::Remove(id) => {
                let Some(index) = self.entry_index(&id) else {
                    return;
                };
                let name = self.entries[index].name.clone();
                let source = self.entries[index].source_path.clone();
                let manual = self.entries[index].manual;
                if manual {
                    self.config.apps.retain(|a| a.id != id);
                } else {
                    self.config.override_mut(&id).removed = true;
                }
                let mut recycled = false;
                if source.starts_with(config::shortcuts_dir()) && source.is_file() {
                    match launch::recycle(&source) {
                        Ok(()) => recycled = true,
                        Err(error) => self.toast(error, true),
                    }
                }
                self.save_config();
                self.items.retain(|item| item.id() != id || !item.manual);
                self.rebuild();
                self.start_scan(ctx);
                let note = if recycled {
                    " (shortcut moved to the Recycle Bin)"
                } else {
                    ""
                };
                self.toast(format!("Removed {name}{note}"), false);
            }
        }
    }

    fn launch(&mut self, ctx: &Context, id: &str, admin: bool) {
        let Some(index) = self.entry_index(id) else {
            return;
        };
        if let Err(error) = launch::launch(&self.entries[index], admin) {
            self.toast(error, true);
            return;
        }
        let now = now_secs();
        let count = {
            let over = self.config.override_mut(id);
            over.launch_count = over.launch_count.saturating_add(1);
            over.last_launched = Some(now);
            over.launch_count
        };
        let entry = &mut self.entries[index];
        entry.launch_count = count;
        entry.last_launched = Some(now);
        self.save_config();
        self.refresh_view();
        if self.config.settings.close_on_launch {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn set_category(&mut self, id: &str, category: &str) {
        self.config.override_mut(id).category = if category == UNCATEGORIZED {
            None
        } else {
            Some(category.to_owned())
        };
        self.save_config();
        self.rebuild();
        self.toast(format!("Moved to {category}"), false);
    }

    fn save_edit(&mut self, state: EditState) {
        let Some(index) = self.entry_index(&state.id) else {
            return;
        };
        let entry = self.entries[index].clone();
        let scanned = self.items.iter().find(|item| item.id() == state.id);
        let scanned_description = scanned.map(|s| s.description.clone()).unwrap_or_default();
        let scanned_args = scanned.map(|s| s.args.clone()).unwrap_or_default();
        let default_name = crate::util::file_stem(&entry.source_path);
        let name = state.name.trim();
        let description = state.description.trim();
        let args = state.args.trim();
        let icon = state.icon.trim();
        let icon_changed = entry.custom_icon.as_deref().map(Path::to_path_buf)
            != (!icon.is_empty()).then(|| PathBuf::from(icon));

        let over = self.config.override_mut(&state.id);
        over.name = (!name.is_empty() && name != default_name).then(|| name.to_owned());
        over.description = (description != scanned_description).then(|| description.to_owned());
        over.args = (args != scanned_args).then(|| args.to_owned());
        over.custom_icon = (!icon.is_empty()).then(|| PathBuf::from(icon));
        over.accent = state.accent.map(format_hex_color);
        if icon_changed {
            self.icons.invalidate(&state.id);
        }
        self.config.prune_overrides();
        self.save_config();
        self.rebuild();
        self.toast("Saved changes", false);
    }

    fn add_manual(&mut self, ctx: &Context, path: &Path) {
        if !is_launchable_file(path) {
            self.toast("Only .exe and .lnk files can be added", true);
            return;
        }
        let category = match &self.tab {
            Tab::Category(name) => name.clone(),
            _ => UNCATEGORIZED.to_owned(),
        };
        let Some(item) = scanner::resolve_item(path, &category) else {
            self.toast(format!("Could not read {}", path.display()), true);
            return;
        };
        let id = item.id();
        let exists = self.entries.iter().any(|e| e.id == id && !e.hidden);
        if exists {
            self.toast("That app is already in your library", false);
            return;
        }
        let over = self.config.override_mut(&id);
        over.manual = true;
        over.source_path = Some(path.to_path_buf());
        over.hidden = false;
        over.removed = false;
        if category != UNCATEGORIZED {
            over.category = Some(category);
        }
        self.save_config();
        self.toast(format!("Added {}", crate::util::file_stem(path)), false);
        self.start_scan(ctx);
    }

    // ---- dialogs on worker threads --------------------------------------------------------

    fn pick(&mut self, ctx: &Context, kind: PickKind) {
        if self.picker_busy {
            return;
        }
        self.picker_busy = true;
        let tx = self.picker_tx.clone();
        let repaint = ctx.clone();
        let spawned = thread::Builder::new()
            .name("file-dialog".into())
            .spawn(move || {
                let picked = match kind {
                    PickKind::Folder => rfd::FileDialog::new()
                        .set_title("Choose a folder to scan")
                        .pick_folder()
                        .map(Picked::Folder),
                    PickKind::App => rfd::FileDialog::new()
                        .set_title("Add an app")
                        .add_filter("Programs and shortcuts", &["exe", "lnk"])
                        .pick_file()
                        .map(Picked::App),
                    PickKind::Icon => rfd::FileDialog::new()
                        .set_title("Choose an icon")
                        .add_filter(
                            "Images and programs",
                            &[
                                "png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff", "ico",
                                "exe", "dll",
                            ],
                        )
                        .add_filter("All files", &["*"])
                        .pick_file()
                        .map(Picked::Icon),
                };
                let _ = tx.send(picked);
                repaint.request_repaint();
            });
        if let Err(error) = spawned {
            self.picker_busy = false;
            self.toast(format!("Could not open the file dialog: {error}"), true);
        }
    }

    fn poll_picker(&mut self, ctx: &Context) {
        while let Ok(picked) = self.picker_rx.try_recv() {
            self.picker_busy = false;
            match picked {
                None => {}
                Some(Picked::Folder(path)) => self.add_source(ctx, path),
                Some(Picked::App(path)) => self.add_manual(ctx, &path),
                Some(Picked::Icon(path)) => {
                    if let Some(state) = &mut self.edit {
                        state.icon = path.display().to_string();
                    }
                }
            }
        }
    }

    fn add_source(&mut self, ctx: &Context, path: PathBuf) {
        if self.config.sources.iter().any(|s| s.path == path) {
            self.toast("That folder is already a source", false);
            return;
        }
        let category = match &self.tab {
            Tab::Category(name) => name.clone(),
            _ => UNCATEGORIZED.to_owned(),
        };
        self.config
            .sources
            .push(crate::model::Source::new(path, &category));
        self.save_config();
        self.start_scan(ctx);
    }

    // ---- tabs and settings ----------------------------------------------------------------

    fn apply_tab_action(&mut self, ctx: &Context, action: TabAction) {
        match action {
            TabAction::Select(tab) => {
                self.tab = tab;
                self.refresh_view();
            }
            TabAction::NewCategory => self.modal = Some(Modal::NewCategory(String::new())),
            TabAction::Rename(name) => {
                self.modal = Some(Modal::RenameCategory {
                    text: name.clone(),
                    old: name,
                });
            }
            TabAction::Delete(name) => self.delete_category(ctx, &name),
            TabAction::Move { name, delta } => {
                if let Some(from) = self.config.categories.iter().position(|c| c.name == name) {
                    let to = from as i32 + delta;
                    if to >= 0 && (to as usize) < self.config.categories.len() {
                        self.config.move_category(from, to as usize);
                        self.save_config();
                        self.refresh_view();
                    }
                }
            }
            TabAction::Recategorize { id, category } => self.set_category(&id, &category),
            TabAction::Reorder { from, to } => {
                let positions = (
                    self.config.categories.iter().position(|c| c.name == from),
                    self.config.categories.iter().position(|c| c.name == to),
                );
                if let (Some(a), Some(b)) = positions {
                    self.config.move_category(a, b);
                    self.save_config();
                    self.refresh_view();
                }
            }
        }
    }

    fn delete_category(&mut self, ctx: &Context, name: &str) {
        let moved = self.entries.iter().filter(|e| e.category == name).count();
        self.config.delete_category(name);
        self.save_config();
        self.rebuild();
        self.toast(
            format!(
                "Deleted {name}; {moved} app{} moved to {UNCATEGORIZED}",
                if moved == 1 { "" } else { "s" }
            ),
            false,
        );
        let _ = ctx;
    }

    fn apply_settings_event(&mut self, ctx: &Context, event: SettingsEvent) {
        match event {
            SettingsEvent::Changed { rescan } => {
                self.ensure_categories();
                self.save_config();
                self.rebuild();
                if rescan {
                    self.start_scan(ctx);
                }
            }
            SettingsEvent::AddFolder => self.pick(ctx, PickKind::Folder),
            SettingsEvent::AddApp => self.pick(ctx, PickKind::App),
            SettingsEvent::RenameCategory(name) => {
                self.modal = Some(Modal::RenameCategory {
                    text: name.clone(),
                    old: name,
                });
            }
            SettingsEvent::DeleteCategory(name) => self.delete_category(ctx, &name),
            SettingsEvent::Restore(id) => {
                self.config.override_mut(&id).hidden = false;
                self.save_config();
                self.rebuild();
            }
            SettingsEvent::Autostart(enabled) => match autostart::set_enabled(enabled) {
                Ok(()) => {
                    self.config.settings.start_with_windows = enabled;
                    self.save_config();
                }
                Err(error) => self.toast(
                    format!("Could not change start with Windows: {error}"),
                    true,
                ),
            },
            SettingsEvent::ClearIcons => {
                let count = self.icons.clear_cache();
                self.toast(
                    format!(
                        "Cleared {count} cached icon{}",
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                );
            }
            SettingsEvent::ResetConfig => {
                let start_with_windows = autostart::is_enabled();
                self.config = Config::default();
                self.config.settings.start_with_windows = start_with_windows;
                self.register_shortcuts_folder();
                self.tab = Tab::All;
                self.save_config();
                self.rebuild();
                self.start_scan(ctx);
                self.toast("Configuration reset", false);
            }
            SettingsEvent::OpenShortcuts => {
                let dir = config::shortcuts_dir();
                if let Err(error) = std::fs::create_dir_all(&dir) {
                    self.toast(
                        format!("Could not create the shortcuts folder: {error}"),
                        true,
                    );
                } else if let Err(error) = launch::open_folder(&dir) {
                    self.toast(error, true);
                }
            }
            SettingsEvent::OpenDataFolder => {
                if let Err(error) = launch::open_folder(&config::data_dir()) {
                    self.toast(error, true);
                }
            }
        }
    }

    fn hidden_apps(&self) -> Vec<HiddenApp> {
        self.entries
            .iter()
            .filter(|e| e.hidden)
            .map(|e| HiddenApp {
                id: e.id.clone(),
                name: e.name.clone(),
            })
            .collect()
    }

    fn category_names(&self) -> Vec<String> {
        std::iter::once(UNCATEGORIZED.to_owned())
            .chain(self.config.categories.iter().map(|c| c.name.clone()))
            .collect()
    }

    fn tab_items(&self) -> Vec<TabItem> {
        let visible = || self.entries.iter().filter(|e| !e.hidden);
        let mut items = vec![TabItem {
            tab: Tab::All,
            label: "All".into(),
            count: visible().count(),
            editable: false,
        }];
        if self.config.settings.special_tabs {
            items.push(TabItem {
                tab: Tab::Favorites,
                label: "Favorites".into(),
                count: visible().filter(|e| e.favorite).count(),
                editable: false,
            });
            items.push(TabItem {
                tab: Tab::Recent,
                label: "Recent".into(),
                count: visible()
                    .filter(|e| e.last_launched.is_some())
                    .count()
                    .min(RECENT_LIMIT),
                editable: false,
            });
        }
        for category in &self.config.categories {
            items.push(TabItem {
                tab: Tab::Category(category.name.clone()),
                label: category.name.clone(),
                count: visible().filter(|e| e.category == category.name).count(),
                editable: true,
            });
        }
        let loose = visible().filter(|e| e.category == UNCATEGORIZED).count();
        if loose > 0 || self.tab == Tab::Category(UNCATEGORIZED.to_owned()) {
            items.push(TabItem {
                tab: Tab::Category(UNCATEGORIZED.to_owned()),
                label: UNCATEGORIZED.into(),
                count: loose,
                editable: false,
            });
        }
        items
    }
}

#[derive(Clone, Copy)]
enum PickKind {
    Folder,
    App,
    Icon,
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

impl App {
    fn top_bar(&mut self, ui: &mut Ui) {
        let mut toggle_settings = false;
        let mut refresh = false;
        let mut view_changed = false;
        let mut search_changed = false;
        let scanning = self.scan_rx.is_some();
        let (first, second) = split_name("ApolloLauncher");
        let accent = theme::accent_color(theme::default_accent(0));
        let ctx = ui.ctx().clone();
        let size = vec2(34.0, 34.0);

        Panel::top("top-bar")
            .exact_size(58.0)
            .frame(
                Frame::NONE
                    .fill(BG)
                    .inner_margin(Margin::symmetric(SIDE_PADDING as i8, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let available = ui.available_width();
                    let compact = available < 800.0;
                    let tiny = available < 560.0;
                    let logo_size = if compact { 18.0 } else { 24.0 };
                    let sort_width = if tiny {
                        80.0
                    } else if compact {
                        110.0
                    } else {
                        150.0
                    };
                    // Logo, three buttons, gaps, sort box, the SORT label and the clear button
                    // are measured first so the search field takes only what is left.
                    let reserved = logo_size * 8.5
                        + 3.0 * size.x
                        + 48.0
                        + sort_width
                        + if compact { 0.0 } else { 45.0 }
                        + if self.search.is_empty() {
                            0.0
                        } else {
                            size.x + 8.0
                        };
                    let search_width = (available - reserved).clamp(60.0, 260.0);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(
                        RichText::new(first)
                            .font(theme::small(logo_size))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(second)
                            .font(theme::small(logo_size))
                            .color(accent),
                    );
                    ui.spacing_mut().item_spacing.x = 8.0;

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        let gear = glyph_button(
                            ui,
                            rect,
                            "settings",
                            self.settings.open,
                            |p, c, color| {
                                glyphs::gear(p, c, color);
                            },
                        );
                        if gear.on_hover_text("Settings").clicked() {
                            toggle_settings = true;
                        }

                        let list = self.config.settings.view == ViewMode::List;
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        let toggle = glyph_button(ui, rect, "view", false, |p, c, color| {
                            if list {
                                glyphs::grid(p, c, color);
                            } else {
                                glyphs::list(p, c, color);
                            }
                        });
                        if toggle
                            .on_hover_text(if list { "Grid view" } else { "List view" })
                            .clicked()
                        {
                            self.config.settings.view =
                                if list { ViewMode::Grid } else { ViewMode::List };
                            view_changed = true;
                        }

                        ui.add_space(8.0);
                        let field = ui.add_sized(
                            [search_width, 34.0],
                            TextEdit::singleline(&mut self.search)
                                .id(Id::unique(SEARCH_ID))
                                .hint_text("Search apps\u{2026}")
                                .align(Align2::LEFT_CENTER)
                                .margin(Margin::symmetric(10, 0)),
                        );
                        if field.changed() {
                            search_changed = true;
                        }
                        if !self.search.is_empty() {
                            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                            let clear =
                                glyph_button(ui, rect, "clear-search", false, |p, c, color| {
                                    glyphs::close(p, c, 5.0, color);
                                });
                            if clear.on_hover_text("Clear search (Esc)").clicked() {
                                self.search.clear();
                                search_changed = true;
                            }
                        }

                        ui.add_space(8.0);
                        let sort = self.config.settings.sort;
                        ComboBox::from_id_salt("sort")
                            .selected_text(sort.label())
                            .width(sort_width)
                            .show_ui(ui, |ui| {
                                for mode in SortMode::ALL {
                                    if ui.selectable_label(sort == mode, mode.label()).clicked()
                                        && sort != mode
                                    {
                                        self.config.settings.sort = mode;
                                        view_changed = true;
                                    }
                                }
                            });
                        if !compact {
                            ui.label(RichText::new("SORT").size(11.5).color(TEXT_SECONDARY));
                        }

                        ui.add_space(8.0);
                        let angle = if scanning {
                            ctx.request_repaint();
                            (ctx.input(|i| i.time) as f32 * 6.0) % std::f32::consts::TAU
                        } else {
                            0.0
                        };
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        let button = glyph_button(ui, rect, "refresh", scanning, |p, c, color| {
                            glyphs::refresh(p, c, color, angle);
                        });
                        if button.on_hover_text("Rescan sources (F5)").clicked() {
                            refresh = true;
                        }
                    });
                });
            });

        if toggle_settings {
            self.settings.open = !self.settings.open;
        }
        if view_changed {
            self.save_config();
        }
        if view_changed || search_changed {
            self.refresh_view();
        }
        if refresh {
            self.start_scan(&ctx);
        }
    }

    fn tabs_bar(&mut self, ui: &mut Ui) -> Vec<TabAction> {
        let items = self.tab_items();
        let mut actions = Vec::new();
        Panel::top("tabs-bar")
            .exact_size(tabs::TAB_HEIGHT)
            .frame(
                Frame::NONE
                    .fill(BG)
                    .inner_margin(Margin::symmetric(SIDE_PADDING as i8, 0)),
            )
            .show(ui, |ui| {
                actions = tabs::show(ui, &items, &self.tab);
            });
        actions
    }

    fn library(&mut self, ui: &mut Ui) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.view.is_empty() {
            self.empty_state(ui);
            return actions;
        }
        let now = now_secs();
        let categories = self.category_names();
        let list = self.config.settings.view == ViewMode::List;
        let entries = &self.entries;
        let view = &self.view;
        let icons = &mut self.icons;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let available = ui.available_width();
                let inner = (available - 2.0 * SIDE_PADDING).max(120.0);
                let gap = if list { 6.0 } else { card::GAP };
                let (columns, item_h) = if list {
                    (1, ROW_HEIGHT)
                } else {
                    (card::columns(inner), card::CARD_HEIGHT)
                };
                let item_w = (inner - gap * (columns as f32 - 1.0)) / columns as f32;
                let rows = view.len().div_ceil(columns);
                let total = rows as f32 * (item_h + gap) + gap;
                let (area, _) = ui.allocate_exact_size(vec2(available, total), Sense::hover());

                let stride = item_h + gap;
                let first = (((viewport.min.y - gap) / stride).floor().max(0.0)) as usize;
                let last = (((viewport.max.y / stride).ceil() as usize) + 1).min(rows);
                for row in first..last {
                    for col in 0..columns {
                        let Some(&index) = view.get(row * columns + col) else {
                            break;
                        };
                        let entry = &entries[index];
                        let rect = egui::Rect::from_min_size(
                            pos2(
                                area.left() + SIDE_PADDING + col as f32 * (item_w + gap),
                                area.top() + gap + row as f32 * stride,
                            ),
                            vec2(item_w, item_h),
                        );
                        let slot = categories
                            .iter()
                            .position(|c| *c == entry.category)
                            .unwrap_or(0);
                        let accent = theme::accent_color(
                            entry.accent.unwrap_or_else(|| theme::default_accent(slot)),
                        );
                        let icon_pending = icons.is_pending(&entry.id);
                        let icon = icons.get(entry);
                        let entry_view = EntryView {
                            entry,
                            icon,
                            icon_pending,
                            accent,
                            categories: &categories,
                            now,
                        };
                        let action = if list {
                            list_row::show(ui, rect, &entry_view)
                        } else {
                            card::show(ui, rect, &entry_view)
                        };
                        actions.extend(action);
                    }
                }
            });
        actions
    }

    fn empty_state(&mut self, ui: &mut Ui) {
        let searching = !self.search.trim().is_empty();
        let no_apps = self.entries.iter().all(|e| e.hidden);
        let (title, body) = if searching {
            (
                "No results",
                format!("Nothing matches \u{201c}{}\u{201d}.", self.search.trim()),
            )
        } else if no_apps && self.config.sources.is_empty() {
            (
                "No apps yet",
                "Add a folder of games or programs to scan, or drop an .exe or .lnk file here."
                    .to_owned(),
            )
        } else if no_apps && !self.scanned_once {
            (
                "Scanning\u{2026}",
                "Looking through your sources.".to_owned(),
            )
        } else if no_apps {
            (
                "No apps found",
                "Your sources contain no programs or shortcuts.".to_owned(),
            )
        } else {
            match &self.tab {
                Tab::Favorites => (
                    "No favorites",
                    "Use the ... menu on an app to add it here.".to_owned(),
                ),
                Tab::Recent => (
                    "Nothing launched yet",
                    "Apps you open appear here.".to_owned(),
                ),
                _ => (
                    "This tab is empty",
                    "Drag a card onto the tab to move an app here.".to_owned(),
                ),
            }
        };
        let offer_add = !searching && no_apps && self.scanned_once;
        let ctx = ui.ctx().clone();
        let mut add_folder = false;
        let mut add_app = false;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(40.0));
            ui.label(RichText::new(title).font(theme::small(22.0)).color(TEXT));
            ui.add_space(6.0);
            ui.label(RichText::new(body).color(TEXT_SECONDARY));
            if offer_add {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() / 2.0 - 110.0).max(0.0));
                    add_folder = ui.button("Add folder\u{2026}").clicked();
                    add_app = ui.button("Add app\u{2026}").clicked();
                });
            }
        });
        if add_folder {
            self.pick(&ctx, PickKind::Folder);
        }
        if add_app {
            self.pick(&ctx, PickKind::App);
        }
    }

    fn drop_overlay(&self, ctx: &Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.content_rect().shrink(10.0);
        let painter = ctx.layer_painter(egui::LayerId::new(
            Order::Foreground,
            Id::unique("drop-overlay"),
        ));
        let accent = theme::accent_color(theme::default_accent(0));
        painter.rect(
            rect,
            0.0,
            BG.gamma_multiply(0.85),
            Stroke::new(2.0, accent),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drop an .exe or .lnk to add it",
            theme::small(22.0),
            TEXT,
        );
    }

    fn toasts_ui(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        for toast in &mut self.toasts {
            if toast.created.is_nan() {
                toast.created = now;
            }
        }
        self.toasts.retain(|t| {
            let life = if t.error {
                ERROR_TOAST_SECONDS
            } else {
                TOAST_SECONDS
            };
            now - t.created < life
        });
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        egui::Area::new(Id::unique("toasts"))
            .order(Order::Tooltip)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-20.0, -20.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                for toast in &self.toasts {
                    let stroke = Stroke::new(1.0, if toast.error { DANGER } else { BORDER });
                    Frame::NONE
                        .fill(CARD)
                        .stroke(stroke)
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&toast.text).color(if toast.error {
                                DANGER
                            } else {
                                TEXT
                            }));
                        });
                    ui.add_space(6.0);
                }
            });
    }

    fn modals(&mut self, ctx: &Context) {
        enum Outcome {
            Keep,
            Close,
            Create(String),
            Rename(String, String),
        }
        let Some(modal) = &mut self.modal else { return };
        let (title, text, old): (&str, &mut String, Option<String>) = match modal {
            Modal::NewCategory(text) => ("New category", text, None),
            Modal::RenameCategory { old, text } => ("Rename category", text, Some(old.clone())),
        };
        let mut outcome = Outcome::Keep;
        let mut open = true;
        egui::Window::new(title)
            .id(Id::unique("category-modal"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(320.0, 0.0))
            .show(ctx, |ui| {
                let field = ui.add(
                    TextEdit::singleline(text)
                        .desired_width(f32::INFINITY)
                        .hint_text("Name"),
                );
                if !field.has_focus() && !field.lost_focus() {
                    field.request_focus();
                }
                let submit = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if old.is_some() { "Rename" } else { "Create" };
                    if ui.button(label).clicked() || submit {
                        outcome = match &old {
                            Some(old) => Outcome::Rename(old.clone(), text.trim().to_owned()),
                            None => Outcome::Create(text.trim().to_owned()),
                        };
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = Outcome::Close;
                    }
                });
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            outcome = Outcome::Close;
        }
        match outcome {
            Outcome::Keep => {}
            Outcome::Close => self.modal = None,
            Outcome::Create(name) => {
                if name.is_empty() {
                    self.toast("Enter a name for the category", true);
                } else if self.config.add_category(&name) {
                    self.modal = None;
                    self.save_config();
                    self.tab = Tab::Category(name);
                    self.refresh_view();
                } else {
                    self.toast(
                        format!("A category named \u{201c}{name}\u{201d} already exists"),
                        true,
                    );
                }
            }
            Outcome::Rename(old, new) => {
                if new.is_empty() {
                    self.toast("Enter a name for the category", true);
                } else if new == old {
                    self.modal = None;
                } else if self.config.rename_category(&old, &new) {
                    self.modal = None;
                    if self.tab == Tab::Category(old) {
                        self.tab = Tab::Category(new);
                    }
                    self.save_config();
                    self.rebuild();
                } else {
                    self.toast(
                        format!("A category named \u{201c}{new}\u{201d} already exists"),
                        true,
                    );
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            self.add_manual(ctx, &path);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        let (find, escape, refresh) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::F5),
            )
        });
        if find {
            self.focus_search = true;
        }
        if self.focus_search {
            ctx.memory_mut(|m| m.request_focus(Id::unique(SEARCH_ID)));
            self.focus_search = false;
        }
        let dialog_open = self.edit.is_some() || self.modal.is_some() || self.settings.open;
        if escape && !dialog_open && !self.search.is_empty() {
            self.search.clear();
            self.refresh_view();
        }
        if refresh {
            self.start_scan(ctx);
        }
    }

    fn edit_dialog(&mut self, ctx: &Context) {
        let Some(state) = &mut self.edit else { return };
        match edit::show(ctx, state) {
            Some(EditEvent::Save) => {
                if let Some(state) = self.edit.take() {
                    self.save_edit(state);
                }
            }
            Some(EditEvent::Cancel) => self.edit = None,
            Some(EditEvent::BrowseIcon) => self.pick(ctx, PickKind::Icon),
            None => {}
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_scan(ctx);
        self.poll_picker(ctx);
        self.icons.poll(ctx);
        if self.watcher.take_changed() {
            self.start_scan(ctx);
        }
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);
        if self.close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.top_bar(ui);
        let tab_actions = self.tabs_bar(ui);
        let mut actions = Vec::new();
        CentralPanel::default()
            .frame(Frame::NONE.fill(BG))
            .show(ui, |ui| {
                actions = self.library(ui);
            });

        if let Some(payload) = DragAndDrop::payload::<DragApp>(&ctx)
            && let Some(entry) = self.entry_index(&payload.0).map(|i| &self.entries[i])
        {
            card::paint_drag_ghost(ui, &entry.name);
        }

        let config_path = config::config_path().display().to_string();
        let hidden = self.hidden_apps();
        let unreadable = self.unreadable.clone();
        let events = settings::show(
            &ctx,
            &mut self.settings,
            &mut self.config,
            &hidden,
            &unreadable,
            &config_path,
        );
        self.edit_dialog(&ctx);
        self.modals(&ctx);
        self.drop_overlay(&ctx);
        self.toasts_ui(&ctx);

        for action in tab_actions {
            self.apply_tab_action(&ctx, action);
        }
        for event in events {
            self.apply_settings_event(&ctx, event);
        }
        for action in actions {
            self.apply(&ctx, action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppEntry;

    fn entry(name: &str, description: &str, category: &str) -> AppEntry {
        let mut e = AppEntry {
            id: name.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            source_path: PathBuf::from(format!("C:\\x\\{name}.exe")),
            target: PathBuf::from(format!("C:\\x\\{name}.exe")),
            args: String::new(),
            work_dir: None,
            icon_location: None,
            category: category.to_owned(),
            custom_icon: None,
            accent: None,
            hidden: false,
            favorite: false,
            last_launched: None,
            launch_count: 0,
            manual: false,
            missing: false,
            name_key: normalize(name),
            search_key: String::new(),
        };
        e.search_key = normalize(&format!("{name} {description} {category}"));
        e
    }

    fn names(entries: &[AppEntry], view: &[usize]) -> Vec<String> {
        view.iter().map(|&i| entries[i].name.clone()).collect()
    }

    #[test]
    fn search_ignores_case_and_diacritics() {
        let e = entry("Pok\u{e9}mon Caf\u{e9}", "A cosy game", "Games");
        assert!(matches_query(&e, "pokemon"));
        assert!(matches_query(&e, "CAFE"));
        assert!(matches_query(&e, "cosy games"));
        assert!(matches_query(&e, ""));
        assert!(!matches_query(&e, "chess"));
    }

    #[test]
    fn search_matches_description_and_category() {
        let e = entry("Blender", "3D modelling suite", "Programs");
        assert!(matches_query(&e, "modelling"));
        assert!(matches_query(&e, "programs"));
    }

    #[test]
    fn sorts_by_every_mode() {
        let mut a = entry("Alpha", "", "Programs");
        let mut b = entry("beta", "", "Games");
        let mut c = entry("Gamma", "", "Games");
        a.launch_count = 1;
        b.launch_count = 5;
        a.last_launched = Some(300);
        c.last_launched = Some(100);
        let entries = vec![a, b, c];
        let sorted = |mode| {
            let mut idx: Vec<usize> = (0..entries.len()).collect();
            sort_indices(&entries, &mut idx, mode);
            names(&entries, &idx)
        };
        assert_eq!(sorted(SortMode::NameAsc), ["Alpha", "beta", "Gamma"]);
        assert_eq!(sorted(SortMode::NameDesc), ["Gamma", "beta", "Alpha"]);
        assert_eq!(sorted(SortMode::MostLaunched), ["beta", "Alpha", "Gamma"]);
        assert_eq!(sorted(SortMode::LastLaunched), ["Alpha", "Gamma", "beta"]);
        assert_eq!(sorted(SortMode::Category), ["beta", "Gamma", "Alpha"]);
    }

    #[test]
    fn view_filters_by_tab_search_and_hidden() {
        let mut a = entry("Alpha", "", "Games");
        let mut b = entry("Beta", "", "Programs");
        let mut c = entry("Gamma", "", "Games");
        a.favorite = true;
        b.last_launched = Some(10);
        c.hidden = true;
        let entries = vec![a, b, c];
        let view =
            |tab: Tab, q: &str| names(&entries, &build_view(&entries, &tab, q, SortMode::NameAsc));
        assert_eq!(view(Tab::All, ""), ["Alpha", "Beta"]);
        assert_eq!(view(Tab::Favorites, ""), ["Alpha"]);
        assert_eq!(view(Tab::Recent, ""), ["Beta"]);
        assert_eq!(view(Tab::Category("Games".into()), ""), ["Alpha"]);
        assert_eq!(view(Tab::All, "bet"), ["Beta"]);
    }

    #[test]
    fn recent_tab_orders_by_last_launch_and_is_capped() {
        let mut entries = Vec::new();
        for i in 0..30 {
            let mut e = entry(&format!("App{i:02}"), "", "Games");
            e.last_launched = Some(i);
            entries.push(e);
        }
        let view = build_view(&entries, &Tab::Recent, "", SortMode::NameAsc);
        assert_eq!(view.len(), RECENT_LIMIT);
        assert_eq!(entries[view[0]].name, "App29");
    }

    #[test]
    fn only_exe_and_lnk_are_launchable() {
        assert!(is_launchable_file(Path::new("C:\\a\\b.EXE")));
        assert!(is_launchable_file(Path::new("C:\\a\\b.lnk")));
        assert!(!is_launchable_file(Path::new("C:\\a\\b.txt")));
        assert!(!is_launchable_file(Path::new("C:\\a\\b")));
    }
}
