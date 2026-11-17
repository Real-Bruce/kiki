//! Explicit current-user startup registration. Reading preferences never enables startup.
use std::path::Path;
use windows::{
    Win32::{Foundation::*, System::Registry::*},
    core::{Error, PCWSTR, Result, w},
};
const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: exclusively owned opened registry key.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
#[derive(Clone, Debug)]
pub enum State {
    Disabled,
    Enabled,
    Moved,
}
fn command() -> Result<Vec<u16>> {
    let path = std::env::current_exe().map_err(|e| Error::new(E_FAIL, e.to_string()))?;
    Ok(quoted(&path))
}
fn quoted(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::iter::once(u16::from(b'"'))
        .chain(path.as_os_str().encode_wide())
        .chain([u16::from(b'"'), 0])
        .collect()
}
fn read_at(subkey: PCWSTR, name: PCWSTR) -> Result<Option<Vec<u16>>> {
    // SAFETY: bounded read of our named value in a current-user key, using initialized size/type outputs.
    unsafe {
        let mut key = HKEY::default();
        let result = RegOpenKeyExW(HKEY_CURRENT_USER, subkey, None, KEY_QUERY_VALUE, &mut key);
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        result.ok()?;
        let key = Key(key);
        let mut length = 0;
        let mut kind = REG_VALUE_TYPE::default();
        let result = RegQueryValueExW(key.0, name, None, Some(&mut kind), None, Some(&mut length));
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        result.ok()?;
        if kind != REG_SZ || length > 65536 || length % 2 != 0 {
            return Err(Error::new(E_FAIL, "开机启动项格式无效，请在设置中修复"));
        }
        let mut data = vec![0u16; length as usize / 2];
        let bytes = std::slice::from_raw_parts_mut(data.as_mut_ptr().cast::<u8>(), length as usize);
        RegQueryValueExW(
            key.0,
            name,
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut length),
        )
        .ok()?;
        data.truncate(length as usize / 2);
        Ok(Some(data))
    }
}
fn write_at(subkey: PCWSTR, name: PCWSTR, value: Option<&[u16]>) -> Result<()> {
    // SAFETY: explicit setting action opens only our current-user value; strings and data live through each registry call.
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let key = Key(key);
        if let Some(value) = value {
            let bytes = std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len() * 2);
            RegSetValueExW(key.0, name, None, REG_SZ, Some(bytes)).ok()?;
        } else {
            let result = RegDeleteValueW(key.0, name);
            if result != ERROR_FILE_NOT_FOUND {
                result.ok()?;
            }
        }
        Ok(())
    }
}
pub fn state() -> Result<State> {
    match read_at(RUN, w!("kiki"))? {
        None => Ok(State::Disabled),
        Some(value) => Ok(if value == command()? {
            State::Enabled
        } else {
            State::Moved
        }),
    }
}
pub fn set(enabled: bool) -> Result<()> {
    let command = command()?;
    write_at(RUN, w!("kiki"), enabled.then_some(command.as_slice()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_writes_are_explicit_and_quote_moved_paths() {
        let name = format!("kiki-test-{}", std::process::id());
        let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        let name = PCWSTR(wide.as_ptr());
        let key = w!("Software\\kiki\\Tests\\Startup");
        assert!(read_at(key, name).unwrap().is_none());
        let value = quoted(Path::new("F:\\App Folder\\kiki.exe"));
        assert_eq!(
            String::from_utf16_lossy(&value[..value.len() - 1]),
            "\"F:\\App Folder\\kiki.exe\""
        );
        write_at(key, name, Some(&value)).unwrap();
        assert_eq!(read_at(key, name).unwrap(), Some(value));
        write_at(key, name, None).unwrap();
        assert!(read_at(key, name).unwrap().is_none());
    }
}
