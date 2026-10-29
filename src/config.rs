use crate::model::Config;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DATA_DIR_ENV: &str = "APOLLOLAUNCHER_DATA_DIR";

/// Portable: the `data` folder next to the exe, or the directory named by `APOLLOLAUNCHER_DATA_DIR`.
/// Falls back to `%APPDATA%\ApolloLauncher` only when the exe location cannot be determined.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data")))
    {
        return dir;
    }
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("ApolloLauncher"))
        .unwrap_or_else(|| std::env::temp_dir().join("ApolloLauncher"))
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

/// The folder ApolloLauncher owns for the user's shortcuts; it is the only source on a fresh install.
pub fn shortcuts_dir() -> PathBuf {
    data_dir().join("Shortcuts")
}

pub fn icon_cache_dir() -> PathBuf {
    data_dir().join("icons")
}

pub struct LoadOutcome {
    pub config: Config,
    pub warning: Option<String>,
}

pub fn load() -> LoadOutcome {
    load_from(&config_path())
}

/// Reads the config; a missing file gives defaults, a corrupt file is set aside and replaced by defaults.
pub fn load_from(path: &Path) -> LoadOutcome {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return LoadOutcome {
                config: Config::default(),
                warning: None,
            };
        }
        Err(e) => {
            return LoadOutcome {
                config: Config::default(),
                warning: Some(format!("Could not read config: {e}. Using defaults.")),
            };
        }
    };
    match toml::from_str::<Config>(&text) {
        Ok(mut config) => {
            sanitize(&mut config);
            LoadOutcome {
                config,
                warning: None,
            }
        }
        Err(e) => {
            let backup = path.with_extension("toml.corrupt");
            let moved = fs::rename(path, &backup).is_ok();
            let reason = first_line(&e.to_string());
            let note = if moved {
                format!(
                    "Config was unreadable ({reason}). A backup was kept as {}.",
                    backup.display()
                )
            } else {
                format!("Config was unreadable ({reason}). Using defaults.")
            };
            LoadOutcome {
                config: Config::default(),
                warning: Some(note),
            }
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("parse error").to_owned()
}

fn sanitize(config: &mut Config) {
    config.settings.scan_depth = config.settings.scan_depth.clamp(1, 12);
    config.sources.retain(|s| !s.path.as_os_str().is_empty());
    let mut seen: Vec<String> = Vec::new();
    config.categories.retain(|c| {
        let key = c.name.trim().to_lowercase();
        let keep = !key.is_empty() && !seen.contains(&key);
        seen.push(key);
        keep
    });
    let mut ids: Vec<String> = Vec::new();
    config.apps.retain(|a| {
        let keep = !a.id.is_empty() && !ids.contains(&a.id);
        ids.push(a.id.clone());
        keep
    });
}

pub fn save(config: &Config) -> io::Result<()> {
    save_to(config, &config_path())
}

/// Writes through a temporary file so a crash never leaves a half-written config.
pub fn save_to(config: &Config, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(config).map_err(io::Error::other)?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppOverride, Source, UNCATEGORIZED, ViewMode};

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("apollolauncher-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        let outcome = load_from(&dir.join("config.toml"));
        assert_eq!(outcome.config, Config::default());
        assert!(outcome.warning.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("nested").join("config.toml");
        let mut config = Config::default();
        config
            .sources
            .push(Source::new(PathBuf::from(r"C:\Games"), "Games"));
        let entry = config.override_mut("abc");
        entry.name = Some("Renamed".into());
        entry.favorite = true;
        entry.launch_count = 4;
        save_to(&config, &path).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.config, config);
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn corrupt_file_falls_back_and_is_kept() {
        let dir = temp_dir("corrupt");
        let path = dir.join("config.toml");
        fs::write(&path, "this is [[ not valid toml ===").unwrap();
        let outcome = load_from(&path);
        assert_eq!(outcome.config, Config::default());
        assert!(outcome.warning.is_some());
        assert!(dir.join("config.toml.corrupt").exists());
    }

    #[test]
    fn partial_file_fills_in_defaults() {
        let dir = temp_dir("partial");
        let path = dir.join("config.toml");
        fs::write(
            &path,
            "[settings]\nview = \"list\"\n\n[[apps]]\nid = \"x\"\nname = \"X\"\n",
        )
        .unwrap();
        let outcome = load_from(&path);
        assert_eq!(outcome.config.settings.view, ViewMode::List);
        assert!(!outcome.config.settings.scan_recursive);
        assert_eq!(outcome.config.apps.len(), 1);
        assert_eq!(outcome.config.categories.len(), 2);
    }

    #[test]
    fn wrong_types_count_as_corruption() {
        let dir = temp_dir("types");
        let path = dir.join("config.toml");
        fs::write(&path, "[settings]\nscan_depth = \"deep\"\n").unwrap();
        assert!(load_from(&path).warning.is_some());
    }

    #[test]
    fn category_helpers_keep_references_consistent() {
        let mut config = Config::default();
        config
            .sources
            .push(Source::new(PathBuf::from("a"), "Games"));
        config.override_mut("1").category = Some("Games".into());
        assert!(config.rename_category("Games", "Play"));
        assert_eq!(config.sources[0].default_category, "Play");
        assert_eq!(
            config.override_for("1").unwrap().category.as_deref(),
            Some("Play")
        );
        config.delete_category("Play");
        assert_eq!(config.sources[0].default_category, UNCATEGORIZED);
        assert_eq!(
            config.override_for("1").unwrap().category.as_deref(),
            Some(UNCATEGORIZED)
        );
        assert!(!config.add_category("programs"));
        assert!(!config.add_category("  "));
        assert!(config.add_category("Tools"));
        config.move_category(0, 1);
        assert_eq!(config.categories[1].name, "Programs");
    }

    #[test]
    fn empty_overrides_are_pruned() {
        let mut config = Config::default();
        config.apps.push(AppOverride::new("a"));
        config.override_mut("b").hidden = true;
        config.prune_overrides();
        assert_eq!(config.apps.len(), 1);
        assert_eq!(config.apps[0].id, "b");
    }
}
