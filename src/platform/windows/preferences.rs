//! UI-thread preference state; disk IO is delegated to one bounded writer.
use super::{
    config_store::{self, Disk, Writer},
    hotkey::Registration,
};
use crate::core::config::{Config, Theme};
use std::{cell::RefCell, path::PathBuf};
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::{Foundation::*, UI::WindowsAndMessaging::*},
    core::{Error, Result},
};
pub const CHANGED: u32 = WM_APP + 61;
pub const OPEN_SETTINGS: u32 = WM_APP + 70;
pub const CAPTURE: u32 = WM_APP + 71;
pub const FULLSCREEN: u32 = WM_APP + 72;
pub const CAPTURE_FROM_SETTINGS: u32 = WM_APP + 73;
#[derive(Clone, Debug)]
pub enum SaveStatus {
    Ready,
    Saving,
    Saved,
    Failed(String),
}
struct Session {
    config: Config,
    path: PathBuf,
    revision: u64,
    status: SaveStatus,
    writer: Writer,
    registration: Registration,
    owner: HWND,
    system_dark: Option<bool>,
}
thread_local! {static SESSION:RefCell<Option<Session>>=const{RefCell::new(None)};}
pub struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let session = SESSION.with_borrow_mut(Option::take);
        drop(session);
    }
}
pub fn initialize(owner: HWND) -> Result<(Guard, Option<String>)> {
    let path = config_store::default_path().map_err(|e| Error::new(E_FAIL, e))?;
    initialize_at(owner, path)
}
pub(super) fn initialize_at(owner: HWND, path: PathBuf) -> Result<(Guard, Option<String>)> {
    if SESSION.with_borrow(Option::is_some) {
        return Err(Error::new(E_FAIL, "设置管理器已启动"));
    }
    let loaded = Disk::load(path.clone());
    let mut config = loaded.config;
    let warning = loaded.warning;
    if std::env::args_os().any(|arg| arg == "--theme=dark") {
        config.theme = Theme::Dark;
    } else if std::env::args_os().any(|arg| arg == "--theme=light") {
        config.theme = Theme::Light;
    }
    let writer = Writer::new(loaded.disk, Some(owner)).map_err(|e| Error::new(E_FAIL, e))?;
    let mut registration = Registration::new(owner);
    let _ = registration.replace(config.hotkey);
    SESSION.with_borrow_mut(|slot| {
        *slot = Some(Session {
            config,
            path,
            revision: 0,
            status: SaveStatus::Ready,
            writer,
            registration,
            owner,
            system_dark: None,
        })
    });
    Ok((Guard, warning))
}
pub fn current() -> Config {
    SESSION.with_borrow(|slot| slot.as_ref().map(|s| s.config.clone()).unwrap_or_default())
}
pub fn theme() -> Option<Theme> {
    SESSION.with_borrow(|slot| slot.as_ref().map(|s| s.config.theme))
}
/// Cache the resolved color mode until the tray host receives a system theme notification.
pub fn system_is_dark() -> bool {
    if let Some(dark) = SESSION.with_borrow(|slot| slot.as_ref().and_then(|s| s.system_dark)) {
        return dark;
    }
    // Query outside the preference borrow because the OS can dispatch native messages.
    let Ok(color) = UISettings::new().and_then(|s| s.GetColorValue(UIColorType::Background)) else {
        return false;
    };
    let dark = u32::from(color.R) + u32::from(color.G) + u32::from(color.B) < 384;
    SESSION.with_borrow_mut(|slot| {
        slot.as_mut()
            .map_or(dark, |s| *s.system_dark.get_or_insert(dark))
    })
}
pub fn invalidate_system_theme() {
    SESSION.with_borrow_mut(|slot| {
        if let Some(session) = slot {
            session.system_dark = None;
        }
    });
}
pub fn hints() -> bool {
    SESSION.with_borrow(|slot| slot.as_ref().is_none_or(|s| s.config.show_hints))
}
pub fn status() -> SaveStatus {
    SESSION.with_borrow(|slot| {
        slot.as_ref()
            .map_or(SaveStatus::Ready, |s| s.status.clone())
    })
}
pub fn path() -> Option<PathBuf> {
    SESSION.with_borrow(|slot| slot.as_ref().map(|s| s.path.clone()))
}
pub fn active() -> bool {
    SESSION.with_borrow(|slot| slot.as_ref().is_some_and(|s| s.registration.active()))
}
pub fn active_shortcut() -> Option<String> {
    SESSION.with_borrow(|slot| {
        let session = slot.as_ref()?;
        session
            .registration
            .active()
            .then(|| session.config.hotkey.label())
    })
}
pub fn request_capture_from_settings() -> std::result::Result<(), String> {
    let owner = SESSION
        .with_borrow(|slot| slot.as_ref().map(|s| s.owner))
        .ok_or("截图暂不可用。")?;
    // SAFETY: the tray owner outlives the UI-thread preferences session.
    unsafe { PostMessageW(Some(owner), CAPTURE_FROM_SETTINGS, WPARAM(0), LPARAM(0)) }
        .map_err(|_| "无法开始截图，请重试。".into())
}
pub fn matches(id: usize) -> bool {
    SESSION.with_borrow(|slot| slot.as_ref().is_some_and(|s| s.registration.matches(id)))
}
pub fn update(config: Config) -> std::result::Result<(), String> {
    update_with_shortcut_retry(config, false)
}

pub fn set_shortcut(shortcut: crate::core::config::Hotkey) -> std::result::Result<(), String> {
    let mut config = current();
    config.hotkey = shortcut;
    update_with_shortcut_retry(config, true)
}

fn update_with_shortcut_retry(config: Config, retry: bool) -> std::result::Result<(), String> {
    config.validate()?;
    let owner = SESSION.with_borrow_mut(|slot| -> std::result::Result<Option<HWND>, String> {
        let session = slot.as_mut().ok_or("设置管理器尚未启动")?;
        let retry_shortcut = retry && !session.registration.active();
        if config == session.config && !retry_shortcut {
            return Ok(None);
        }
        let revision = session
            .revision
            .checked_add(1)
            .ok_or("设置版本计数已用尽")?;
        if config.hotkey != session.config.hotkey || retry_shortcut {
            session
                .registration
                .replace(config.hotkey)
                .map_err(|error| {
                    let reason = if error.code()
                        == windows::core::HRESULT::from_win32(ERROR_HOTKEY_ALREADY_REGISTERED.0)
                    {
                        "此快捷键已被占用"
                    } else {
                        "Windows 无法使用此快捷键"
                    };
                    format!("{reason}，请换一个组合。原设置已保留。")
                })?;
        }
        session.revision = revision;
        if config.theme != session.config.theme {
            session.system_dark = None;
        }
        session.config = config;
        session.status = SaveStatus::Saving;
        session
            .writer
            .request(session.revision, session.config.clone());
        Ok(Some(session.owner))
    })?;
    if let Some(owner) = owner {
        notify(owner);
    }
    Ok(())
}
pub fn record_first_use() {
    SESSION.with_borrow_mut(|slot| {
        if let Some(session) = slot.as_mut()
            && !session.config.first_tip_seen
            && let Some(revision) = session.revision.checked_add(1)
        {
            session.config.first_tip_seen = true;
            session.revision = revision;
            session.writer.request(revision, session.config.clone());
        }
    });
}

pub fn retry() {
    let owner = SESSION.with_borrow_mut(|slot| {
        let session = slot.as_mut()?;
        session.status = SaveStatus::Saving;
        session
            .writer
            .request(session.revision, session.config.clone());
        Some(session.owner)
    });
    if let Some(owner) = owner {
        notify(owner);
    }
}
pub fn poll() {
    SESSION.with_borrow_mut(|slot| {
        if let Some(session) = slot {
            if let Some(outcome) = session.writer.outcome() {
                if outcome.revision == session.revision {
                    session.status = match outcome.result {
                        Ok(()) if matches!(session.status, SaveStatus::Ready) => SaveStatus::Ready,
                        Ok(()) => SaveStatus::Saved,
                        Err(error) => SaveStatus::Failed(error),
                    };
                }
            }
        }
    });
}
pub fn external_saved() {
    SESSION.with_borrow_mut(|slot| {
        if let Some(session) = slot {
            if matches!(session.status, SaveStatus::Ready | SaveStatus::Saved) {
                session.status = SaveStatus::Saved;
            }
        }
    });
}
pub fn remember_directory(path: &std::path::Path) {
    if let Some(path) = path.to_str() {
        let mut config = current();
        config.last_directory = Some(path.into());
        let _ = update(config);
    }
}
fn notify(owner: HWND) {
    // SAFETY: value-only notification to the UI host, whose lifetime encloses this session.
    unsafe {
        let _ = PostMessageW(Some(owner), CHANGED, WPARAM(0), LPARAM(0));
    }
}
