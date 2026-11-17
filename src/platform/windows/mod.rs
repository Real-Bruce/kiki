//! Windows 平台入口；功能实现分别归入对应子模块。
mod app;
mod capture;
#[cfg(feature = "diagnostics")]
mod diagnostics;
mod editor;
mod graphics;
mod output;
mod pinned;
mod settings;
mod ui;

pub use app::{run, show_error};
