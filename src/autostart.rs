//! "Start with Windows" through the per-user registry Run key.

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{PCWSTR, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("ApolloLauncher");

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether the Run key currently contains our value.
pub fn is_enabled() -> bool {
    let mut key = HKEY::default();
    // SAFETY: plain registry calls with valid pointers; the key is closed before returning.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_QUERY_VALUE, &mut key)
            != ERROR_SUCCESS
        {
            return false;
        }
        let found = RegQueryValueExW(key, VALUE_NAME, None, None, None, None) == ERROR_SUCCESS;
        let _ = RegCloseKey(key);
        found
    }
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let mut key = HKEY::default();
    // SAFETY: plain registry calls with valid pointers; the key is closed before returning.
    unsafe {
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(format!(
                "could not open the registry Run key ({})",
                status.0
            ));
        }
        let result = if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| format!("could not find the executable: {e}"))?;
            let command = wide(&format!("\"{}\"", exe.display()));
            let bytes =
                std::slice::from_raw_parts(command.as_ptr().cast::<u8>(), command.len() * 2);
            let status = RegSetValueExW(key, VALUE_NAME, None, REG_SZ, Some(bytes));
            if status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(format!("could not write the registry value ({})", status.0))
            }
        } else {
            let status = RegDeleteValueW(key, VALUE_NAME);
            // A value that is already gone counts as success.
            if status == ERROR_SUCCESS || !is_enabled() {
                Ok(())
            } else {
                Err(format!(
                    "could not remove the registry value ({})",
                    status.0
                ))
            }
        };
        let _ = RegCloseKey(key);
        result
    }
}
