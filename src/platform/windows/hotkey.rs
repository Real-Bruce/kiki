//! Transactional system hotkeys: acquire a candidate before relinquishing the active registration.
use crate::core::config::{Hotkey, Key};
use windows::{
    Win32::{
        Foundation::HWND,
        UI::{
            Controls::{HKM_GETHOTKEY, HKM_SETHOTKEY, HOTKEYF_ALT, HOTKEYF_CONTROL, HOTKEYF_SHIFT},
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::SendMessageW,
        },
    },
    core::Result,
};
pub struct Registration {
    owner: HWND,
    active: Option<(i32, Hotkey)>,
}
impl Registration {
    pub fn new(owner: HWND) -> Self {
        Self {
            owner,
            active: None,
        }
    }
    pub fn active(&self) -> bool {
        self.active.is_some()
    }
    pub fn matches(&self, id: usize) -> bool {
        self.active
            .is_some_and(|(current, _)| current as usize == id)
    }
    pub fn replace(&mut self, hotkey: Hotkey) -> Result<()> {
        hotkey.validate().map_err(|error| {
            windows::core::Error::new(windows::Win32::Foundation::E_INVALIDARG, error)
        })?;
        if self.active.is_some_and(|(_, current)| current == hotkey) {
            return Ok(());
        }
        let id = if self.active.is_some_and(|(id, _)| id == 1) {
            2
        } else {
            1
        };
        let mut flags = MOD_NOREPEAT;
        if hotkey.control {
            flags |= MOD_CONTROL;
        }
        if hotkey.alt {
            flags |= MOD_ALT;
        }
        if hotkey.shift {
            flags |= MOD_SHIFT;
        }
        let key = match hotkey.key {
            Key::Character(c) => c as u32,
            Key::Function(n) => u32::from(VK_F1.0) + u32::from(n) - 1,
        };
        // SAFETY: the owning hidden HWND and registration IDs stay on this UI thread. No message loop runs while replacing registrations.
        unsafe {
            RegisterHotKey(Some(self.owner), id, flags, key)?;
            if let Some((old_id, _)) = self.active {
                if let Err(error) = UnregisterHotKey(Some(self.owner), old_id) {
                    let _ = UnregisterHotKey(Some(self.owner), id);
                    return Err(error);
                }
            }
        }
        self.active = Some((id, hotkey));
        Ok(())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Some((id, _)) = self.active.take() {
            // SAFETY: the manager is dropped before the owner HWND, on its UI thread.
            unsafe {
                let _ = UnregisterHotKey(Some(self.owner), id);
            }
        }
    }
}
pub fn control_value(hwnd: HWND) -> Result<Hotkey> {
    // SAFETY: HWND is a live native hotkey child; message returns a packed virtual-key/modifier value.
    let value = unsafe { SendMessageW(hwnd, HKM_GETHOTKEY, None, None).0 as u16 };
    let key = (value & 255) as u8;
    let flags = value >> 8;
    let key = if (VK_F1.0..=VK_F24.0).contains(&u16::from(key)) {
        Key::Function((u16::from(key) - VK_F1.0 + 1) as u8)
    } else {
        Key::Character(char::from(key))
    };
    let hotkey = Hotkey {
        control: flags & (HOTKEYF_CONTROL as u16) != 0,
        alt: flags & (HOTKEYF_ALT as u16) != 0,
        shift: flags & (HOTKEYF_SHIFT as u16) != 0,
        key,
    };
    hotkey.validate().map_err(|error| {
        windows::core::Error::new(windows::Win32::Foundation::E_INVALIDARG, error)
    })?;
    Ok(hotkey)
}
pub fn set_control(hwnd: HWND, hotkey: Hotkey) {
    let key = match hotkey.key {
        Key::Character(c) => c as u16,
        Key::Function(n) => VK_F1.0 + u16::from(n) - 1,
    };
    let mut flags = 0u16;
    if hotkey.control {
        flags |= HOTKEYF_CONTROL as u16;
    }
    if hotkey.alt {
        flags |= HOTKEYF_ALT as u16;
    }
    if hotkey.shift {
        flags |= HOTKEYF_SHIFT as u16;
    }
    // SAFETY: call-scoped packed value sent to a live UI-thread hotkey control. This initialization does not register a global hotkey.
    unsafe {
        let _ = SendMessageW(
            hwnd,
            HKM_SETHOTKEY,
            Some(windows::Win32::Foundation::WPARAM(usize::from(
                key | (flags << 8),
            ))),
            None,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::{System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*},
        core::w,
    };
    #[test]
    fn conflict_preserves_old_registration_and_replacement_releases_it() {
        // SAFETY: both test-owned windows and all registrations are created and destroyed on this test thread.
        unsafe {
            let module = GetModuleHandleW(None).unwrap().into();
            let first = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki hotkey test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(module),
                None,
            )
            .unwrap();
            let second = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki hotkey contender"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(module),
                None,
            )
            .unwrap();
            let mut manager = Registration::new(first);
            let mut keys = Vec::new();
            for function in 19..=24 {
                let key = Hotkey {
                    control: true,
                    alt: true,
                    shift: false,
                    key: Key::Function(function),
                };
                if RegisterHotKey(
                    Some(second),
                    i32::from(function),
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    u32::from(VK_F1.0) + u32::from(function) - 1,
                )
                .is_ok()
                {
                    keys.push(key);
                }
            }
            assert!(keys.len() >= 2, "need two available test chords");
            let original = keys[0];
            let candidate = keys[1];
            let original_id = match original.key {
                Key::Function(n) => i32::from(n),
                _ => 0,
            };
            UnregisterHotKey(Some(second), original_id).unwrap();
            manager.replace(original).unwrap();
            assert!(manager.active());
            assert!(manager.matches(1));
            assert!(manager.replace(candidate).is_err());
            assert_eq!(manager.active.unwrap().1, original);
            let key = match original.key {
                Key::Function(n) => u32::from(VK_F1.0) + u32::from(n) - 1,
                _ => 0,
            };
            assert!(
                RegisterHotKey(Some(second), 99, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, key)
                    .is_err()
            );
            let candidate_id = match candidate.key {
                Key::Function(n) => i32::from(n),
                _ => 0,
            };
            UnregisterHotKey(Some(second), candidate_id).unwrap();
            manager.replace(candidate).unwrap();
            assert!(manager.matches(2));
            assert!(!manager.matches(1));
            use windows::Win32::UI::Controls::*;
            InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_HOTKEY_CLASS,
            })
            .ok()
            .unwrap();
            let input = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                HOTKEY_CLASS,
                w!(""),
                WS_CHILD,
                0,
                0,
                200,
                30,
                Some(first),
                None,
                Some(module),
                None,
            )
            .unwrap();
            set_control(input, candidate);
            assert_eq!(control_value(input).unwrap(), candidate);
            RegisterHotKey(Some(second), 99, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, key).unwrap();
            UnregisterHotKey(Some(second), 99).unwrap();
            drop(manager);
            for hotkey in keys.iter().skip(2) {
                if let Key::Function(n) = hotkey.key {
                    UnregisterHotKey(Some(second), i32::from(n)).unwrap();
                }
            }
            DestroyWindow(first).unwrap();
            DestroyWindow(second).unwrap();
        }
    }
}
