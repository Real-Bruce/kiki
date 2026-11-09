mod annotation_render;
mod capture;
mod composition;
#[cfg(feature = "diagnostics")]
mod diagnostics;
mod display;
mod icon;
mod menu;
mod output;
#[cfg(feature = "diagnostics")]
mod performance;
mod pinned;
mod pinned_device;
mod pinned_render;
mod png;
mod preview;
mod runtime;
mod selection_render;
mod settings;
mod settings_render;
#[cfg(test)]
mod tests;
mod text_input;
mod text_viewer;
mod toolbar_icons;
mod ui_palette;
mod window_targets;

use crate::core::session::{SessionGate, SessionId};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};
use windows::{
    Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, RemoteDesktop::*, Threading::CreateMutexW},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{Error, Result, w},
};

const TRAY: u32 = WM_APP + 1;
thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }
struct App {
    receiver: Option<mpsc::Receiver<(SessionId, Result<capture::CaptureResult>)>>,
    gate: SessionGate,
    cancel: Option<Arc<AtomicBool>>,
    worker: Option<CaptureWorker>,
    preview: Option<preview::Preview>,
    select_screen: bool,
    settings_during_capture: Option<HWND>,
}
struct AppStateGuard;
impl Drop for AppStateGuard {
    fn drop(&mut self) {
        // Take ownership first: window destruction and thread cleanup may reenter callbacks.
        let state = APP.with_borrow_mut(Option::take);
        settings::close();
        pinned::close_all();
        pinned_render::shutdown();
        if let Some(mut state) = state {
            state.gate.cancel();
            if let Some(cancel) = state.cancel.take() {
                cancel.store(true, Ordering::Release);
            }
            if let Some(worker) = state.worker.take() {
                worker.stop();
            }
        }
    }
}
struct Instance(HANDLE);
struct HiddenState {
    hwnd: Cell<Option<HWND>>,
    notifications: Cell<bool>,
}
struct HiddenWindow(Rc<HiddenState>);
impl HiddenWindow {
    fn own(hwnd: HWND) -> Self {
        let lifetime = Rc::new(HiddenState {
            hwnd: Cell::new(Some(hwnd)),
            notifications: Cell::new(false),
        });
        // SAFETY: fresh UI-thread window has no userdata; the callback reclaims this one Box on NCDESTROY.
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                Box::into_raw(Box::new(lifetime.clone())) as isize,
            );
        }
        Self(lifetime)
    }
}
impl Drop for HiddenWindow {
    fn drop(&mut self) {
        if let Some(hwnd) = self.0.hwnd.get() {
            destroy_hidden(hwnd);
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: wrapper uniquely owns the mutex handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct Tray(NOTIFYICONDATAW);
impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: notification data remains valid until icon removal.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.0);
        }
    }
}
pub fn show_error(message: &str) {
    #[cfg(test)]
    tests::record_ui_error(message);
    #[cfg(not(test))]
    {
        let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
        // SAFETY: NUL-terminated strings remain live for the modal call.
        unsafe {
            MessageBoxW(
                None,
                windows::core::PCWSTR(text.as_ptr()),
                w!("kiki"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

pub fn run() -> Result<()> {
    let _runtime = runtime::Runtime::initialize()?;
    #[cfg(feature = "diagnostics")]
    if diagnostics::run_if_requested()? {
        return Ok(());
    }
    #[cfg(feature = "diagnostics")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--tray-menu")
    {
        return probe_tray_menu();
    }
    #[cfg(feature = "diagnostics")]
    if let Some(directory) = std::env::args_os()
        .nth(1)
        .filter(|a| a == "--probe")
        .and_then(|_| std::env::args_os().nth(2))
    {
        let folder = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&folder).map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        let result = std::thread::Builder::new()
            .name("capture-probe".into())
            .spawn(capture::desktop)
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?
            .join()
            .map_err(|_| Error::new(E_FAIL, "诊断线程意外退出"))??;
        png::save(&result.image, &folder.join("capture.png"))?;
        let bounds = result.image.bounds();
        let quarter = |a: i32, b: i32, n: i64| {
            i32::try_from(i64::from(a) + (i64::from(b) - i64::from(a)) * n / 4)
                .map_err(|_| Error::new(E_FAIL, "裁剪坐标溢出"))
        };
        let region = result
            .image
            .region(crate::core::geometry::PixelRect {
                left: quarter(bounds.left, bounds.right, 1)?,
                top: quarter(bounds.top, bounds.bottom, 1)?,
                right: quarter(bounds.left, bounds.right, 3)?,
                bottom: quarter(bounds.top, bounds.bottom, 3)?,
            })
            .map_err(|e| Error::new(E_FAIL, e))?;
        png::save(&region, &folder.join("capture-region.png"))?;
        std::fs::write(
            folder.join("capture.txt"),
            format!(
                "WGC首帧及读回合计: {:?}\n显示器: {}\n范围: {:?}\n屏幕信息: {:#?}\n",
                result.elapsed,
                result.screens.len(),
                result.image.bounds(),
                result.screens
            ),
        )
        .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        return Ok(());
    }
    // SAFETY: window/tray APIs run on the main thread; pointers below are call-scoped.
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\kiki.WindowsV1"))?;
        let mutex_status = GetLastError();
        let _instance = Instance(mutex);
        if mutex_status == ERROR_ALREADY_EXISTS {
            if std::env::args_os().any(|a| a == "--settings") {
                if let Ok(existing) = FindWindowW(w!("kiki.Tray"), None) {
                    let _ = PostMessageW(
                        Some(existing),
                        preferences::OPEN_SETTINGS,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
            if let Ok(existing) = FindWindowW(w!("kiki.Tray"), None) {
                if std::env::args_os().any(|a| a == "--capture" || a == "--capture-screen") {
                    let msg = if std::env::args_os().any(|a| a == "--capture-screen") {
                        preferences::FULLSCREEN
                    } else {
                        preferences::CAPTURE
                    };
                    let _ = PostMessageW(Some(existing), msg, WPARAM(0), LPARAM(0));
                }
            }
            return Ok(());
        }
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            hIcon: icon::application(module.into(), false)?,
            lpszClassName: w!("kiki.Tray"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(Error::from_win32());
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("kiki"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        let _window = HiddenWindow::own(hwnd);
        let (_preferences, warning) = preferences::initialize(hwnd)?;
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: icon::application(module.into(), true)?,
            ..Default::default()
        };
        for (dst, src) in data.szTip.iter_mut().zip(tray_tip().encode_utf16()) {
            *dst = src;
        }
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            // Shell_NotifyIcon documents only BOOL success, not a reliable last-error code.
            // Return a stable error before RAII destroys the hidden window and changes thread error state.
            return Err(Error::new(
                E_FAIL,
                "无法创建托盘图标，请确认桌面任务栏可用，并在正常桌面环境中重试。",
            ));
        }
        let _tray = Tray(data);
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        _window.0.notifications.set(true);
        APP.with_borrow_mut(|app| {
            *app = Some(App {
                receiver: None,
                gate: SessionGate::default(),
                cancel: None,
                worker: None,
                preview: None,
                select_screen: false,
                settings_during_capture: None,
            })
        });
        let app_state = AppStateGuard;
        preview::register(module.into())?;
        pinned::register(module.into())?;
        #[cfg(feature = "diagnostics")]
        if std::env::args_os().any(|a| a == "--pin-demo") {
            pinned::demo()?;
        }
        #[cfg(feature = "diagnostics")]
        if std::env::args_os().any(|a| a == "--pin-4k-demo") {
            pinned::demo_4k()?;
        }
        #[cfg(feature = "diagnostics")]
        let readiness_probe = diagnostics::start_readiness_probe(hwnd, preferences::active())?;
        #[cfg(feature = "diagnostics")]
        diagnostics::start_pin_observer(hwnd)?;
        #[cfg(feature = "diagnostics")]
        performance::start(hwnd)?;
        if std::env::args_os().any(|arg| arg == "--capture-screen") {
            PostMessageW(Some(hwnd), preferences::FULLSCREEN, WPARAM(0), LPARAM(0))?;
        }
        if std::env::args_os().any(|arg| arg == "--capture") {
            PostMessageW(Some(hwnd), preferences::CAPTURE, WPARAM(0), LPARAM(0))?;
        }
        let show_settings = std::env::args_os().any(|arg| arg == "--settings")
            || (!preferences::current().first_tip_seen && std::env::args_os().nth(1).is_none());
        if show_settings {
            settings::open()?;
            preferences::record_first_use();
        }
        if let Some(warning) = warning {
            show_error(&warning);
        }
        if !show_settings && !preferences::active() {
            let mut balloon = _tray.0;
            balloon.uFlags = NIF_INFO;
            for (dst, src) in balloon
                .szInfoTitle
                .iter_mut()
                .zip("截图快捷键暂不可用".encode_utf16())
            {
                *dst = src;
            }
            for (dst, src) in balloon
                .szInfo
                .iter_mut()
                .zip("可从托盘开始截图，或在设置里更换快捷键。".encode_utf16())
            {
                *dst = src;
            }
            let _ = Shell_NotifyIconW(NIM_MODIFY, &balloon);
        }
        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status == -1 {
                return Err(Error::from_win32());
            }
            if status == 0 {
                break;
            }
            if settings::route(&message) {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        drop(app_state);
        #[cfg(feature = "diagnostics")]
        if let Some(probe) = readiness_probe {
            probe
                .join()
                .map_err(|_| Error::new(E_FAIL, "启动采样线程意外退出"))??;
        }
    }
    Ok(())
}

fn tray_tip() -> String {
    preferences::active_shortcut().map_or_else(|| "kiki".into(), |key| format!("kiki（{key}）"))
}

fn begin_capture_from_settings(hwnd: HWND) {
    let ready = APP.with_borrow(|app| {
        app.as_ref().is_some_and(|state| {
            state.receiver.is_none() && state.preview.as_ref().is_none_or(|p| !p.is_open())
        })
    });
    if !ready {
        return;
    }
    let settings = settings::hide_for_capture();
    match try_begin_capture_mode(hwnd, false) {
        Ok(()) => APP.with_borrow_mut(|app| {
            if let Some(state) = app {
                state.settings_during_capture = settings;
            }
        }),
        Err(message) => {
            settings::restore_after_capture_failure(settings);
            report_capture_error(&message);
        }
    }
}

fn begin_capture(hwnd: HWND) {
    begin_capture_mode(hwnd, false);
}
fn begin_capture_mode(hwnd: HWND, select_screen: bool) {
    let result = try_begin_capture_mode(hwnd, select_screen);
    // Error UI can run a nested message loop; no application-state borrow may remain.
    if let Err(message) = result {
        report_capture_error(&message);
    }
}

fn report_capture_error(message: &str) {
    #[cfg(test)]
    {
        tests::record_error(message);
    }
    #[cfg(not(test))]
    {
        show_error(message);
    }
}

fn try_begin_capture_mode(hwnd: HWND, select_screen: bool) -> std::result::Result<(), String> {
    APP.with_borrow_mut(|app| {
        let Some(state) = app else {
            return Ok(());
        };
        if state.receiver.is_some() || state.preview.as_ref().is_some_and(|p| p.is_open()) {
            return Ok(());
        }
        state.preview = None;
        state.select_screen = select_screen;
        state.gate.cancel();
        let ticket = match state.gate.start() {
            Ok(Some(ticket)) => ticket,
            Ok(None) => return Ok(()),
            Err(message) => {
                return Err(message.into());
            }
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        if !start_capture_timer(hwnd) {
            state.gate.cancel();
            return Err("无法接收截图完成通知，请稍后重试。".into());
        }
        match submit_capture(&mut state.worker, ticket, worker_cancel, tx) {
            Ok(()) => {
                state.receiver = Some(rx);
                state.cancel = Some(cancel);
                Ok(())
            }
            Err(e) => {
                state.gate.cancel();
                // SAFETY: the request was not queued; roll back this UI thread's capture timer.
                unsafe {
                    let _ = KillTimer(Some(hwnd), 1);
                }
                Err(format!("无法启动捕获任务：{e}"))
            }
        }
    })
}

fn start_capture_timer(hwnd: HWND) -> bool {
    #[cfg(test)]
    if tests::take_fault(tests::Fault::Timer) {
        return false;
    }
    // SAFETY: caller is the owning UI thread, with a live hidden window and no timer callback.
    unsafe { SetTimer(Some(hwnd), 1, 15, None) != 0 }
}

struct CaptureRequest {
    ticket: SessionId,
    cancel: Arc<AtomicBool>,
    reply: mpsc::SyncSender<(SessionId, Result<capture::CaptureResult>)>,
    #[cfg(test)]
    fault: Option<tests::Fault>,
}

impl CaptureRequest {
    fn run(self) {
        #[cfg(test)]
        if let Some(fault) = self.fault {
            if fault == tests::Fault::CaptureError {
                let _ = self
                    .reply
                    .send((self.ticket, Err(Error::new(E_FAIL, "测试注入：捕获失败"))));
            }
            if fault == tests::Fault::LateFrame {
                let result = tests::late_frame(&self.cancel);
                let _ = self.reply.send((self.ticket, result));
            }
            return;
        }
        let result = capture::desktop_with_cancel(&self.cancel);
        let _ = self.reply.send((self.ticket, result));
    }
}

struct CaptureWorker {
    requests: mpsc::SyncSender<CaptureRequest>,
    thread: JoinHandle<()>,
}

impl CaptureWorker {
    fn start() -> std::io::Result<Self> {
        let (requests, queue) = mpsc::sync_channel::<CaptureRequest>(1);
        let thread = std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || {
                for request in queue {
                    request.run();
                }
            })?;
        Ok(Self { requests, thread })
    }

    fn stop(self) {
        drop(self.requests);
        let _ = self.thread.join();
    }
}

fn submit_capture(
    worker: &mut Option<CaptureWorker>,
    ticket: SessionId,
    cancel: Arc<AtomicBool>,
    reply: mpsc::SyncSender<(SessionId, Result<capture::CaptureResult>)>,
) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::take_fault(tests::Fault::Spawn) {
        return Err(std::io::Error::other("测试注入：工作线程创建失败"));
    }
    let request = CaptureRequest {
        ticket,
        cancel,
        reply,
        #[cfg(test)]
        fault: tests::take_worker_fault(),
    };
    if let Some(active) = worker.as_ref().filter(|w| !w.thread.is_finished()) {
        return active
            .requests
            .try_send(request)
            .map_err(std::io::Error::other);
    }
    let new_worker = CaptureWorker::start()?;
    let result = new_worker
        .requests
        .try_send(request)
        .map_err(std::io::Error::other);
    *worker = Some(new_worker);
    result
}

fn cancel_session() {
    let preview = APP.with_borrow_mut(|app| {
        let state = app.as_mut()?;
        state.gate.cancel();
        if let Some(cancel) = state.cancel.as_ref() {
            cancel.store(true, Ordering::Release);
        }
        state.preview.take()
    });
    // Window destruction can reenter the dispatcher; release the state borrow first.
    drop(preview);
}

fn destroy_hidden(hwnd: HWND) {
    // SAFETY: called only on the owning UI thread; userdata remains installed until NCDESTROY.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Rc<HiddenState>;
        if !pointer.is_null() && (&*pointer).notifications.replace(false) {
            let _ = WTSUnRegisterSessionNotification(hwnd);
        }
        let _ = DestroyWindow(hwnd);
    }
}

fn poll(hwnd: HWND) {
    let result = APP.with_borrow_mut(|app| {
        let state = app.as_mut()?;
        let (ticket, result) = match state.receiver.as_ref()?.try_recv() {
            Ok(message) => message,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(_) => {
                state.receiver = None;
                state.cancel = None;
                state.gate.cancel();
                return Some(Err(Error::new(E_FAIL, "捕获线程意外退出")));
            }
        };
        state.receiver = None;
        state.cancel = None;
        if !state.gate.accept_frame(ticket) {
            // A display cancellation invalidates this generation; dropping releases pixels.
            return Some(Ok(None));
        }
        match result {
            Ok(frame) => Some(Ok(Some(frame))),
            Err(error) => {
                state.gate.cancel();
                Some(Err(error))
            }
        }
    });
    if let Some(result) = result {
        let settings = APP.with_borrow_mut(|app| {
            app.as_mut()
                .and_then(|state| state.settings_during_capture.take())
        });
        // SAFETY: timer belongs to the hidden tray window.
        unsafe {
            let _ = KillTimer(Some(hwnd), 1);
        }
        let result = match result {
            Ok(Some(frame)) => {
                let mode = APP.with_borrow(|app| app.as_ref().is_some_and(|s| s.select_screen));
                preview::Preview::open_mode(frame, mode)
            }
            Ok(None) => {
                settings::restore_after_capture_failure(settings);
                return;
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(preview) => {
                #[cfg(feature = "diagnostics")]
                let opened = preview.hwnd();
                APP.with_borrow_mut(|app| {
                    if let Some(state) = app {
                        state.preview = Some(preview);
                    }
                });
                #[cfg(feature = "diagnostics")]
                if let Some(opened) = opened {
                    performance::ready(opened);
                }
            }
            Err(e) => {
                settings::restore_after_capture_failure(settings);
                report_capture_error(&format!("无法完成截图，请重试。\n{e}"));
            }
        }
    }
}

#[cfg(feature = "diagnostics")]
fn probe_tray_menu() -> Result<()> {
    use windows::Win32::Graphics::Gdi::*;
    // SAFETY: the diagnostic owner and native popup belong to this process's UI thread.
    // No tray registration, preferences, or user commands are used by this probe.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: w!("kiki.TrayMenuProbe"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(Error::from_win32());
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.lpszClassName,
            w!("kiki 托盘菜单预览"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        let _owner = HiddenWindow::own(hwnd);
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            point.x = info.rcWork.left + (info.rcWork.right - info.rcWork.left) / 2;
            point.y = info.rcWork.top + (info.rcWork.bottom - info.rcWork.top) / 2;
        }
        let _ = menu::tray(hwnd, point.x, point.y);
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(|| dispatch(hwnd, msg, wparam, lparam)) {
        Ok(result) => result,
        Err(_) => {
            show_error("窗口操作失败，请重试。");
            LRESULT(0)
        }
    }
}

fn dispatch(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: parameters originate from Win32; popup menu is destroyed in this call.
    unsafe {
        if let Some(result) = menu::handle(msg, lparam) {
            return result;
        }
        match msg {
            WM_HOTKEY => {
                if preferences::matches(wparam.0) {
                    if settings::hotkey_focused() {
                        settings::restore_hotkey_input();
                    } else {
                        begin_capture(hwnd);
                    }
                }
                LRESULT(0)
            }
            preferences::OPEN_SETTINGS => {
                if let Err(error) = settings::open() {
                    show_error(&error.to_string());
                }
                LRESULT(0)
            }
            preferences::CAPTURE_FROM_SETTINGS => {
                begin_capture_from_settings(hwnd);
                LRESULT(0)
            }
            preferences::CAPTURE => {
                begin_capture(hwnd);
                LRESULT(0)
            }
            preferences::FULLSCREEN => {
                begin_capture_mode(hwnd, true);
                LRESULT(0)
            }
            config_store::SAVED => {
                preferences::poll();
                settings::refresh();
                LRESULT(0)
            }
            preferences::CHANGED | WM_THEMECHANGED | WM_SETTINGCHANGE => {
                if msg != preferences::CHANGED {
                    preferences::invalidate_system_theme();
                }
                settings::refresh();
                pinned::theme_changed();
                text_viewer::theme_changed();
                let preview = APP.with_borrow(|app| {
                    app.as_ref()
                        .and_then(|s| s.preview.as_ref())
                        .and_then(|p| p.hwnd())
                });
                if let Some(preview) = preview {
                    let _ = PostMessageW(Some(preview), WM_THEMECHANGED, WPARAM(0), LPARAM(0));
                }
                let mut tip = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: hwnd,
                    uID: 1,
                    uFlags: NIF_TIP,
                    ..Default::default()
                };
                for (d, s) in tip.szTip.iter_mut().zip(tray_tip().encode_utf16()) {
                    *d = s;
                }
                let _ = Shell_NotifyIconW(NIM_MODIFY, &tip);
                LRESULT(0)
            }
            #[cfg(feature = "diagnostics")]
            WM_TIMER if wparam.0 == 78 => {
                performance::tick(hwnd);
                LRESULT(0)
            }
            #[cfg(feature = "diagnostics")]
            WM_TIMER if wparam.0 == 77 => {
                diagnostics::pin_observer_tick(hwnd);
                LRESULT(0)
            }
            WM_TIMER => {
                poll(hwnd);
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                cancel_session();
                LRESULT(0)
            }
            WM_WTSSESSION_CHANGE
                if matches!(
                    wparam.0 as u32,
                    WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT
                ) =>
            {
                cancel_session();
                LRESULT(0)
            }
            TRAY if lparam.0 as u32 == WM_LBUTTONUP => {
                begin_capture(hwnd);
                LRESULT(0)
            }
            WM_ENTERIDLE if wparam.0 == MSGF_MENU as usize => {
                menu::style_tray_popup(hwnd, HWND(lparam.0 as *mut _));
                LRESULT(0)
            }
            TRAY if lparam.0 as u32 == WM_RBUTTONUP => {
                let mut point = POINT::default();
                let _ = GetCursorPos(&mut point);
                let command = menu::tray(hwnd, point.x, point.y);
                match command {
                    1 => begin_capture(hwnd),
                    4 => {
                        if let Err(error) = settings::open() {
                            show_error(&error.to_string());
                        }
                    }
                    3 => pinned::close_all(),
                    2 => PostQuitMessage(0),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_CLOSE => {
                // Exit the pump first so workers and registrations drop before the host HWND.
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Rc<HiddenState>;
                if !pointer.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let lifetime = Box::from_raw(pointer);
                    lifetime.hwnd.set(None);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
mod config_store;
mod hotkey;

mod preferences;
mod startup;
