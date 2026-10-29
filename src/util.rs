use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_normalization::UnicodeNormalization;

/// Lower-cases, strips diacritics and folds a few letters that have no decomposition.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.nfd().filter(|c| !is_combining_mark(*c)) {
        match c {
            'ł' | 'Ł' => out.push('l'),
            'ø' | 'Ø' => out.push('o'),
            'đ' | 'Đ' => out.push('d'),
            'ß' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF)
}

/// FNV-1a, rendered as 16 hex digits. Stable across runs and platforms.
pub fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn hash_value(text: &str) -> u64 {
    u64::from_str_radix(&stable_hash(text), 16).unwrap_or(0)
}

/// Identity of an app: its resolved target (case-insensitive) plus launch arguments.
pub fn app_id(target: &Path, args: &str) -> String {
    let key = format!(
        "{}|{}",
        target.to_string_lossy().to_lowercase(),
        args.trim().to_lowercase()
    );
    stable_hash(&key)
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn relative_time(then: Option<i64>, now: i64) -> String {
    let Some(then) = then else {
        return "Never".to_owned();
    };
    let delta = (now - then).max(0);
    match delta {
        0..=59 => "Just now".to_owned(),
        60..=3599 => format!("{} min ago", delta / 60),
        3600..=86_399 => format!("{} h ago", delta / 3600),
        86_400..=2_591_999 => format!("{} d ago", delta / 86_400),
        _ => format!("{} mo ago", delta / 2_592_000),
    }
}

/// Splits a display name into a plain head and an accented tail ("CADCraft" -> "CAD", "Craft").
pub fn split_name(name: &str) -> (String, String) {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() < 2 {
        return (name.to_owned(), String::new());
    }
    let camel = (1..chars.len())
        .rev()
        .find(|&i| chars[i].is_uppercase() && chars[i - 1].is_lowercase());
    let acronym = (1..chars.len() - 1).rev().find(|&i| {
        chars[i].is_uppercase() && chars[i - 1].is_uppercase() && chars[i + 1].is_lowercase()
    });
    let space = chars
        .iter()
        .rposition(|c| *c == ' ' || *c == '-' || *c == '_');
    let split = camel
        .or(acronym)
        .or(space.map(|i| i + 1))
        .unwrap_or(chars.len().div_ceil(2));
    let split = split.min(chars.len());
    (
        chars[..split].iter().collect(),
        chars[split..].iter().collect(),
    )
}

pub fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

pub fn format_hex_color(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_diacritics_and_case() {
        assert_eq!(normalize("Łódź Éclair Ångström"), "lodz eclair angstrom");
        assert_eq!(normalize("Straße"), "strasse");
        assert_eq!(normalize("MatCraft"), "matcraft");
    }

    #[test]
    fn hash_is_stable_and_case_insensitive_for_paths() {
        let a = app_id(Path::new(r"C:\Games\Foo.exe"), "");
        let b = app_id(Path::new(r"c:\games\foo.EXE"), " ");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert_ne!(a, app_id(Path::new(r"C:\Games\Foo.exe"), "-windowed"));
        assert_eq!(stable_hash("abc"), "e71fa2190541574b");
    }

    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(None, 100), "Never");
        assert_eq!(relative_time(Some(100), 130), "Just now");
        assert_eq!(relative_time(Some(0), 300), "5 min ago");
        assert_eq!(relative_time(Some(0), 7300), "2 h ago");
        assert_eq!(relative_time(Some(0), 86_400 * 3), "3 d ago");
    }

    #[test]
    fn split_name_prefers_camel_case() {
        assert_eq!(split_name("CADCraft"), ("CAD".into(), "Craft".into()));
        assert_eq!(split_name("MatCraft"), ("Mat".into(), "Craft".into()));
        assert_eq!(
            split_name("Visual Studio"),
            ("Visual ".into(), "Studio".into())
        );
        assert_eq!(split_name("Blender"), ("Blen".into(), "der".into()));
        assert_eq!(split_name("X"), ("X".into(), String::new()));
    }

    #[test]
    fn hex_colors_round_trip() {
        assert_eq!(parse_hex_color("#1ABC9C"), Some([0x1A, 0xBC, 0x9C]));
        assert_eq!(parse_hex_color("zzz"), None);
        assert_eq!(format_hex_color([1, 2, 255]), "#0102FF");
    }
}
