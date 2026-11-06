//! Real hidden-window/message-pump tests. Faults affect this test UI thread only.
use super::*;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fault {
    Timer,
    Spawn,
    CaptureError,
    Disconnected,
    LateFrame,
}
thread_local! {
    static FAULT: Cell<Option<Fault>> = const { Cell::new(None) };
    static ERRORS: RefCell<Vec<(String,bool)>> = const { RefCell::new(Vec::new()) };
}
pub(super) fn take_fault(expected: Fault) -> bool {
    FAULT.with(|fault| {
        if fault.get() == Some(expected) {
            fault.set(None);
            true
        } else {
            false
        }
    })
}
pub(super) fn take_worker_fault() -> Option<Fault> {
    FAULT.with(|fault| match fault.get() {
        Some(Fault::CaptureError | Fault::Disconnected | Fault::LateFrame) => fault.take(),
        _ => None,
    })
}
pub(super) fn record_error(message: &str) {
    let borrow_released = APP.with(|app| app.try_borrow_mut().is_ok());
    ERRORS.with_borrow_mut(|errors| errors.push((message.into(), borrow_released)));
}

pub(super) fn late_frame(cancel: &AtomicBool) -> Result<capture::CaptureResult> {
    let until = Instant::now() + Duration::from_secs(2);
    while !cancel.load(Ordering::Acquire) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(2));
    }
    if !cancel.load(Ordering::Acquire) {
        return Err(Error::new(E_FAIL, "测试未收到取消信号"));
    }
    // Deliberate one-pixel fixture, never a real screenshot or exported validation image.
    let image = crate::core::image::Image::new(crate::core::geometry::PixelRect {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
    })
    .map_err(|e| Error::new(E_FAIL, e))?;
    Ok(capture::CaptureResult {
        image,
        #[cfg(feature = "diagnostics")]
        elapsed: Duration::ZERO,
        screens: Vec::new(),
        windows: Vec::new(),
    })
}

fn pump_for(duration: Duration) -> usize {
    let until = Instant::now() + duration;
    let mut timer_messages = 0;
    while Instant::now() < until {
        // SAFETY: this dedicated test thread owns the hidden window and pumps its real Win32 messages.
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_TIMER {
                    timer_messages += 1;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    timer_messages
}

fn assert_idle() {
    APP.with_borrow(|app| {
        let state = app.as_ref().unwrap();
        assert!(state.receiver.is_none());
        assert!(state.cancel.is_none());
        assert!(state.worker.is_none());
        assert!(state.preview.is_none());
    });
}

#[test]
fn native_failure_paths_release_state_and_allow_retry() {
    std::thread::Builder::new()
        .name("native-ui-test".into())
        .spawn(|| {
            // SAFETY: test window/class live on this thread; callback remains valid until class unregister.
            unsafe {
                let module = GetModuleHandleW(None).unwrap();
                let class = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: module.into(),
                    lpszClassName: w!("kiki.FailureTest"),
                    ..Default::default()
                };
                assert_ne!(RegisterClassW(&class), 0);
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class.lpszClassName,
                    w!("kiki 测试窗口"),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(module.into()),
                    None,
                )
                .unwrap();
                let owned = HiddenWindow::own(hwnd);
                let lifetime = owned.0.clone();
                APP.with_borrow_mut(|app| {
                    *app = Some(App {
                        receiver: None,
                        gate: SessionGate::default(),
                        cancel: None,
                        worker: None,
                        preview: None,
                    })
                });

                for fault in [Fault::Timer, Fault::Spawn] {
                    FAULT.set(Some(fault));
                    begin_capture(hwnd);
                    assert_idle();
                    // KillTimer may leave one already-queued WM_TIMER; it must not keep generating ticks.
                    assert!(pump_for(Duration::from_millis(120)) <= 1);
                }
                for fault in [
                    Fault::CaptureError,
                    Fault::Disconnected,
                    Fault::CaptureError,
                ] {
                    FAULT.set(Some(fault));
                    begin_capture(hwnd);
                    begin_capture(hwnd); // Active request must not create a second worker.
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while APP.with_borrow(|app| app.as_ref().unwrap().receiver.is_some())
                        && Instant::now() < deadline
                    {
                        pump_for(Duration::from_millis(10));
                    }
                    assert_idle();
                    assert!(pump_for(Duration::from_millis(120)) <= 1);
                }
                for (message, reason) in [
                    (WM_DISPLAYCHANGE, 0),
                    (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK),
                    (WM_WTSSESSION_CHANGE, WTS_CONSOLE_DISCONNECT),
                    (WM_WTSSESSION_CHANGE, WTS_REMOTE_DISCONNECT),
                ] {
                    FAULT.set(Some(Fault::LateFrame));
                    begin_capture(hwnd);
                    // Internal event injection exercises the callback; no actual display/session setting changes.
                    SendMessageW(
                        hwnd,
                        message,
                        Some(WPARAM(reason as usize)),
                        Some(LPARAM(0)),
                    );
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while APP.with_borrow(|app| app.as_ref().unwrap().receiver.is_some())
                        && Instant::now() < deadline
                    {
                        pump_for(Duration::from_millis(10));
                    }
                    assert_idle(); // No preview may open for the deliberately late result.
                }
                ERRORS.with_borrow(|errors| {
                    assert_eq!(errors.len(), 5);
                    assert!(
                        errors.iter().all(|(_, released)| *released),
                        "报告错误时应用状态仍被借用"
                    );
                    assert!(errors[0].0.contains("完成通知"));
                    assert!(errors[1].0.contains("启动捕获任务"));
                    assert!(errors[2].0.contains("捕获失败"));
                    assert!(errors[3].0.contains("意外退出"));
                });
                FAULT.set(Some(Fault::LateFrame));
                begin_capture(hwnd);
                let shutdown_started = Instant::now();
                drop(AppStateGuard);
                assert!(shutdown_started.elapsed() < Duration::from_secs(2));
                assert!(APP.with_borrow(|app| app.is_none()));
                drop(owned);
                assert!(lifetime.hwnd.get().is_none());
                UnregisterClassW(class.lpszClassName, Some(module.into())).unwrap();
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
