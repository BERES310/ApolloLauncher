use crate::model::{AppEntry, Config, Settings, Source, UNCATEGORIZED};
use crate::util::{app_id, file_stem, normalize, parse_hex_color};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{Interface, PCWSTR};

/// One shortcut or executable found on disk, before user overrides are applied.
#[derive(Clone, Debug, PartialEq)]
pub struct ScannedItem {
    pub source_path: PathBuf,
    /// Resolved target; empty when a shortcut has no file target (the shortcut itself is launched).
    pub target: PathBuf,
    pub args: String,
    pub work_dir: Option<PathBuf>,
    pub icon_location: Option<(PathBuf, i32)>,
    pub description: String,
    pub default_category: String,
    pub from_lnk: bool,
    pub missing: bool,
    pub manual: bool,
    /// Id saved in the config for a manually added app. It keeps the card tied to its saved entry
    /// even when the shortcut moves, is retargeted or is deleted, so the entry can still be removed.
    pub stored_id: Option<String>,
}

impl ScannedItem {
    pub fn id(&self) -> String {
        if let Some(id) = &self.stored_id {
            return id.clone();
        }
        if self.target.as_os_str().is_empty() {
            app_id(&self.source_path, &self.args)
        } else {
            app_id(&self.target, &self.args)
        }
    }
}

#[derive(Default)]
pub struct ScanResult {
    pub items: Vec<ScannedItem>,
    /// Source folders that could not be read (missing, no access).
    pub unreadable_sources: Vec<PathBuf>,
}

/// Initialises COM for the calling thread. Safe to call repeatedly.
pub fn init_com() {
    // SAFETY: plain COM initialisation for this thread; a repeated or mismatched call only returns an error code, which is ignored.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

/// Expands `%VAR%` references; unknown variables are left untouched.
pub fn expand_env(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => match std::env::var(&after[..end]) {
                Ok(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                Err(_) => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

struct LnkData {
    target: String,
    args: String,
    work_dir: String,
    icon_path: String,
    icon_index: i32,
    description: String,
}

fn read_lnk(path: &Path) -> Option<LnkData> {
    // SAFETY: COM calls on interfaces created in this function; buffers outlive the calls that fill them.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let wide_path = wide(path);
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ).ok()?;

        let mut target = [0u16; 1024];
        let mut args = [0u16; 2048];
        let mut work_dir = [0u16; 1024];
        let mut icon = [0u16; 1024];
        let mut description = [0u16; 1024];
        let mut icon_index = 0i32;
        let _ = link.GetPath(&mut target, std::ptr::null_mut(), 0);
        let _ = link.GetArguments(&mut args);
        let _ = link.GetWorkingDirectory(&mut work_dir);
        let _ = link.GetIconLocation(&mut icon, &mut icon_index);
        let _ = link.GetDescription(&mut description);
        Some(LnkData {
            target: from_wide(&target),
            args: from_wide(&args),
            work_dir: from_wide(&work_dir),
            icon_path: from_wide(&icon),
            icon_index,
            description: from_wide(&description),
        })
    }
}

/// Builds a scanned item for a single `.lnk` or `.exe` path. Returns None for unreadable shortcuts.
pub fn resolve_item(path: &Path, default_category: &str) -> Option<ScannedItem> {
    let is_lnk = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
    if !is_lnk {
        return Some(ScannedItem {
            source_path: path.to_path_buf(),
            target: path.to_path_buf(),
            args: String::new(),
            work_dir: None,
            icon_location: None,
            description: String::new(),
            default_category: default_category.to_owned(),
            from_lnk: false,
            missing: !path.exists(),
            manual: false,
            stored_id: None,
        });
    }
    let data = read_lnk(path)?;
    let target = PathBuf::from(expand_env(&data.target));
    let missing = !target.as_os_str().is_empty() && !target.exists();
    let non_empty = |s: &str| (!s.trim().is_empty()).then(|| PathBuf::from(expand_env(s.trim())));
    let icon_location = non_empty(&data.icon_path).map(|p| (p, data.icon_index));
    Some(ScannedItem {
        source_path: path.to_path_buf(),
        target,
        args: data.args.trim().to_owned(),
        work_dir: non_empty(&data.work_dir),
        icon_location,
        description: data.description.trim().to_owned(),
        default_category: default_category.to_owned(),
        from_lnk: true,
        missing,
        manual: false,
        stored_id: None,
    })
}

/// True for installer, uninstaller and crash-reporter executables that are never wanted as apps.
pub fn is_noise(file_name: &str) -> bool {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let numbered_unins = stem
        .strip_prefix("unins")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    numbered_unins
        || stem.starts_with("uninstall")
        || stem.starts_with("crashhandler")
        || stem.starts_with("vcredist")
        || stem.contains("setup")
}

fn passes_noise_filter(item: &ScannedItem) -> bool {
    let name_noise = item
        .source_path
        .file_name()
        .is_some_and(|n| is_noise(&n.to_string_lossy()));
    let target_noise = item.from_lnk
        && item
            .target
            .file_name()
            .is_some_and(|n| is_noise(&n.to_string_lossy()));
    !(name_noise || target_noise)
}

fn scan_source(source: &Source, settings: &Settings, out: &mut Vec<ScannedItem>) -> bool {
    if std::fs::read_dir(&source.path).is_err() {
        return false;
    }
    let depth = if settings.scan_recursive {
        settings.scan_depth.max(1) as usize
    } else {
        1
    };
    for entry in WalkDir::new(&source.path)
        .max_depth(depth)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(extension) = path.extension().map(|e| e.to_string_lossy().into_owned()) else {
            continue;
        };
        let extension = extension.to_lowercase();
        if (extension != "lnk" && extension != "exe") || !source.includes(&extension) {
            continue;
        }
        let Some(item) = resolve_item(path, &source.default_category) else {
            continue;
        };
        if settings.filter_noise && !passes_noise_filter(&item) {
            continue;
        }
        out.push(item);
    }
    true
}

/// Scans every source and resolves manually added entries. Runs on a worker thread.
pub fn scan(config: &Config) -> ScanResult {
    init_com();
    let mut result = ScanResult::default();
    for source in &config.sources {
        if !scan_source(source, &config.settings, &mut result.items) {
            result.unreadable_sources.push(source.path.clone());
        }
    }
    for entry in config.apps.iter().filter(|a| a.manual) {
        let Some(path) = &entry.source_path else {
            continue;
        };
        let category = entry.category.as_deref().unwrap_or(UNCATEGORIZED);
        let mut item = resolve_item(path, category).unwrap_or_else(|| ScannedItem {
            source_path: path.clone(),
            target: path.clone(),
            args: String::new(),
            work_dir: None,
            icon_location: None,
            description: String::new(),
            default_category: category.to_owned(),
            from_lnk: false,
            missing: true,
            manual: true,
            stored_id: None,
        });
        item.manual = true;
        item.stored_id = Some(entry.id.clone());
        result.items.push(item);
    }
    result
}

/// Merges scan results with user overrides. Duplicates (same target and args) collapse into one
/// card, preferring a shortcut over a bare executable and earlier sources over later ones.
pub fn build_entries(items: &[ScannedItem], config: &Config) -> Vec<AppEntry> {
    let mut chosen: Vec<(String, &ScannedItem)> = Vec::new();
    let mut index_by_id: HashMap<String, usize> = HashMap::new();
    for item in items {
        let id = item.id();
        match index_by_id.get(&id) {
            None => {
                index_by_id.insert(id.clone(), chosen.len());
                chosen.push((id, item));
            }
            Some(&at) => {
                let current = chosen[at].1;
                let better =
                    (item.from_lnk && !current.from_lnk) || (item.manual && !current.manual);
                if better {
                    chosen[at].1 = item;
                }
            }
        }
    }
    chosen
        .into_iter()
        .filter(|(id, _)| !config.override_for(id).is_some_and(|o| o.removed))
        .map(|(id, item)| entry_from(id, item, config))
        .collect()
}

fn entry_from(id: String, item: &ScannedItem, config: &Config) -> AppEntry {
    let over = config.override_for(&id);
    let name = over
        .and_then(|o| o.name.clone())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| file_stem(&item.source_path));
    let description = over
        .and_then(|o| o.description.clone())
        .unwrap_or_else(|| item.description.clone());
    let requested = over
        .and_then(|o| o.category.clone())
        .unwrap_or_else(|| item.default_category.clone());
    let category = if requested == UNCATEGORIZED || config.has_category(&requested) {
        requested
    } else {
        UNCATEGORIZED.to_owned()
    };
    let args = over
        .and_then(|o| o.args.clone())
        .unwrap_or_else(|| item.args.clone());
    let search_key = normalize(&format!("{name} {description} {category}"));
    AppEntry {
        id,
        name_key: normalize(&name),
        search_key,
        name,
        description,
        source_path: item.source_path.clone(),
        target: item.target.clone(),
        args,
        work_dir: item.work_dir.clone(),
        icon_location: item.icon_location.clone(),
        category,
        custom_icon: over.and_then(|o| o.custom_icon.clone()),
        accent: over
            .and_then(|o| o.accent.as_deref())
            .and_then(parse_hex_color),
        hidden: over.is_some_and(|o| o.hidden),
        favorite: over.is_some_and(|o| o.favorite),
        last_launched: over.and_then(|o| o.last_launched),
        launch_count: over.map_or(0, |o| o.launch_count),
        manual: item.manual,
        missing: item.missing,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs;
    use windows::Win32::System::Com::IPersistFile;

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("apollolauncher-scan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes a small placeholder file; the scanner only looks at names and shortcut data.
    pub fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"MZ").unwrap();
    }

    pub fn make_lnk(lnk: &Path, target: &Path, args: &str, work_dir: Option<&Path>) {
        init_com();
        if let Some(parent) = lnk.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // SAFETY: COM objects created and used within this function only.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let target_w = wide(target);
            link.SetPath(PCWSTR(target_w.as_ptr())).unwrap();
            let args_w = wide(Path::new(args));
            link.SetArguments(PCWSTR(args_w.as_ptr())).unwrap();
            if let Some(dir) = work_dir {
                let dir_w = wide(dir);
                link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr())).unwrap();
            }
            let persist: IPersistFile = link.cast().unwrap();
            let lnk_w = wide(lnk);
            persist.Save(PCWSTR(lnk_w.as_ptr()), true).unwrap();
        }
    }

    #[test]
    fn lnk_resolution_reads_target_args_and_workdir() {
        let dir = temp_dir("lnk");
        let exe = dir.join("bin").join("game.exe");
        touch(&exe);
        let lnk = dir.join("Game.lnk");
        make_lnk(&lnk, &exe, "-windowed -w 800", Some(&dir.join("bin")));
        let item = resolve_item(&lnk, "Games").unwrap();
        assert!(item.from_lnk);
        assert_eq!(item.target, exe);
        assert_eq!(item.args, "-windowed -w 800");
        assert_eq!(item.work_dir, Some(dir.join("bin")));
        assert!(!item.missing);
        assert_eq!(item.default_category, "Games");
    }

    #[test]
    fn lnk_with_missing_target_is_flagged() {
        let dir = temp_dir("missing");
        let lnk = dir.join("Gone.lnk");
        make_lnk(&lnk, &dir.join("nowhere").join("gone.exe"), "", None);
        let item = resolve_item(&lnk, "Games").unwrap();
        assert!(item.missing);
    }

    #[test]
    fn garbage_lnk_is_skipped_without_panic() {
        let dir = temp_dir("garbage");
        let lnk = dir.join("bad.lnk");
        fs::write(&lnk, b"not a shortcut").unwrap();
        assert!(resolve_item(&lnk, "x").is_none());
    }

    #[test]
    fn noise_filter_matches_spec_patterns() {
        for name in [
            "unins000.exe",
            "unins001.exe",
            "Uninstall Foo.exe",
            "uninstall.exe",
            "CrashHandler.exe",
            "crashhandler64.exe",
            "vcredist_x64.exe",
            "FooSetup.exe",
            "setup.exe",
        ] {
            assert!(is_noise(name), "{name} should be noise");
        }
        for name in ["Blender.exe", "unison.exe", "Game.lnk", "uninst.exe"] {
            assert!(!is_noise(name), "{name} should not be noise");
        }
    }

    #[test]
    fn scan_filters_recurses_and_respects_include() {
        let dir = temp_dir("scan");
        touch(&dir.join("A.exe"));
        touch(&dir.join("unins000.exe"));
        touch(&dir.join("sub").join("B.exe"));
        touch(&dir.join("sub").join("deeper").join("C.exe"));
        touch(&dir.join("notes.txt"));
        touch(&dir.join("Zażółć gęślą.exe"));
        let mut config = Config::default();
        let mut source = Source::new(dir.clone(), "Programs");
        source.include = vec!["lnk".into(), "exe".into()];
        config.sources.push(source);
        config.settings.scan_recursive = true;
        config.settings.scan_depth = 2;
        let names = |r: &ScanResult| {
            let mut n: Vec<String> = r.items.iter().map(|i| file_stem(&i.source_path)).collect();
            n.sort();
            n
        };
        assert_eq!(names(&scan(&config)), ["A", "B", "Zażółć gęślą"]);
        config.settings.scan_recursive = false;
        assert_eq!(names(&scan(&config)), ["A", "Zażółć gęślą"]);
        config.settings.scan_recursive = true;
        config.settings.scan_depth = 5;
        config.settings.filter_noise = false;
        assert_eq!(names(&scan(&config)).len(), 5);
        config.sources[0].include = vec!["lnk".into()];
        assert!(scan(&config).items.is_empty());
    }

    #[test]
    fn unreadable_source_is_reported() {
        let mut config = Config::default();
        let gone = temp_dir("gone").join("does-not-exist");
        config.sources.push(Source::new(gone.clone(), "Programs"));
        let result = scan(&config);
        assert_eq!(result.unreadable_sources, vec![gone]);
    }

    #[test]
    fn duplicates_collapse_preferring_lnk_and_first_source() {
        let dir = temp_dir("dedupe");
        let exe = dir.join("apps").join("tool.exe");
        touch(&exe);
        make_lnk(&dir.join("lnks").join("Tool.lnk"), &exe, "", None);
        make_lnk(&dir.join("more").join("Tool copy.lnk"), &exe, "", None);
        make_lnk(&dir.join("more").join("Tool args.lnk"), &exe, "-x", None);
        let mut config = Config::default();
        config
            .sources
            .push(Source::new(dir.join("apps"), "Programs"));
        config.sources.push(Source::new(dir.join("lnks"), "Games"));
        config
            .sources
            .push(Source::new(dir.join("more"), "Programs"));
        let entries = build_entries(&scan(&config).items, &config);
        assert_eq!(entries.len(), 2, "{entries:?}");
        let plain = entries.iter().find(|e| e.args.is_empty()).unwrap();
        assert_eq!(plain.name, "Tool");
        assert_eq!(plain.category, "Games");
        assert!(entries.iter().any(|e| e.args == "-x"));
    }

    #[test]
    fn categories_inherit_from_source_and_overrides_win() {
        let dir = temp_dir("category");
        touch(&dir.join("g").join("Game.exe"));
        touch(&dir.join("p").join("Prog.exe"));
        let mut config = Config::default();
        for (sub, category) in [("g", "Games"), ("p", "Programs")] {
            let mut source = Source::new(dir.join(sub), category);
            source.include = vec!["lnk".into(), "exe".into()];
            config.sources.push(source);
        }
        let items = scan(&config).items;
        let entries = build_entries(&items, &config);
        let game = entries.iter().find(|e| e.name == "Game").unwrap();
        let prog = entries.iter().find(|e| e.name == "Prog").unwrap();
        assert_eq!(game.category, "Games");
        assert_eq!(prog.category, "Programs");

        let id = game.id.clone();
        config.override_mut(&id).category = Some("Programs".into());
        config.override_mut(&id).name = Some("Renamed".into());
        let entries = build_entries(&items, &config);
        let game = entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!(game.category, "Programs");
        assert_eq!(game.name, "Renamed");

        config.delete_category("Games");
        config.sources[0].default_category = "Vanished".into();
        let items = scan(&config).items;
        let entries = build_entries(&items, &config);
        assert!(
            entries
                .iter()
                .all(|e| e.category == "Programs" || e.category == UNCATEGORIZED)
        );
    }

    #[test]
    fn manual_entries_are_listed_and_flagged() {
        let dir = temp_dir("manual");
        let exe = dir.join("solo.exe");
        touch(&exe);
        let item = resolve_item(&exe, UNCATEGORIZED).unwrap();
        let mut config = Config::default();
        let over = config.override_mut(&item.id());
        over.manual = true;
        over.source_path = Some(exe.clone());
        let result = scan(&config);
        let entries = build_entries(&result.items, &config);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].manual);
        assert_eq!(entries[0].category, UNCATEGORIZED);
    }

    #[test]
    fn removed_entries_are_not_listed() {
        let dir = temp_dir("removed");
        let exe = dir.join("gone.exe");
        touch(&exe);
        let item = resolve_item(&exe, UNCATEGORIZED).unwrap();
        let mut config = Config::default();
        let over = config.override_mut(&item.id());
        over.manual = true;
        over.source_path = Some(exe.clone());
        let result = scan(&config);
        assert_eq!(build_entries(&result.items, &config).len(), 1);
        config.override_mut(&item.id()).removed = true;
        assert!(build_entries(&result.items, &config).is_empty());
    }

    #[test]
    fn manual_entry_with_deleted_shortcut_keeps_its_saved_id_and_can_be_removed() {
        let dir = temp_dir("stale");
        let lnk = dir.join("Moved.lnk");
        let mut config = Config::default();
        let over = config.override_mut("saved-id");
        over.manual = true;
        over.source_path = Some(lnk);
        let result = scan(&config);
        let entries = build_entries(&result.items, &config);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "saved-id");
        assert!(entries[0].missing);
        config.apps.retain(|a| a.id != entries[0].id);
        let result = scan(&config);
        assert!(build_entries(&result.items, &config).is_empty());
    }

    #[test]
    fn manual_entry_keeps_its_saved_id_when_the_shortcut_is_retargeted() {
        let dir = temp_dir("retarget");
        let exe = dir.join("new.exe");
        touch(&exe);
        let lnk = dir.join("Game.lnk");
        make_lnk(&lnk, &exe, "", None);
        let mut config = Config::default();
        let over = config.override_mut("saved-id");
        over.manual = true;
        over.source_path = Some(lnk);
        over.favorite = true;
        let entries = build_entries(&scan(&config).items, &config);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "saved-id");
        assert!(entries[0].favorite);
        assert!(!entries[0].missing);
    }

    #[test]
    fn env_expansion() {
        assert_eq!(
            expand_env("%TEMP%\\x"),
            format!("{}\\x", std::env::var("TEMP").unwrap())
        );
        assert_eq!(expand_env("100%"), "100%");
        assert_eq!(expand_env("%NOPE_NOT_SET_12%\\a"), "%NOPE_NOT_SET_12%\\a");
    }
}
