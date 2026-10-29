use crate::model::AppEntry;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

/// What will be handed to `ShellExecuteW`.
#[derive(Debug, PartialEq)]
pub struct LaunchPlan {
    pub file: PathBuf,
    pub params: String,
    pub dir: Option<PathBuf>,
}

/// Decides file, arguments and working directory for an entry. Shortcuts without a file target
/// (for example store apps) are opened through the shortcut itself.
pub fn plan(entry: &AppEntry) -> LaunchPlan {
    if entry.target.as_os_str().is_empty() {
        return LaunchPlan {
            file: entry.source_path.clone(),
            params: String::new(),
            dir: entry.source_path.parent().map(Path::to_path_buf),
        };
    }
    let dir = entry
        .work_dir
        .clone()
        .filter(|d| d.is_dir())
        .or_else(|| entry.target.parent().map(Path::to_path_buf));
    LaunchPlan {
        file: entry.target.clone(),
        params: entry.args.clone(),
        dir,
    }
}

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn wide_str(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn shell_execute(verb: &str, file: &Path, params: &str, dir: Option<&Path>) -> Result<(), String> {
    let verb_w = wide_str(verb);
    let file_w = wide(file.as_os_str());
    let params_w = wide_str(params);
    let dir_w = dir.map(|d| wide(d.as_os_str()));
    let params_ptr = if params.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(params_w.as_ptr())
    };
    let dir_ptr = dir_w
        .as_ref()
        .map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr()));
    // SAFETY: all pointers refer to NUL-terminated buffers that outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(file_w.as_ptr()),
            params_ptr,
            dir_ptr,
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(describe_error(code))
    }
}

fn describe_error(code: isize) -> String {
    match code {
        0 => "Out of memory or resources".to_owned(),
        2 => "File not found".to_owned(),
        3 => "Path not found".to_owned(),
        5 => "Access denied".to_owned(),
        8 => "Not enough memory".to_owned(),
        26 => "Sharing violation".to_owned(),
        27..=31 => "No application is associated with this file".to_owned(),
        1223 => "Cancelled".to_owned(),
        other => format!("Windows error {other}"),
    }
}

/// Starts the app. `admin` requests elevation through the `runas` verb.
pub fn launch(entry: &AppEntry, admin: bool) -> Result<(), String> {
    let plan = plan(entry);
    if !plan.file.exists() {
        return Err(format!("{} was not found", plan.file.display()));
    }
    let verb = if admin { "runas" } else { "open" };
    shell_execute(verb, &plan.file, &plan.params, plan.dir.as_deref())
        .map_err(|e| format!("Could not start {}: {e}", entry.name))
}

/// Opens Explorer with the shortcut (or the executable) selected.
pub fn reveal(entry: &AppEntry) -> Result<(), String> {
    let path = if entry.source_path.exists() {
        &entry.source_path
    } else {
        &entry.target
    };
    if !path.exists() {
        return Err(format!("{} no longer exists", path.display()));
    }
    let params = format!("/select,\"{}\"", path.display());
    shell_execute("open", Path::new("explorer.exe"), &params, None)
        .map_err(|e| format!("Could not open the folder: {e}"))
}

/// Moves a file to the Recycle Bin.
pub fn recycle(path: &Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    // The path list must end with two NUL characters.
    let mut from = wide(path.as_os_str());
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    // SAFETY: `op` and the buffer behind `pFrom` outlive the call.
    let result = unsafe { SHFileOperationW(&mut op) };
    if result != 0 || op.fAnyOperationsAborted.as_bool() {
        return Err(format!("Could not remove {}", path.display()));
    }
    Ok(())
}

/// Opens a folder in Explorer.
pub fn open_folder(path: &Path) -> Result<(), String> {
    shell_execute("open", path, "", None).map_err(|e| format!("Could not open the folder: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(target: &str, work_dir: Option<&str>) -> AppEntry {
        AppEntry {
            id: "1".into(),
            name: "Game".into(),
            description: String::new(),
            source_path: PathBuf::from(r"C:\Links\Game.lnk"),
            target: PathBuf::from(target),
            args: "-windowed".into(),
            work_dir: work_dir.map(PathBuf::from),
            icon_location: None,
            category: "Games".into(),
            custom_icon: None,
            accent: None,
            hidden: false,
            favorite: false,
            last_launched: None,
            launch_count: 0,
            manual: false,
            missing: false,
            name_key: String::new(),
            search_key: String::new(),
        }
    }

    #[test]
    fn working_dir_defaults_to_target_folder() {
        let plan = plan(&entry(r"C:\Games\Foo\foo.exe", Some(r"C:\does\not\exist")));
        assert_eq!(plan.file, PathBuf::from(r"C:\Games\Foo\foo.exe"));
        assert_eq!(plan.params, "-windowed");
        assert_eq!(plan.dir, Some(PathBuf::from(r"C:\Games\Foo")));
    }

    #[test]
    fn existing_shortcut_work_dir_is_kept() {
        let tmp = std::env::temp_dir();
        let plan = plan(&entry(r"C:\Games\Foo\foo.exe", tmp.to_str()));
        assert_eq!(plan.dir, Some(tmp));
    }

    #[test]
    fn targetless_shortcut_launches_itself() {
        let plan = plan(&entry("", None));
        assert_eq!(plan.file, PathBuf::from(r"C:\Links\Game.lnk"));
        assert!(plan.params.is_empty());
        assert_eq!(plan.dir, Some(PathBuf::from(r"C:\Links")));
    }

    #[test]
    fn missing_file_is_an_error_not_a_crash() {
        let result = launch(&entry(r"C:\definitely\not\here.exe", None), false);
        assert!(result.unwrap_err().contains("was not found"));
    }
}
