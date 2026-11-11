//! Independent topmost image windows. Each HWND owns one State until NCDESTROY.
use super::{
    output::{self, Completed, Destination, Job},
    pinned_render::{self, Canvas},
};
use crate::core::{image::Image, pinned::View, selection::PixelPoint};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::Arc,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::WM_MOUSELEAVE, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*,
        },
    },
    core::{Error, Result, w},
};
thread_local! {static WINDOWS:RefCell<Vec<Entry>>=const{RefCell::new(Vec::new())};}
struct Entry {
    life: Weak<Cell<Option<HWND>>>,
    bytes: usize,
}
struct State {
    life: Rc<Cell<Option<HWND>>>,
    image: Arc<Image>,
    canvas: Option<Canvas>,
    view: View,
    minimum: (i32, i32),
    dpi: u32,
    hover: bool,
    down: Option<usize>,
    notice: Option<String>,
    busy: bool,
    job: Option<Job>,
}
impl State {
    fn paint(&mut self, hwnd: HWND) -> Result<()> {
        let draw = |canvas: &Canvas| {
            canvas.paint(
                self.view,
                self.minimum,
                self.dpi,
                self.hover,
                self.notice.as_deref(),
                self.busy,
            )
        };
        if let Some(canvas) = self.canvas.as_ref() {
            if draw(canvas).is_ok() {
                return Ok(());
            }
        }
        // A flip-model HWND accepts only one swap chain: release the old one before recreating it.
        self.canvas = None;
        let (w, h) = self.view.dimensions(self.minimum);
        let canvas = Canvas::new(hwnd, &self.image, w, h, self.dpi)?;
        draw(&canvas)?;
        self.canvas = Some(canvas);
        Ok(())
    }
}
struct Creating(HWND);
impl Drop for Creating {
    fn drop(&mut self) {
        // SAFETY: guard uniquely owns a not-yet-published UI-thread HWND, including partial initialization.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
fn minimum(dpi: u32) -> (i32, i32) {
    (
        (144. * dpi as f64 / 96.).ceil() as i32,
        (64. * dpi as f64 / 96.).ceil() as i32,
    )
}
fn fail(text: &str) -> Error {
    Error::new(E_FAIL, text)
}
pub fn register(instance: HINSTANCE) -> Result<()> {
    // SAFETY: static callback/class name and shared system resources remain live for the process.
    unsafe {
        let class = WNDCLASSW {
            style: CS_DBLCLKS | CS_DROPSHADOW,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hIcon: super::icon::application(instance, false)?,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("kiki.Pinned"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(Error::from_win32());
        }
    }
    Ok(())
}
pub fn open(image: Arc<Image>, anchor: PixelPoint) -> Result<()> {
    open_inner(image, anchor, !cfg!(test))
}
fn open_inner(image: Arc<Image>, anchor: PixelPoint, visible: bool) -> Result<()> {
    let bytes = image.pixels().len();
    let allowed = WINDOWS.with_borrow_mut(|windows| {
        windows.retain(|entry| {
            entry
                .life
                .upgrade()
                .is_some_and(|life| life.get().is_some())
        });
        windows.len() < 64
            && windows
                .iter()
                .map(|entry| entry.bytes)
                .sum::<usize>()
                .saturating_add(bytes)
                <= 512 * 1024 * 1024
    });
    if !allowed {
        return Err(fail(
            "贴图已达到 64 张或 512 MiB 图像内存上限，请先关闭部分贴图",
        ));
    }
    let (width, height) = image.bounds().size().ok_or_else(|| fail("无效贴图尺寸"))?;
    // SAFETY: all window and monitor operations run on the UI thread; State is published once initialization is complete.
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: anchor.x,
                y: anchor.y,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(Error::from_win32());
        }
        let mut dpi = 96;
        let mut dpi_y = 96;
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let minimum = minimum(dpi);
        let work = info.rcWork;
        let view = View::fit(
            width,
            height,
            (work.right - work.left - 32).max(1),
            (work.bottom - work.top - 32).max(1),
        )
        .ok_or_else(|| fail("无效显示区域"))?;
        let (w, h) = view.dimensions(minimum);
        let x = anchor
            .x
            .clamp(work.left, work.right.saturating_sub(w).max(work.left));
        let y = anchor
            .y
            .clamp(work.top, work.bottom.saturating_sub(h).max(work.top));
        #[cfg(feature = "diagnostics")]
        let accessible = std::env::args_os().any(|a| a == "--accessible-capture");
        #[cfg(not(feature = "diagnostics"))]
        let accessible = false;
        let hwnd = CreateWindowExW(
            if accessible {
                WS_EX_TOPMOST
            } else {
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW
            },
            w!("kiki.Pinned"),
            w!("kiki 贴图"),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?;
        let creating = Creating(hwnd);
        // Non-text surfaces handle command keys; text composition belongs exclusively to the native EDIT.
        let _ = windows::Win32::UI::Input::Ime::ImmAssociateContextEx(
            hwnd,
            windows::Win32::UI::Input::Ime::HIMC::default(),
            0,
        );
        let life = Rc::new(Cell::new(Some(hwnd)));
        let canvas = Canvas::new(hwnd, &image, w, h, dpi)?;
        let corner = DWMWCP_ROUNDSMALL;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&corner).cast(),
            std::mem::size_of_val(&corner) as u32,
        );
        let state = State {
            life: life.clone(),
            image,
            canvas: Some(canvas),
            view,
            minimum,
            dpi,
            hover: false,
            down: None,
            notice: None,
            busy: false,
            job: None,
        };
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(state)) as isize);
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            w,
            h,
            if visible {
                SWP_SHOWWINDOW
            } else {
                SWP_NOACTIVATE
            },
        )?;
        WINDOWS.with_borrow_mut(|windows| {
            windows.push(Entry {
                life: Rc::downgrade(&life),
                bytes,
            })
        });
        std::mem::forget(creating);
    }
    Ok(())
}
#[cfg(feature = "diagnostics")]
pub fn count() -> usize {
    WINDOWS.with_borrow(|windows| {
        windows
            .iter()
            .filter(|entry| {
                entry
                    .life
                    .upgrade()
                    .is_some_and(|life| life.get().is_some())
            })
            .count()
    })
}
pub fn theme_changed() {
    let handles: Vec<_> = WINDOWS.with_borrow(|windows| {
        windows
            .iter()
            .filter_map(|e| e.life.upgrade().and_then(|l| l.get()))
            .collect()
    });
    for hwnd in handles {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_THEMECHANGED, WPARAM(0), LPARAM(0));
        }
    }
}
pub fn close_all() {
    let handles = WINDOWS.with_borrow_mut(|windows| {
        let handles: Vec<_> = windows
            .iter()
            .filter_map(|entry| entry.life.upgrade().and_then(|life| life.get()))
            .collect();

        handles
    });
    for hwnd in handles {
        // SAFETY: weak lifetimes were resolved on this UI thread; registry borrow ended before destruction can reenter.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    WINDOWS.with_borrow_mut(|windows| {
        windows.retain(|entry| {
            entry
                .life
                .upgrade()
                .is_some_and(|life| life.get().is_some())
        })
    });
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(|| dispatch(hwnd, msg, wp, lp)) {
        Ok(value) => value,
        Err(_) => {
            super::show_error("贴图操作失败，请关闭后重新创建贴图。");
            LRESULT(0)
        }
    }
}
fn point(lp: LPARAM) -> (i32, i32) {
    (
        (lp.0 as u16 as i16) as i32,
        ((lp.0 as usize >> 16) as u16 as i16) as i32,
    )
}
fn dispatch(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: userdata holds the unique State Box. No Rust reference to it is retained over modal/native window operations.
    unsafe {
        if let Some(result) = super::menu::handle(msg, lp) {
            return result;
        }
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if msg == WM_NCDESTROY && !pointer.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let state = Box::from_raw(pointer);
            state.life.set(None);
            drop(state);
            if !WINDOWS.with_borrow(|windows| {
                windows.iter().any(|entry| {
                    entry
                        .life
                        .upgrade()
                        .is_some_and(|life| life.get().is_some())
                })
            }) {
                super::pinned_device::idle();
            }
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if pointer.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let result = (*pointer).paint(hwnd);
                let _ = EndPaint(hwnd, &ps);
                if let Err(e) = result {
                    super::show_error(&format!("贴图显示失败：{e}"));
                }
                LRESULT(0)
            }
            WM_SIZE => {
                let (w, h) = (lp.0 as u16 as i32, ((lp.0 as usize >> 16) as u16) as i32);
                if let Some(canvas) = (*pointer).canvas.as_ref() {
                    let _ = canvas.resize(w, h);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if !(*pointer).hover {
                    (*pointer).hover = true;
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                (*pointer).hover = false;
                (*pointer).down = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let hit = {
                    let s = &*pointer;
                    pinned_render::button(
                        s.view.dimensions(s.minimum).0,
                        point(lp),
                        s.dpi as f32 / 96.,
                    )
                };
                (*pointer).down = hit;
                if hit.is_some() {
                    let _ = SetCapture(hwnd);
                } else {
                    let _ = SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION as usize)),
                        Some(LPARAM(0)),
                    );
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let hit = {
                    let s = &*pointer;
                    pinned_render::button(
                        s.view.dimensions(s.minimum).0,
                        point(lp),
                        s.dpi as f32 / 96.,
                    )
                };
                let down = (*pointer).down.take();
                let life = (*pointer).life.clone();
                let _ = ReleaseCapture();
                if life.get() == Some(hwnd) && down.is_some() && down == hit {
                    action(
                        hwnd,
                        pointer,
                        match hit {
                            Some(0) => 1,
                            Some(1) => 2,
                            _ => 4,
                        },
                    );
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                (*pointer).down = None;
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                if !(*pointer).busy {
                    let delta = ((wp.0 >> 16) as u16 as i16) as f64 / 120.;
                    zoom(hwnd, pointer, Some(1.1f64.powf(delta)), point(lp));
                }
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                zoom(hwnd, pointer, None, (p.x, p.y));
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                let mut p = point(lp);
                if p == (-1, -1) {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    p = (rect.left + 16, rect.top + 16);
                }
                let life = (*pointer).life.clone();
                let command = context_menu(hwnd, p, (*pointer).busy);
                if life.get() == Some(hwnd) {
                    action(hwnd, pointer, command);
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if wp.0 == VK_ESCAPE.0 as usize {
                    let _ = DestroyWindow(hwnd);
                } else if GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    match wp.0 {
                        67 => action(hwnd, pointer, 1),
                        83 => action(hwnd, pointer, 2),
                        48 => {
                            let mut r = RECT::default();
                            let _ = GetWindowRect(hwnd, &mut r);
                            zoom(
                                hwnd,
                                pointer,
                                None,
                                ((r.left + r.right) / 2, (r.top + r.bottom) / 2),
                            );
                        }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == 1 => {
                let result =
                    (*pointer)
                        .job
                        .as_ref()
                        .and_then(|job| match job.receiver.try_recv() {
                            Ok(result) => Some(result),
                            Err(std::sync::mpsc::TryRecvError::Empty) => None,
                            Err(_) => Some(Err(fail("输出线程意外退出"))),
                        });
                if let Some(result) = result {
                    (*pointer).job = None;
                    (*pointer).busy = false;
                    let _ = KillTimer(Some(hwnd), 1);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    match result {
                        Ok(Completed::Done) => notice(hwnd, pointer, "已完成".into()),
                        Ok(Completed::Pinned(_)) => {}
                        Err(e) => super::show_error(&format!("输出失败，贴图已保留：{e}")),
                    }
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == 2 => {
                let _ = KillTimer(Some(hwnd), 2);
                (*pointer).notice = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                (*pointer).dpi = wp.0 as u16 as u32;
                (*pointer).minimum = minimum((*pointer).dpi);
                let rect = *(lp.0 as *const RECT);
                let (w, h) = (*pointer).view.dimensions((*pointer).minimum);
                (*pointer).canvas = None;
                (*pointer).canvas = Canvas::new(hwnd, &(*pointer).image, w, h, (*pointer).dpi).ok();
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    w,
                    h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                LRESULT(0)
            }
            WM_THEMECHANGED | WM_SETTINGCHANGE => {
                if let Some(canvas) = (*pointer).canvas.as_mut() {
                    canvas.dark = super::selection_render::system_dark();
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
fn notice(hwnd: HWND, pointer: *mut State, text: String) {
    // SAFETY: caller checked this UI-thread window lifetime and has no outstanding state borrow.
    unsafe {
        (*pointer).notice = Some(text);
        if SetTimer(Some(hwnd), 2, 1200, None) == 0 {
            (*pointer).notice = None;
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
fn zoom(hwnd: HWND, pointer: *mut State, factor: Option<f64>, screen: (i32, i32)) {
    // SAFETY: UI thread; native calls that can reenter happen after the temporary state borrow ends.
    unsafe {
        if (*pointer).busy {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let (x, y, w, h, percent) = {
            let s = &mut *pointer;
            let requested = factor.map_or(1., |f| s.view.scale * f);
            let (x, y) = s.view.change_scale(
                requested,
                (rect.left, rect.top),
                (screen.0 - rect.left, screen.1 - rect.top),
                s.minimum,
            );
            let (w, h) = s.view.dimensions(s.minimum);
            (x, y, w, h, (s.view.scale * 100.).round())
        };
        if super::preferences::hints() {
            notice(
                hwnd,
                pointer,
                if factor.is_none() && percent < 100. {
                    format!("显示上限 {percent:.0}%")
                } else {
                    format!("{percent:.0}%")
                },
            );
        }
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}
fn context_menu(hwnd: HWND, p: (i32, i32), busy: bool) -> u32 {
    super::menu::popup(
        hwnd,
        p.0,
        p.1,
        super::menu::Style::Pinned,
        &[
            (1, "复制\tCtrl+C", !busy),
            (2, "保存 PNG\tCtrl+S", !busy),
            (3, "恢复原始比例\tCtrl+0", !busy),
            (0, "", true),
            (4, "关闭\tEsc", true),
        ],
    )
}

fn action(hwnd: HWND, pointer: *mut State, command: u32) {
    // SAFETY: caller checked lifetime. Modal save dialog runs without borrowed State and lifetime is checked again afterwards.
    unsafe {
        if command == 4 {
            let _ = DestroyWindow(hwnd);
            return;
        }
        if (*pointer).busy {
            return;
        }
        if command == 3 {
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            zoom(
                hwnd,
                pointer,
                None,
                ((r.left + r.right) / 2, (r.top + r.bottom) / 2),
            );
            return;
        }
        if command != 1 && command != 2 {
            return;
        }
        let life = (*pointer).life.clone();
        (*pointer).busy = true;
        let _ = InvalidateRect(Some(hwnd), None, false);
        let destination = if command == 2 {
            let result = output::save_dialog(hwnd);
            if life.get() != Some(hwnd) {
                return;
            }
            match result {
                Ok(Some(path)) => Destination::File(path),
                Ok(None) => {
                    (*pointer).busy = false;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return;
                }
                Err(e) => {
                    (*pointer).busy = false;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    super::show_error(&e.to_string());
                    return;
                }
            }
        } else {
            Destination::Clipboard
        };
        if SetTimer(Some(hwnd), 1, 20, None) == 0 {
            (*pointer).busy = false;
            super::show_error("无法启动贴图输出状态计时器");
            return;
        }
        match output::start_image((*pointer).image.clone(), hwnd.0 as usize, destination) {
            Ok(job) => (*pointer).job = Some(job),
            Err(e) => {
                (*pointer).busy = false;
                let _ = KillTimer(Some(hwnd), 1);
                let _ = InvalidateRect(Some(hwnd), None, false);
                super::show_error(&e.to_string());
            }
        }
    }
}

#[cfg(feature = "diagnostics")]
pub fn demo() -> Result<()> {
    let (source, editor) = super::composition::fixture()?;
    let selection = source.bounds();
    let image = Arc::new(super::composition::render(&source, selection, &editor, 0)?);
    open(image.clone(), PixelPoint { x: 120, y: 120 })?;
    open(image, PixelPoint { x: 850, y: 300 })
}

#[cfg(feature = "diagnostics")]
pub fn demo_4k() -> Result<()> {
    for index in 0..4 {
        let mut image = Image::new(crate::core::geometry::PixelRect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        })
        .map_err(|e| fail(&e))?;
        for (pixel_index, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
            let x = pixel_index % 3840;
            let y = pixel_index / 3840;
            pixel.copy_from_slice(&[
                (64 + x / 24) as u8,
                (64 + y / 16) as u8,
                64 + index * 40,
                255,
            ]);
        }
        let anchor = PixelPoint {
            x: 160 + 1120 * i32::from(index % 2),
            y: 160 + 700 * i32::from(index / 2),
        };
        open_inner(Arc::new(image), anchor, false)?;
        let hwnd = WINDOWS
            .with_borrow(|windows| {
                windows
                    .last()
                    .and_then(|entry| entry.life.upgrade())
                    .and_then(|life| life.get())
            })
            .ok_or_else(|| fail("测试贴图已关闭"))?;
        // SAFETY: explicit diagnostic initial layout on the owning UI thread; the state borrow ends before sizing sends messages.
        unsafe {
            let (w, h) = {
                let s = &mut *(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State);
                s.view.scale = 0.25;
                s.view.dimensions(s.minimum)
            };
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                anchor.x,
                anchor.y,
                w,
                h,
                SWP_SHOWWINDOW,
            )?;
        }
    }
    Ok(())
}
/// Explicit diagnostics: independent 4K fixtures, native hidden surfaces, CPU/handle sampling.
#[cfg(feature = "diagnostics")]
pub fn stress(path: &std::path::Path) -> Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(path).map_err(|e| fail(&e.to_string()))?;
    let mut csv = std::fs::File::create(path.join("pins.csv")).map_err(|e| fail(&e.to_string()))?;
    writeln!(
        csv,
        "iteration,unused_capture_ms,private_bytes,total_working_set,handles,gdi,user"
    )
    .map_err(|e| fail(&e.to_string()))?;
    // SAFETY: the diagnostics command owns this UI thread and all hidden test windows.
    unsafe {
        register(GetModuleHandleW(None)?.into())?;
    }
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            close_all();
            super::pinned_render::shutdown();
        }
    }
    let _cleanup = Cleanup;
    super::diagnostics::row(&mut csv, 0, 0.)?;
    let mut gpu = std::fs::File::create(path.join("gpu.csv")).map_err(|e| fail(&e.to_string()))?;
    writeln!(gpu, "iteration,local_bytes,nonlocal_bytes").map_err(|e| fail(&e.to_string()))?;
    let gpu_row = |csv: &mut std::fs::File, index| -> Result<()> {
        match super::pinned_device::memory() {
            Some((local, nonlocal)) => writeln!(csv, "{index},{local},{nonlocal}"),
            None => writeln!(csv, "{index},NA,NA"),
        }
        .map_err(|e| fail(&e.to_string()))
    };
    gpu_row(&mut gpu, 0)?;
    for cycle in 0..100 {
        for index in 0..4 {
            let mut image = Image::new(crate::core::geometry::PixelRect {
                left: 0,
                top: 0,
                right: 3840,
                bottom: 2160,
            })
            .map_err(|e| fail(&e))?;
            for (position, pixel) in image.pixels_mut().chunks_exact_mut(4).enumerate() {
                let x = position % 3840;
                let y = position / 3840;
                pixel.copy_from_slice(&[(x / 16) as u8, (y / 12) as u8, 96 + index * 20, 255]);
            }
            open_inner(
                Arc::new(image),
                PixelPoint {
                    x: 64 + 32 * i32::from(index),
                    y: 64,
                },
                false,
            )?;
        }
        WINDOWS.with_borrow(|windows| -> Result<()> {
            for entry in windows {
                if let Some(hwnd) = entry.life.upgrade().and_then(|life| life.get()) {
                    // SAFETY: live State belongs to this thread; D2D drawing cannot invoke the application message loop.
                    unsafe {
                        let s = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State);
                        if let Some(canvas) = s.canvas.as_ref() {
                            canvas.paint(s.view, s.minimum, s.dpi, false, None, false)?;
                        }
                    }
                }
            }
            Ok(())
        })?;
        super::diagnostics::row(&mut csv, cycle * 2 + 1, 0.)?;
        gpu_row(&mut gpu, cycle * 2 + 1)?;
        close_all();
        // Pump release/paint messages for this diagnostics process, without UI input or mouse polling.
        // SAFETY: the current thread owns every message and window in this isolated probe process.
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        super::diagnostics::row(&mut csv, cycle * 2 + 2, 0.)?;
        gpu_row(&mut gpu, cycle * 2 + 2)?;
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    // SAFETY: final idle phase pumps the one-shot device cleanup that also runs in the normal application message loop.
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    super::diagnostics::row(&mut csv, 201, 0.)?;
    gpu_row(&mut gpu, 201)?;
    csv.flush().map_err(|e| fail(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_windows_release_image_and_lifetimes_without_quitting() {
        super::super::tests::graphics_runtime();
        let _apartment = super::super::composition::Apartment::enter().unwrap();
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                close_all();
                super::super::pinned_render::shutdown();
            }
        }
        let _cleanup = Cleanup;
        // SAFETY: the test thread creates, inspects and destroys its own hidden native windows.
        unsafe {
            register(GetModuleHandleW(None).unwrap().into()).unwrap();
            let image = Arc::new(
                Image::new(crate::core::geometry::PixelRect {
                    left: 0,
                    top: 0,
                    right: 8,
                    bottom: 6,
                })
                .unwrap(),
            );
            open_inner(image.clone(), PixelPoint { x: 0, y: 0 }, false).unwrap();
            open_inner(image.clone(), PixelPoint { x: 200, y: 200 }, false).unwrap();
            let lives: Vec<_> = WINDOWS.with_borrow(|windows| {
                windows
                    .iter()
                    .map(|entry| entry.life.upgrade().unwrap())
                    .collect()
            });
            assert_eq!(Arc::strong_count(&image), 3);
            let hwnd = lives[0].get().unwrap();
            assert_ne!(
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize,
                0
            );
            let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
            (*state).canvas = None;
            (*state).paint(hwnd).unwrap();
            DestroyWindow(hwnd).unwrap();
            assert!(lives[0].get().is_none());
            assert!(lives[1].get().is_some());
            assert_eq!(Arc::strong_count(&image), 2);
            close_all();
            assert!(lives[1].get().is_none());
            assert_eq!(Arc::strong_count(&image), 1);
            let mut message = MSG::default();
            assert!(!PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE).as_bool());
            open_inner(image.clone(), PixelPoint { x: 0, y: 0 }, false).unwrap();
            open_inner(image.clone(), PixelPoint { x: 200, y: 200 }, false).unwrap();
            drop(super::super::AppStateGuard);
            assert_eq!(Arc::strong_count(&image), 1);
            assert!(WINDOWS.with_borrow(|windows| windows.is_empty()));
        }
    }
}
