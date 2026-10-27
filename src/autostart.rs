//! Start with Windows: one value under `HKCU\...\Run`, which needs no admin rights.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ,
    REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{PCWSTR, Result};

use crate::win32::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "topsy";

/// A string value under `HKEY_CURRENT_USER`. Everything the app does goes through
/// [`SYSTEM`]; the tests use a scratch key so they never touch the real startup list.
pub struct RunEntry<'a> {
    key: &'a str,
    name: &'a str,
}

pub const SYSTEM: RunEntry<'static> = RunEntry {
    key: RUN_KEY,
    name: VALUE_NAME,
};

fn check(status: WIN32_ERROR) -> Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(status.into())
    }
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Key {
    fn open(path: &str, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<Self> {
        let path = wide(path);
        let mut key = HKEY::default();
        check(unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                access,
                &mut key,
            )
        })?;
        Ok(Self(key))
    }

    fn create(path: &str) -> Result<Self> {
        let path = wide(path);
        let mut key = HKEY::default();
        check(unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        })?;
        Ok(Self(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

impl RunEntry<'_> {
    /// The stored command line, if the value exists.
    pub fn value(&self) -> Option<String> {
        let key = Key::open(self.key, KEY_QUERY_VALUE).ok()?;
        let name = wide(self.name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut bytes = 0u32;
        unsafe {
            check(RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut bytes),
            ))
            .ok()?;
            // A value written by hand may be REG_EXPAND_SZ; it is still the command line.
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                return None;
            }
            let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
            check(RegQueryValueExW(
                key.0,
                PCWSTR(name.as_ptr()),
                None,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut bytes),
            ))
            .ok()?;
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    pub fn set(&self, command: &str) -> Result<()> {
        let key = Key::create(self.key)?;
        let name = wide(self.name);
        let data: Vec<u8> = wide(command).iter().flat_map(|c| c.to_le_bytes()).collect();
        check(unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)) })
    }

    /// Delete the value. Succeeds if it was already absent.
    pub fn remove(&self) -> Result<()> {
        let Ok(key) = Key::open(self.key, KEY_SET_VALUE) else {
            return Ok(());
        };
        let name = wide(self.name);
        match unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) } {
            ERROR_FILE_NOT_FOUND => Ok(()),
            status => check(status),
        }
    }
}

/// The command line stored in the Run key: the exe, quoted so a path with spaces works.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The exe path inside a stored command line (quoted, or the whole string if it isn't).
pub fn exe_in(command: &str) -> PathBuf {
    let command = command.trim();
    match command.strip_prefix('"') {
        Some(rest) => PathBuf::from(rest.split('"').next().unwrap_or(rest)),
        None => PathBuf::from(command),
    }
}

pub fn is_enabled() -> bool {
    SYSTEM.value().is_some()
}

pub fn enable(exe: &Path) -> Result<()> {
    SYSTEM.set(&command_for(exe))
}

pub fn disable() -> Result<()> {
    SYSTEM.remove()
}

/// If start-with-Windows is on but points at an exe that no longer exists (the app was
/// moved or reinstalled), point it at this one. A different exe that still exists is left
/// alone, so running a development build never takes over the installed app's autostart.
pub fn refresh(exe: &Path) {
    if let Some(stored) = SYSTEM.value()
        && !exe_in(&stored).exists()
        && let Err(err) = enable(exe)
    {
        eprintln!("topsy: could not update the start-with-Windows entry: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-level scratch key, so deleting it afterwards leaves nothing behind.
    struct Scratch(String);

    impl Scratch {
        fn new(tag: &str) -> Self {
            Self(format!("Software\\topsy-test-{}-{tag}", std::process::id()))
        }

        fn entry(&self) -> RunEntry<'_> {
            RunEntry {
                key: &self.0,
                name: "value",
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            use windows::Win32::System::Registry::RegDeleteKeyW;
            let path = wide(&self.0);
            unsafe {
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()));
            }
        }
    }

    #[test]
    fn set_read_overwrite_remove_round_trip() {
        let scratch = Scratch::new("roundtrip");
        let entry = scratch.entry();
        assert_eq!(entry.value(), None);

        entry.set(r#""C:\Program Files\topsy\topsy.exe""#).unwrap();
        assert_eq!(
            entry.value().as_deref(),
            Some(r#""C:\Program Files\topsy\topsy.exe""#)
        );

        entry.set(r#""D:\new\topsy.exe""#).unwrap();
        assert_eq!(entry.value().as_deref(), Some(r#""D:\new\topsy.exe""#));

        entry.remove().unwrap();
        assert_eq!(entry.value(), None);
    }

    #[test]
    fn removing_an_absent_value_or_key_is_fine() {
        let scratch = Scratch::new("absent");
        scratch.entry().remove().unwrap();
        scratch.entry().set("x").unwrap();
        scratch.entry().remove().unwrap();
        scratch.entry().remove().unwrap();
    }

    #[test]
    fn unicode_paths_survive() {
        let scratch = Scratch::new("unicode");
        let entry = scratch.entry();
        entry.set("\"C:\\Users\\Zoë\\日本語\\topsy.exe\"").unwrap();
        assert_eq!(
            entry.value().as_deref(),
            Some("\"C:\\Users\\Zoë\\日本語\\topsy.exe\"")
        );
    }

    #[test]
    fn command_is_quoted_and_parses_back() {
        let exe = Path::new(r"C:\Program Files\topsy\topsy.exe");
        let command = command_for(exe);
        assert_eq!(command, r#""C:\Program Files\topsy\topsy.exe""#);
        assert_eq!(exe_in(&command), exe);
        // An unquoted value (written by hand) is taken whole.
        assert_eq!(exe_in(r"C:\topsy.exe"), Path::new(r"C:\topsy.exe"));
    }
}
