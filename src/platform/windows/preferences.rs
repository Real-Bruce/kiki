//! UI-thread preference state; disk IO is delegated to one bounded writer.
use super::{
    config_store::{self, Disk, Writer},
    hotkey::Registration,
};
use crate::core::config::{Config, Theme};
use std::{cell::RefCell, path::PathBuf};
use windows::{
    Win32::{Foundation::*, UI::WindowsAndMessaging::*},
    core::{Error, Result},
};
pub const CHANGED: u32 = WM_APP + 61;
pub const OPEN_SETTINGS: u32 = WM_APP + 70;
pub const CAPTURE: u32 = WM_APP + 71;
pub const FULLSCREEN: u32 = WM_APP + 72;
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
    let mut warning = loaded.warning;
    if std::env::args_os().any(|arg| arg == "--theme=dark") {
        config.theme = Theme::Dark;
    } else if std::env::args_os().any(|arg| arg == "--theme=light") {
        config.theme = Theme::Light;
    }
    let writer = Writer::new(loaded.disk, Some(owner)).map_err(|e| Error::new(E_FAIL, e))?;
    let mut registration = Registration::new(owner);
    if let Err(error) = registration.replace(config.hotkey) {
        let message = format!(
            "{} 注册失败（{error}），可从托盘进入设置修改。",
            config.hotkey.label()
        );
        warning = Some(warning.map_or(message.clone(), |old| format!("{old}\n{message}")));
    }
    SESSION.with_borrow_mut(|slot| {
        *slot = Some(Session {
            config,
            path,
            revision: 0,
            status: SaveStatus::Ready,
            writer,
            registration,
            owner,
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
pub fn matches(id: usize) -> bool {
    SESSION.with_borrow(|slot| slot.as_ref().is_some_and(|s| s.registration.matches(id)))
}
pub fn update(config: Config) -> std::result::Result<(), String> {
    config.validate()?;
    let owner = SESSION.with_borrow_mut(|slot| -> std::result::Result<Option<HWND>, String> {
        let session = slot.as_mut().ok_or("设置管理器尚未启动")?;
        if config == session.config && session.registration.active() {
            return Ok(None);
        }
        let revision = session
            .revision
            .checked_add(1)
            .ok_or("设置版本计数已用尽")?;
        if config.hotkey != session.config.hotkey || !session.registration.active() {
            session
                .registration
                .replace(config.hotkey)
                .map_err(|e| format!("热键已被占用或系统拒绝注册：{e}；原热键已保留"))?;
        }
        session.revision = revision;
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
