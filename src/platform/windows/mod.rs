mod capture;
#[cfg(feature = "diagnostics")]
mod diagnostics;
mod display;
#[cfg(any(feature = "diagnostics", test))]
mod png;
mod preview;
mod runtime;
mod selection_render;
#[cfg(test)]
mod tests;
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
        UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::{Error, Result, w},
};

const TRAY: u32 = WM_APP + 1;
thread_local! { static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }
struct App {
    receiver: Option<mpsc::Receiver<(SessionId, Result<capture::CaptureResult>)>>,
    gate: SessionGate,
    cancel: Option<Arc<AtomicBool>>,
    worker: Option<JoinHandle<()>>,
    preview: Option<preview::Preview>,
}
struct AppStateGuard;
impl Drop for AppStateGuard {
    fn drop(&mut self) {
        // Take ownership first: window destruction and thread cleanup may reenter callbacks.
        let state = APP.with_borrow_mut(Option::take);
        if let Some(mut state) = state {
            state.gate.cancel();
            if let Some(cancel) = state.cancel.take() {
                cancel.store(true, Ordering::Release);
            }
            if let Some(worker) = state.worker.take() {
                let _ = worker.join();
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
struct Hotkey(HWND);
impl Drop for Hotkey {
    fn drop(&mut self) {
        // SAFETY: this UI thread owns the registered hotkey.
        unsafe {
            let _ = UnregisterHotKey(Some(self.0), 1);
        }
    }
}

pub fn show_error(message: &str) {
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    // SAFETY: NUL-terminated strings remain live for the modal call.
    unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(text.as_ptr()),
            w!("iCapture"),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn run() -> Result<()> {
    let _runtime = runtime::Runtime::initialize()?;
    #[cfg(feature = "diagnostics")]
    if diagnostics::run_if_requested()? {
        return Ok(());
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
        let mutex = CreateMutexW(None, false, w!("Local\\iCapture.WindowsV1"))?;
        let _instance = Instance(mutex);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: module.into(),
            lpszClassName: w!("iCapture.Tray"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(Error::from_win32());
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!("iCapture"),
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
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: LoadIconW(None, IDI_APPLICATION)?,
            ..Default::default()
        };
        for (dst, src) in data
            .szTip
            .iter_mut()
            .zip("iCapture · Ctrl+Alt+A".encode_utf16())
        {
            *dst = src;
        }
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            let _ = DestroyWindow(hwnd);
            return Err(Error::from_win32());
        }
        let _tray = Tray(data);
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        _window.0.notifications.set(true);
        let _hotkey = match RegisterHotKey(
            Some(hwnd),
            1,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            u32::from(b'A'),
        ) {
            Ok(()) => Some(Hotkey(hwnd)),
            Err(_) => {
                #[cfg(feature = "diagnostics")]
                let is_startup_probe = std::env::args_os().any(|a| a == "--startup-probe");
                #[cfg(not(feature = "diagnostics"))]
                let is_startup_probe = false;
                if !is_startup_probe {
                    show_error("Ctrl+Alt+A 已被占用，仍可通过托盘开始截图。");
                }
                None
            }
        };
        APP.with_borrow_mut(|app| {
            *app = Some(App {
                receiver: None,
                gate: SessionGate::default(),
                cancel: None,
                worker: None,
                preview: None,
            })
        });
        let app_state = AppStateGuard;
        preview::register(module.into())?;
        #[cfg(feature = "diagnostics")]
        let readiness_probe = diagnostics::start_readiness_probe(hwnd, _hotkey.is_some())?;
        if std::env::args_os().any(|arg| arg == "--capture") {
            PostMessageW(Some(hwnd), WM_HOTKEY, WPARAM(1), LPARAM(0))?;
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

fn begin_capture(hwnd: HWND) {
    let result = try_begin_capture(hwnd);
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

fn try_begin_capture(hwnd: HWND) -> std::result::Result<(), String> {
    APP.with_borrow_mut(|app| {
        let Some(state) = app else {
            return Ok(());
        };
        if state.receiver.is_some() || state.preview.as_ref().is_some_and(|p| p.is_open()) {
            return Ok(());
        }
        state.preview = None;
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
        match spawn_capture(ticket, worker_cancel, tx) {
            Ok(worker) => {
                state.receiver = Some(rx);
                state.cancel = Some(cancel);
                state.worker = Some(worker);
                Ok(())
            }
            Err(e) => {
                state.gate.cancel();
                // SAFETY: no worker was started; roll back this UI thread's capture timer.
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

fn spawn_capture(
    ticket: SessionId,
    cancel: Arc<AtomicBool>,
    tx: mpsc::SyncSender<(SessionId, Result<capture::CaptureResult>)>,
) -> std::io::Result<JoinHandle<()>> {
    #[cfg(test)]
    if tests::take_fault(tests::Fault::Spawn) {
        return Err(std::io::Error::other("测试注入：工作线程创建失败"));
    }
    #[cfg(test)]
    let fault = tests::take_worker_fault();
    std::thread::Builder::new()
        .name("capture".into())
        .spawn(move || {
            #[cfg(test)]
            if let Some(fault) = fault {
                if fault == tests::Fault::CaptureError {
                    let _ = tx.send((ticket, Err(Error::new(E_FAIL, "测试注入：捕获失败"))));
                }
                if fault == tests::Fault::LateFrame {
                    let result = tests::late_frame(&cancel);
                    let _ = tx.send((ticket, result));
                }
                return;
            }
            let result = capture::desktop_with_cancel(&cancel);
            let _ = tx.send((ticket, result));
        })
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
                if let Some(worker) = state.worker.take() {
                    let _ = worker.join();
                }
                return Some(Err(Error::new(E_FAIL, "捕获线程意外退出")));
            }
        };
        state.receiver = None;
        state.cancel = None;
        if let Some(worker) = state.worker.take() {
            let _ = worker.join();
        }
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
        // SAFETY: timer belongs to the hidden tray window.
        unsafe {
            let _ = KillTimer(Some(hwnd), 1);
        }
        let result = match result {
            Ok(Some(frame)) => preview::Preview::open(frame),
            Ok(None) => return,
            Err(error) => Err(error),
        };
        match result {
            Ok(preview) => APP.with_borrow_mut(|app| {
                if let Some(state) = app {
                    state.preview = Some(preview);
                }
            }),
            Err(e) => report_capture_error(&format!("截图失败，已返回托盘：{e}")),
        }
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
        match msg {
            WM_HOTKEY => {
                begin_capture(hwnd);
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
            TRAY if lparam.0 as u32 == WM_RBUTTONUP => {
                if let Ok(menu) = CreatePopupMenu() {
                    let _ = AppendMenuW(menu, MF_STRING, 1, w!("开始截图"));
                    let _ = AppendMenuW(menu, MF_STRING, 2, w!("退出"));
                    let mut point = POINT::default();
                    let _ = GetCursorPos(&mut point);
                    let _ = SetForegroundWindow(hwnd);
                    let command = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_RIGHTBUTTON,
                        point.x,
                        point.y,
                        None,
                        hwnd,
                        None,
                    )
                    .0;
                    let _ = DestroyMenu(menu);
                    if command == 1 {
                        begin_capture(hwnd);
                    } else if command == 2 {
                        destroy_hidden(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_CLOSE => {
                destroy_hidden(hwnd);
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
