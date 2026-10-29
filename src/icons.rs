use crate::config::icon_cache_dir;
use crate::model::AppEntry;
use crate::scanner::{expand_env, init_com};
use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW,
};
use windows::core::PCWSTR;

/// Edge length of icons that are extracted and cached.
const ICON_SIZE: u32 = 128;
const WORKERS: usize = 3;
/// Textures uploaded per frame, so a burst of finished icons never stalls painting.
const UPLOADS_PER_FRAME: usize = 12;

pub struct RawIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RawIcon {
    fn is_blank(&self) -> bool {
        self.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0)
    }

    fn to_color_image(&self) -> ColorImage {
        ColorImage::from_rgba_unmultiplied([self.width as usize, self.height as usize], &self.rgba)
    }
}

#[derive(Clone)]
pub struct IconRequest {
    pub id: String,
    pub custom_icon: Option<PathBuf>,
    pub icon_location: Option<(PathBuf, i32)>,
    pub target: PathBuf,
    pub source_path: PathBuf,
}

impl IconRequest {
    pub fn for_entry(entry: &AppEntry) -> Self {
        Self {
            id: entry.id.clone(),
            custom_icon: entry.custom_icon.clone(),
            icon_location: entry.icon_location.clone(),
            target: entry.target.clone(),
            source_path: entry.source_path.clone(),
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Reads a bitmap into top-down BGRA bytes.
fn bitmap_bgra(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    // SAFETY: the bitmap handle is valid for the duration of the call and the buffer is sized for the DIB request.
    unsafe {
        let mut info = BITMAP::default();
        let read = GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut info as *mut BITMAP as *mut c_void),
        );
        if read == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 {
            return None;
        }
        let (width, height) = (info.bmWidth, info.bmHeight);
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buffer = vec![0u8; width as usize * height as usize * 4];
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut header,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        (lines != 0).then_some((width as u32, height as u32, buffer))
    }
}

fn bgra_to_rgba(mut bytes: Vec<u8>) -> Vec<u8> {
    for px in bytes.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    bytes
}

fn icon_to_raw(icon: HICON) -> Option<RawIcon> {
    // SAFETY: `icon` is a live icon handle; the bitmaps returned by GetIconInfo are released before returning.
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(icon, &mut info).ok()?;
        let color = (!info.hbmColor.is_invalid())
            .then(|| bitmap_bgra(info.hbmColor))
            .flatten();
        let mask = (!info.hbmMask.is_invalid())
            .then(|| bitmap_bgra(info.hbmMask))
            .flatten();
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(info.hbmColor.into());
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(info.hbmMask.into());
        }
        let (width, height, mut bgra) = color?;
        if bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
            let opaque = match &mask {
                Some((mw, mh, bits)) if *mw == width && *mh == height => bits
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|px| px[0] < 128)
                    .collect::<Vec<_>>(),
                _ => vec![true; (width * height) as usize],
            };
            for (px, solid) in bgra.as_chunks_mut::<4>().0.iter_mut().zip(opaque) {
                px[3] = if solid { 255 } else { 0 };
            }
        }
        Some(RawIcon {
            width,
            height,
            rgba: bgra_to_rgba(bgra),
        })
    }
}

/// Extracts icon `index` from an exe, dll, ico or similar file.
fn extract_from_file(path: &Path, index: i32) -> Option<RawIcon> {
    let units = wide(path);
    if units.len() > 260 {
        return None;
    }
    let mut name = [0u16; 260];
    name[..units.len()].copy_from_slice(&units);
    let mut handles = [HICON::default()];
    // SAFETY: `name` is a NUL-terminated MAX_PATH buffer and `handles` has room for the single requested icon.
    let count = unsafe {
        PrivateExtractIconsW(
            &name,
            index,
            ICON_SIZE as i32,
            ICON_SIZE as i32,
            Some(&mut handles),
            None,
            0,
        )
    };
    if count == 0 || count == u32::MAX || handles[0].is_invalid() {
        return None;
    }
    let raw = icon_to_raw(handles[0]);
    // SAFETY: the handle was created by PrivateExtractIconsW and is no longer used.
    unsafe {
        let _ = DestroyIcon(handles[0]);
    }
    raw.filter(|r| !r.is_blank())
}

/// Asks the shell for the file's icon (works for shortcuts and store apps).
fn extract_from_shell(path: &Path) -> Option<RawIcon> {
    let name = wide(path);
    // SAFETY: COM calls on objects created here; `name` outlives the call.
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).ok()?;
        let bitmap = factory
            .GetImage(
                SIZE {
                    cx: ICON_SIZE as i32,
                    cy: ICON_SIZE as i32,
                },
                SIIGBF_ICONONLY,
            )
            .ok()?;
        let pixels = bitmap_bgra(bitmap);
        let _ = DeleteObject(bitmap.into());
        let (width, height, bgra) = pixels?;
        let raw = RawIcon {
            width,
            height,
            rgba: bgra_to_rgba(bgra),
        };
        (!raw.is_blank()).then_some(raw)
    }
}

fn load_custom(path: &Path) -> Option<RawIcon> {
    let image = image::open(path).ok()?;
    let image = image.resize(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);
    let rgba = image.to_rgba8();
    Some(RawIcon {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn cache_file(id: &str) -> PathBuf {
    icon_cache_dir().join(format!("{id}.png"))
}

fn read_cached(id: &str) -> Option<RawIcon> {
    let rgba = image::open(cache_file(id)).ok()?.to_rgba8();
    Some(RawIcon {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn write_cached(id: &str, icon: &RawIcon) {
    let dir = icon_cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let tmp = dir.join(format!("{id}.png.tmp"));
    let saved = image::save_buffer_with_format(
        &tmp,
        &icon.rgba,
        icon.width,
        icon.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    );
    if saved.is_ok() {
        let _ = std::fs::rename(&tmp, cache_file(id));
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Resolves an icon following the spec priority: custom, shortcut icon location, target exe,
/// then the shell. Extracted icons are cached as PNG. None means "draw the letter tile".
pub fn resolve_icon(request: &IconRequest) -> Option<RawIcon> {
    if let Some(custom) = &request.custom_icon
        && let Some(icon) = load_custom(custom)
    {
        return Some(icon);
    }
    if let Some(icon) = read_cached(&request.id) {
        return Some(icon);
    }
    let from_location = request.icon_location.as_ref().and_then(|(path, index)| {
        let path = PathBuf::from(expand_env(&path.to_string_lossy()));
        path.exists()
            .then(|| extract_from_file(&path, *index))
            .flatten()
    });
    let extracted = from_location
        .or_else(|| {
            let target = &request.target;
            (target.exists() && !target.as_os_str().is_empty())
                .then(|| extract_from_file(target, 0))
                .flatten()
        })
        .or_else(|| {
            let target = &request.target;
            (target.exists() && !target.as_os_str().is_empty())
                .then(|| extract_from_shell(target))
                .flatten()
        })
        .or_else(|| {
            request
                .source_path
                .exists()
                .then(|| extract_from_shell(&request.source_path))
                .flatten()
        })?;
    write_cached(&request.id, &extracted);
    Some(extracted)
}

enum IconState {
    Pending,
    Ready(TextureHandle),
    Fallback,
}

struct Finished {
    id: String,
    icon: Option<RawIcon>,
}

/// Loads icons on worker threads and keeps the resulting egui textures.
pub struct IconManager {
    state: HashMap<String, IconState>,
    requests: Sender<IconRequest>,
    finished: Receiver<Finished>,
}

impl IconManager {
    pub fn new(ctx: &Context) -> Self {
        let (request_tx, request_rx) = channel::<IconRequest>();
        let (finished_tx, finished_rx) = channel::<Finished>();
        let shared: Arc<Mutex<Receiver<IconRequest>>> = Arc::new(Mutex::new(request_rx));
        for _ in 0..WORKERS {
            let shared = Arc::clone(&shared);
            let finished_tx = finished_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                init_com();
                loop {
                    let next = match shared.lock() {
                        Ok(rx) => rx.recv(),
                        Err(_) => return,
                    };
                    let Ok(request) = next else { return };
                    let icon = resolve_icon(&request);
                    if finished_tx
                        .send(Finished {
                            id: request.id,
                            icon,
                        })
                        .is_err()
                    {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self {
            state: HashMap::new(),
            requests: request_tx,
            finished: finished_rx,
        }
    }

    /// Uploads finished icons. Call once per frame.
    pub fn poll(&mut self, ctx: &Context) {
        for _ in 0..UPLOADS_PER_FRAME {
            let Ok(done) = self.finished.try_recv() else {
                return;
            };
            let state = match done.icon {
                Some(icon) => IconState::Ready(ctx.load_texture(
                    format!("icon-{}", done.id),
                    icon.to_color_image(),
                    TextureOptions::LINEAR,
                )),
                None => IconState::Fallback,
            };
            self.state.insert(done.id, state);
        }
        ctx.request_repaint();
    }

    /// Returns the icon texture if ready, queueing a load the first time an entry is seen.
    pub fn get(&mut self, entry: &AppEntry) -> Option<&TextureHandle> {
        if !self.state.contains_key(&entry.id) {
            self.state.insert(entry.id.clone(), IconState::Pending);
            let _ = self.requests.send(IconRequest::for_entry(entry));
        }
        match self.state.get(&entry.id) {
            Some(IconState::Ready(texture)) => Some(texture),
            _ => None,
        }
    }

    /// True while the icon is still loading (so the UI can avoid flashing a letter tile).
    pub fn is_pending(&self, id: &str) -> bool {
        matches!(self.state.get(id), Some(IconState::Pending))
    }

    /// Forgets an icon and its cache file so it is extracted again (after an icon edit).
    pub fn invalidate(&mut self, id: &str) {
        self.state.remove(id);
        let _ = std::fs::remove_file(cache_file(id));
    }

    pub fn clear_cache(&mut self) -> usize {
        self.state.clear();
        let mut removed = 0;
        if let Ok(entries) = std::fs::read_dir(icon_cache_dir()) {
            for entry in entries.flatten() {
                if std::fs::remove_file(entry.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_icon_loads_any_common_picture_format() {
        let dir = std::env::temp_dir().join(format!("apollolauncher-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let picture = image::RgbImage::from_pixel(300, 200, image::Rgb([200, 40, 90]));
        for name in [
            "a.png", "a.jpg", "a.bmp", "a.gif", "a.tiff", "a.webp", "a.ico",
        ] {
            let path = dir.join(name);
            if name.ends_with("ico") {
                image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 40, 90, 255]))
                    .save(&path)
                    .unwrap();
            } else {
                picture.save(&path).unwrap();
            }
            let icon = load_custom(&path).unwrap_or_else(|| panic!("{name} did not load"));
            assert!(
                icon.width <= ICON_SIZE && icon.height <= ICON_SIZE,
                "{name}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn system_exe() -> PathBuf {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        PathBuf::from(root).join("System32").join("notepad.exe")
    }

    fn data_dir_guard(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apollolauncher-icons-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn exe_icon_is_extracted() {
        init_com();
        let icon = extract_from_file(&system_exe(), 0).expect("notepad has an icon");
        assert!(icon.width >= 32 && icon.height >= 32);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        assert!(!icon.is_blank());
    }

    #[test]
    fn shell_fallback_works_for_files() {
        init_com();
        let icon = extract_from_shell(&system_exe()).expect("shell icon");
        assert!(!icon.is_blank());
    }

    #[test]
    fn missing_file_gives_no_icon() {
        init_com();
        assert!(extract_from_file(Path::new(r"C:\nope\none.exe"), 0).is_none());
        assert!(extract_from_shell(Path::new(r"C:\nope\none.exe")).is_none());
    }

    #[test]
    fn custom_png_is_loaded_and_resized() {
        let dir = data_dir_guard("custom");
        let file = dir.join("c.png");
        image::save_buffer(
            &file,
            &vec![200u8; 16 * 16 * 4],
            16,
            16,
            image::ColorType::Rgba8,
        )
        .unwrap();
        let icon = load_custom(&file).unwrap();
        assert_eq!((icon.width, icon.height), (ICON_SIZE, ICON_SIZE));
        assert!(load_custom(&dir.join("missing.png")).is_none());
        std::fs::write(dir.join("bad.png"), b"not an image").unwrap();
        assert!(load_custom(&dir.join("bad.png")).is_none());
    }
}
