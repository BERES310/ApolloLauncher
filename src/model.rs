use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const UNCATEGORIZED: &str = "Uncategorized";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    #[default]
    NameAsc,
    NameDesc,
    LastLaunched,
    MostLaunched,
    Category,
}

impl SortMode {
    pub const ALL: [SortMode; 5] = [
        SortMode::NameAsc,
        SortMode::NameDesc,
        SortMode::LastLaunched,
        SortMode::MostLaunched,
        SortMode::Category,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortMode::NameAsc => "Name (A-Z)",
            SortMode::NameDesc => "Name (Z-A)",
            SortMode::LastLaunched => "Recently launched",
            SortMode::MostLaunched => "Most launched",
            SortMode::Category => "Category",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub view: ViewMode,
    pub sort: SortMode,
    pub close_on_launch: bool,
    pub scan_recursive: bool,
    pub scan_depth: u32,
    pub filter_noise: bool,
    pub special_tabs: bool,
    pub start_with_windows: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            view: ViewMode::Grid,
            sort: SortMode::NameAsc,
            close_on_launch: false,
            scan_recursive: false,
            scan_depth: 3,
            filter_noise: true,
            special_tabs: true,
            start_with_windows: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Source {
    pub path: PathBuf,
    pub default_category: String,
    pub include: Vec<String>,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            default_category: UNCATEGORIZED.to_owned(),
            include: vec!["lnk".to_owned()],
        }
    }
}

impl Source {
    pub fn new(path: PathBuf, default_category: &str) -> Self {
        Self {
            path,
            default_category: default_category.to_owned(),
            ..Self::default()
        }
    }

    pub fn includes(&self, extension: &str) -> bool {
        self.include
            .iter()
            .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(extension))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Category {
    pub name: String,
}

/// User edits and usage data for one app, keyed by id. The scanner never overwrites these.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct AppOverride {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub args: Option<String>,
    pub custom_icon: Option<PathBuf>,
    pub accent: Option<String>,
    pub hidden: bool,
    /// Removed from the launcher for good; the scanner skips the app.
    pub removed: bool,
    pub favorite: bool,
    pub last_launched: Option<i64>,
    pub launch_count: u32,
    /// Set for entries added by drag and drop or the file picker.
    pub manual: bool,
    pub source_path: Option<PathBuf>,
}

impl AppOverride {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            ..Self::default()
        }
    }

    /// True when nothing worth persisting is stored.
    pub fn is_empty(&self) -> bool {
        *self == Self::new(&self.id)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub settings: Settings,
    pub sources: Vec<Source>,
    pub categories: Vec<Category>,
    pub apps: Vec<AppOverride>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            sources: Vec::new(),
            categories: ["Games", "Programs"]
                .iter()
                .map(|name| Category {
                    name: (*name).to_owned(),
                })
                .collect(),
            apps: Vec::new(),
        }
    }
}

impl Config {
    pub fn override_for(&self, id: &str) -> Option<&AppOverride> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// Returns the editable override for `id`, creating it if needed.
    pub fn override_mut(&mut self, id: &str) -> &mut AppOverride {
        if let Some(index) = self.apps.iter().position(|a| a.id == id) {
            return &mut self.apps[index];
        }
        self.apps.push(AppOverride::new(id));
        let last = self.apps.len() - 1;
        &mut self.apps[last]
    }

    pub fn has_category(&self, name: &str) -> bool {
        self.categories
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Adds a category; returns false for empty or duplicate names.
    pub fn add_category(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case(UNCATEGORIZED) || self.has_category(name) {
            return false;
        }
        self.categories.push(Category {
            name: name.to_owned(),
        });
        true
    }

    pub fn rename_category(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        let clash = new.is_empty()
            || new.eq_ignore_ascii_case(UNCATEGORIZED)
            || (!new.eq_ignore_ascii_case(old) && self.has_category(new));
        if clash {
            return false;
        }
        let Some(category) = self.categories.iter_mut().find(|c| c.name == old) else {
            return false;
        };
        category.name = new.to_owned();
        for app in &mut self.apps {
            if app.category.as_deref() == Some(old) {
                app.category = Some(new.to_owned());
            }
        }
        for source in &mut self.sources {
            if source.default_category == old {
                source.default_category = new.to_owned();
            }
        }
        true
    }

    /// Removes a category; its apps and sources fall back to Uncategorized.
    pub fn delete_category(&mut self, name: &str) {
        self.categories.retain(|c| c.name != name);
        for app in &mut self.apps {
            if app.category.as_deref() == Some(name) {
                app.category = Some(UNCATEGORIZED.to_owned());
            }
        }
        for source in &mut self.sources {
            if source.default_category == name {
                source.default_category = UNCATEGORIZED.to_owned();
            }
        }
    }

    pub fn move_category(&mut self, from: usize, to: usize) {
        if from < self.categories.len() && to < self.categories.len() && from != to {
            let item = self.categories.remove(from);
            self.categories.insert(to, item);
        }
    }

    /// Drops overrides that carry no information.
    pub fn prune_overrides(&mut self) {
        self.apps.retain(|a| !a.is_empty());
    }
}

/// A fully merged app as shown in the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source_path: PathBuf,
    pub target: PathBuf,
    pub args: String,
    pub work_dir: Option<PathBuf>,
    pub icon_location: Option<(PathBuf, i32)>,
    pub category: String,
    pub custom_icon: Option<PathBuf>,
    pub accent: Option<[u8; 3]>,
    pub hidden: bool,
    pub favorite: bool,
    pub last_launched: Option<i64>,
    pub launch_count: u32,
    pub manual: bool,
    pub missing: bool,
    pub name_key: String,
    pub search_key: String,
}
