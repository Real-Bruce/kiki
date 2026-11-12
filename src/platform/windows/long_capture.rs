use crate::core::{
    display::DisplayInfo,
    geometry::PixelRect,
    image::Image,
    long_image::{self, Step, Stitcher, View},
    selection::{PixelPoint, contains},
};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
    core::{Error, Result},
};

pub const SCROLLED: u32 = WM_APP + 73;
pub const TIMER: usize = 8;
static WORKERS: AtomicUsize = AtomicUsize::new(0);
const FRAME_UNSTABLE: windows::core::HRESULT = windows::core::HRESULT(0x8000000a_u32 as i32);
const PAUSED: &str = "未找到可靠接缝，已保留已拼接部分。请缓慢滚回已捕获内容，或点击完成。";

fn fail(message: &str) -> Error {
    Error::new(E_FAIL, message)
}

#[derive(Clone, Copy)]
struct Target {
    window: usize,
    pid: u32,
    bounds: RECT,
}

impl Target {
    fn read(window: HWND) -> Option<Self> {
        // SAFETY: query-only calls validate the HWND; it may disappear between calls.
        unsafe {
            if window.is_invalid()
                || !IsWindowVisible(window).as_bool()
                || IsIconic(window).as_bool()
            {
                return None;
            }
            let mut bounds = RECT::default();
            GetWindowRect(window, &mut bounds).ok()?;
            let mut pid = 0;
            GetWindowThreadProcessId(window, Some(&mut pid));
            (pid != 0).then_some(Self {
                window: window.0 as usize,
                pid,
                bounds,
            })
        }
    }

    fn valid(self) -> bool {
        Self::read(HWND(self.window as *mut _))
            .is_some_and(|target| target.pid == self.pid && target.bounds == self.bounds)
    }
}

#[derive(Clone, Copy)]
struct Route {
    hwnd: HWND,
    target: Target,
    area: PixelRect,
    panel: PixelRect,
    ready: bool,
}

thread_local! { static ROUTE: Cell<Option<Route>> = const { Cell::new(None) }; }

unsafe extern "system" fn wheel_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: Windows supplies MSLLHOOKSTRUCT for a nonnegative mouse hook code.
    // The hook only posts messages, and all routing state belongs to its UI thread.
    unsafe {
        if code >= 0
            && wp.0 == WM_MOUSEWHEEL as usize
            && let Some(route) = ROUTE.get()
        {
            let mouse = &*(lp.0 as *const MSLLHOOKSTRUCT);
            let point = PixelPoint {
                x: mouse.pt.x,
                y: mouse.pt.y,
            };
            if contains(route.area, point) && !contains(route.panel, point) {
                if route.ready {
                    let child = WindowFromPoint(mouse.pt);
                    let root = GetAncestor(child, GA_ROOT);
                    let key_flags = |key, flag| {
                        if windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key) < 0 {
                            flag
                        } else {
                            0
                        }
                    };
                    let keys = key_flags(0x10, 4) | key_flags(0x11, 8);
                    let delta = (mouse.mouseData >> 16) as u16 as i16;
                    let coordinates = u32::from(point.x as u16) | (u32::from(point.y as u16) << 16);
                    let forwarded = root.0 as usize == route.target.window
                        && route.target.valid()
                        && PostMessageW(
                            Some(child),
                            WM_MOUSEWHEEL,
                            WPARAM(((mouse.mouseData & 0xffff0000) | keys) as usize),
                            LPARAM(coordinates as isize),
                        )
                        .is_ok();
                    let direction = if !forwarded {
                        0
                    } else if delta < 0 {
                        1
                    } else {
                        2
                    };
                    let _ = PostMessageW(Some(route.hwnd), SCROLLED, WPARAM(direction), LPARAM(0));
                }
                return LRESULT(1);
            }
        }
        CallNextHookEx(None, code, wp, lp)
    }
}

struct Hook(HHOOK);
impl Drop for Hook {
    fn drop(&mut self) {
        ROUTE.set(None);
        // SAFETY: this is the hook installed on the current UI thread.
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
    }
}

fn target_at(area: PixelRect) -> Option<Target> {
    let center = POINT {
        x: area.left + (area.right - area.left) / 2,
        y: area.top + (area.bottom - area.top) / 2,
    };
    // SAFETY: the preview has been hidden before hit testing the source.
    unsafe { Target::read(GetAncestor(WindowFromPoint(center), GA_ROOT)) }
}

fn verify_exclusion(owner: usize) -> Result<()> {
    // SAFETY: query returns an error if the session HWND has been destroyed.
    unsafe {
        let mut value = 0;
        GetWindowDisplayAffinity(HWND(owner as *mut _), &mut value)?;
        if value != WDA_EXCLUDEFROMCAPTURE.0 {
            return Err(fail("无法从长截图中排除工具界面"));
        }
    }
    Ok(())
}

pub struct Panel {
    pub bounds: PixelRect,
    pub track: PixelRect,
    pub finish: PixelRect,
    pub cancel: PixelRect,
}

impl Panel {
    fn new(screen: &DisplayInfo, selection: PixelRect, edit: bool) -> Self {
        let s = screen.bounds;
        let (w, h) = s.size().unwrap_or((1, 1));
        let dpi = screen.scale_percent as f32 / 100.;
        let px = |dip: f32| (dip * dpi).round() as i32;
        let margin = px(12.).min((w as i32 / 20).min(h as i32 / 20));
        let width = px(210.).min(w as i32 / 3).max(1);
        let height = if edit {
            h as i32 - px(114.)
        } else {
            px(520.).min(h as i32 - 2 * margin)
        }
        .max(1);
        let right = s.right - margin;
        let left = if !edit && selection.right + margin + width <= right {
            selection.right + margin
        } else {
            right - width
        };
        let top = if edit {
            s.top + margin
        } else {
            selection.top.clamp(
                s.top + margin,
                (s.bottom - margin - height).max(s.top + margin),
            )
        };
        let bounds = PixelRect {
            left,
            right: left + width,
            top,
            bottom: top + height,
        };
        let inset = margin.min(width / 8);
        let bottom = bounds.bottom - inset;
        let middle = left + width / 2;
        let button = px(32.).min(height / 5);
        Self {
            bounds,
            track: PixelRect {
                left: left + inset,
                right: bounds.right - inset,
                top: top + px(42.).min(height / 5),
                bottom: bottom - if edit { button } else { 4 * button },
            },
            finish: PixelRect {
                left: left + inset,
                right: middle - 2,
                top: bottom - button,
                bottom,
            },
            cancel: PixelRect {
                left: middle + 2,
                right: bounds.right - inset,
                top: bottom - button,
                bottom,
            },
        }
    }
}

pub struct Document {
    pub view: View,
    pub panel: Panel,
    pub thumbnail: Image,
    pub dragging: bool,
}

impl Document {
    pub fn new(screen: &DisplayInfo, image: &Image, thumbnail: Image) -> Option<Self> {
        let panel = Panel::new(screen, screen.bounds, true);
        let margin = (12. * screen.scale_percent as f32 / 100.).round() as i32;
        let canvas = PixelRect {
            left: screen.bounds.left + margin,
            top: panel.track.top,
            right: panel.bounds.left - margin,
            bottom: panel.bounds.bottom,
        };
        Some(Self {
            view: View::new(canvas, image.bounds())?,
            panel,
            thumbnail,
            dragging: false,
        })
    }

    pub fn locate(&mut self, point: PixelPoint) {
        let track = self.scrollbar();
        let bounds = if point.x >= track.left {
            track
        } else {
            self.preview_bounds()
        };
        let relative = (point.y - bounds.top) as f32 / (bounds.bottom - bounds.top).max(1) as f32;
        let height = self.view.document.bottom - self.view.document.top;
        let center = (relative * height as f32).round() as i32;
        self.view.offset = (center - self.view.visible_rows() / 2).clamp(0, self.view.max_offset());
    }

    pub fn scroll(&mut self, delta: i16) {
        let step = (self.view.visible_rows() / 6).max(1);
        self.view.offset =
            (self.view.offset - i32::from(delta) * step / 120).clamp(0, self.view.max_offset());
    }

    pub fn preview_bounds(&self) -> PixelRect {
        let track = self.panel.track;
        thumbnail_bounds(
            PixelRect {
                right: track.right - 14,
                ..track
            },
            &self.thumbnail,
        )
    }

    pub fn scrollbar(&self) -> PixelRect {
        let track = self.panel.track;
        PixelRect {
            left: track.right - 10,
            ..track
        }
    }
}

pub fn thumbnail_bounds(area: PixelRect, image: &Image) -> PixelRect {
    let (width, height) = image.bounds().size().unwrap_or((1, 1));
    let (aw, ah) = area.size().unwrap_or((1, 1));
    let scale = (aw as f32 / width as f32).min(ah as f32 / height as f32);
    let w = (width as f32 * scale).round().max(1.) as i32;
    let h = (height as f32 * scale).round().max(1.) as i32;
    let left = area.left + (aw as i32 - w) / 2;
    PixelRect {
        left,
        right: left + w,
        top: area.top,
        bottom: area.top + h,
    }
}

pub struct Update {
    pub thumbnail: Image,
    pub width: u32,
    pub height: u32,
    pub viewport: (usize, usize),
    pub message: &'static str,
    full: bool,
}

pub enum Reply {
    Update(Update),
    Paused(Error),
    Stopped(Error),
    Finished(Image, Image),
}

enum Command {
    Sample(bool),
    Finish,
}

struct Permit;
impl Permit {
    fn acquire() -> Result<Self> {
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .map_err(|_| fail("上一轮长截图仍在释放资源，请稍后重试"))?;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Worker {
    cancel: Arc<AtomicBool>,
    commands: SyncSender<Command>,
    replies: Receiver<Reply>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub struct Capture {
    pub panel: Panel,
    pub screen: DisplayInfo,
    pub area: PixelRect,
    pub browse: Option<f32>,
    target: Target,
    pub update: Option<Update>,
    pub message: String,
    running: bool,
    finishing: bool,
    pub requested: bool,
    pending: Option<bool>,
    deadline: Instant,
    worker: Worker,
    hook: Option<Hook>,
}

impl Capture {
    pub fn start(hwnd: HWND, area: PixelRect, screen: DisplayInfo) -> Result<Self> {
        // SAFETY: all calls operate on the preview's live top-level HWND. Region ownership
        // transfers to Windows only on success. Do not borrow preview state across these calls.
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let result = (|| {
                let target = target_at(area).ok_or_else(|| fail("请框选可滚动的正文窗口"))?;
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)?;
                verify_exclusion(hwnd.0 as usize)?;
                let panel = Panel::new(&screen, area, false);
                let mut window = RECT::default();
                GetWindowRect(hwnd, &mut window)?;
                let r = panel.bounds;
                let region = CreateRectRgn(
                    r.left - window.left,
                    r.top - window.top,
                    r.right - window.left,
                    r.bottom - window.top,
                );
                if region.is_invalid() {
                    return Err(fail("无法创建长截图预览窗口区域"));
                }
                if let Err(error) = add_capture_border(region, window, area) {
                    let _ = DeleteObject(region.into());
                    return Err(error);
                }
                if SetWindowRgn(hwnd, Some(region), false) == 0 {
                    let _ = DeleteObject(region.into());
                    return Err(fail("无法隔离长截图预览窗口"));
                }
                let module = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
                let hook = Hook(SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(wheel_hook),
                    Some(module.into()),
                    0,
                )?);
                ROUTE.set(Some(Route {
                    hwnd,
                    target,
                    area,
                    panel: panel.bounds,
                    ready: false,
                }));
                let worker = Worker::start(hwnd.0 as usize, area, screen.clone(), target)?;
                Ok(Self {
                    panel,
                    screen,
                    area,
                    browse: None,
                    target,
                    update: None,
                    message: "正在获取首帧…".into(),
                    running: true,
                    finishing: false,
                    requested: false,
                    pending: None,
                    deadline: Instant::now(),
                    worker,
                    hook: Some(hook),
                })
            })();
            if result.is_err() {
                let _ = SetWindowRgn(hwnd, None, false);
                let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = focus_preview(hwnd);
            result
        }
    }

    pub fn scrolled(&mut self, direction: usize) {
        if direction == 0 {
            self.stop("目标窗口已移动、被遮挡或无法接收滚轮。");
        } else if self.hook.is_some() && !self.finishing {
            self.pending = Some(direction == 1);
            self.deadline = Instant::now() + Duration::from_millis(160);
        }
    }

    pub fn stop(&mut self, reason: &str) {
        self.pending = None;
        self.hook = None;
        self.requested = false;
        self.finishing = false;
        self.message = reason.to_owned();
    }

    pub fn request_finish(&mut self) {
        self.requested = true;
        if let Some(mut route) = ROUTE.get() {
            route.ready = false;
            ROUTE.set(Some(route));
        }
    }

    pub fn tick(&mut self) -> Option<Reply> {
        if self.hook.is_some() && !self.target.valid() {
            self.stop("目标窗口已移动或关闭。请结束长截图。");
            return Some(Reply::Stopped(fail("目标窗口已移动或关闭。请结束长截图。")));
        }
        match self.worker.replies.try_recv() {
            Ok(reply) => {
                self.running = false;
                return Some(reply);
            }
            Err(TryRecvError::Disconnected) => {
                if self.running {
                    self.running = false;
                    return Some(Reply::Stopped(fail("长截图工作线程已退出")));
                }
            }
            Err(TryRecvError::Empty) => {}
        }
        if !self.running {
            let command = if self.pending.is_some() {
                if Instant::now() < self.deadline {
                    return None;
                }
                self.pending.take().map(Command::Sample)
            } else if self.requested && !self.finishing {
                self.finishing = true;
                self.hook = None;
                Some(Command::Finish)
            } else {
                None
            };
            if let Some(command) = command {
                if self.worker.commands.try_send(command).is_ok() {
                    self.running = true;
                } else {
                    return Some(Reply::Stopped(fail("无法继续长截图")));
                }
            }
        }
        None
    }

    pub fn update(&mut self, update: Update) {
        self.browse = None;
        if self.hook.is_some() {
            ROUTE.with(|route| {
                if let Some(mut current) = route.get() {
                    current.ready = !self.requested;
                    route.set(Some(current));
                }
            });
            self.message = update.message.into();
            if update.full {
                self.hook = None;
                self.pending = None;
            }
        }
        self.update = Some(update);
    }

    pub fn reset_window(hwnd: HWND) {
        // SAFETY: restore the live preview's full input/paint area after stopping the hook.
        unsafe {
            let _ = SetWindowRgn(hwnd, None, true);
            let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
        }
    }
}

fn add_capture_border(region: HRGN, window: RECT, area: PixelRect) -> Result<()> {
    // SAFETY: the caller owns the destination region; each temporary GDI region is deleted here.
    unsafe {
        for (l, t, r, b) in [
            (area.left - 2, area.top - 2, area.right + 2, area.top),
            (area.left - 2, area.bottom, area.right + 2, area.bottom + 2),
            (area.left - 2, area.top, area.left, area.bottom),
            (area.right, area.top, area.right + 2, area.bottom),
        ] {
            let strip = CreateRectRgn(
                l - window.left,
                t - window.top,
                r - window.left,
                b - window.top,
            );
            if strip.is_invalid() {
                return Err(fail("无法创建长截图边框"));
            }
            let result = CombineRgn(Some(region), Some(region), Some(strip), RGN_OR);
            let _ = DeleteObject(strip.into());
            if result == RGN_ERROR {
                return Err(fail("无法隔离长截图边框"));
            }
        }
    }
    Ok(())
}

fn focus_preview(hwnd: HWND) -> HWND {
    // SAFETY: this is the live HWND on its creating UI thread.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd)).unwrap_or_default()
    }
}

impl Worker {
    fn start(owner: usize, area: PixelRect, screen: DisplayInfo, target: Target) -> Result<Self> {
        let permit = Permit::acquire()?;
        let (commands, rx) = mpsc::sync_channel(1);
        let (tx, replies) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        std::thread::Builder::new()
            .name("kiki-long-capture".into())
            .spawn(move || {
                let _permit = permit;
                let result = (|| {
                    let initial = stable_frame(owner, area, &screen, target, &token)?;
                    let mut stitch = Stitcher::new(initial).map_err(|e| fail(&e))?;
                    let mut terminal = false;
                    let update = make_update(&stitch, "在正文区域慢慢滚动，避开固定页头和底栏。")?;
                    if tx.send(Reply::Update(update)).is_err() {
                        return Ok(());
                    }
                    while !token.load(Ordering::Acquire) {
                        match rx.recv() {
                            Ok(Command::Sample(down)) if !terminal => {
                                let sampled = stable_frame(owner, area, &screen, target, &token);
                                let response = match sampled {
                                    Ok(frame) => match stitch.accept(&frame, down) {
                                        Ok(step) => {
                                            let message = match step {
                                                Step::Added => {
                                                    "已追加新内容。可继续滚动或点击完成。"
                                                }
                                                Step::Known => {
                                                    "当前位置已捕获。可继续滚动或点击完成。"
                                                }
                                                Step::Unmatched => PAUSED,
                                                Step::Limit => {
                                                    terminal = true;
                                                    "已达到长图上限。点击完成保留当前长图。"
                                                }
                                            };
                                            match make_update(&stitch, message) {
                                                Ok(mut update) => {
                                                    update.full = terminal;
                                                    Reply::Update(update)
                                                }
                                                Err(error) => {
                                                    terminal = true;
                                                    Reply::Stopped(error)
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            terminal = true;
                                            Reply::Stopped(fail(&error))
                                        }
                                    },
                                    Err(error) if error.code() == FRAME_UNSTABLE => {
                                        Reply::Paused(error)
                                    }
                                    Err(error) => {
                                        terminal = true;
                                        Reply::Stopped(error)
                                    }
                                };
                                if tx.send(response).is_err() {
                                    return Ok(());
                                }
                            }
                            Ok(Command::Sample(_)) => {
                                if tx
                                    .send(Reply::Stopped(fail("已停止追加，请点击完成")))
                                    .is_err()
                                {
                                    return Ok(());
                                }
                            }
                            Ok(Command::Finish) => {
                                let thumbnail =
                                    long_image::thumbnail(stitch.image()).map_err(|e| fail(&e))?;
                                let _ = tx.send(Reply::Finished(stitch.into_image(), thumbnail));
                                return Ok(());
                            }
                            Err(_) => return Ok(()),
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let _ = tx.send(Reply::Stopped(error));
                }
            })
            .map_err(|e| fail(&format!("无法启动长截图：{e}")))?;
        Ok(Self {
            cancel,
            commands,
            replies,
        })
    }
}

fn make_update(stitch: &Stitcher, message: &'static str) -> Result<Update> {
    let image = stitch.image();
    let (width, height) = image.bounds().size().ok_or_else(|| fail("无效长图尺寸"))?;
    let thumbnail = long_image::thumbnail(image).map_err(|e| fail(&e))?;
    Ok(Update {
        thumbnail,
        width,
        height,
        viewport: stitch.viewport(),
        message,
        full: false,
    })
}

fn stable_frame(
    owner: usize,
    area: PixelRect,
    screen: &DisplayInfo,
    target: Target,
    cancel: &AtomicBool,
) -> Result<Image> {
    let mut previous: Option<Image> = None;
    for _ in 0..4 {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::new(E_ABORT, "长截图已取消"));
        }
        verify_exclusion(owner)?;
        if !target.valid() {
            return Err(fail("目标窗口已移动或关闭"));
        }
        let frame = super::capture::long_frame(cancel, area, screen)?;
        verify_exclusion(owner)?;
        if !target.valid() {
            return Err(fail("目标窗口已移动或关闭"));
        }
        if previous
            .as_ref()
            .is_some_and(|last| last.pixels() == frame.pixels())
        {
            return Ok(frame);
        }
        previous = Some(frame);
        std::thread::sleep(Duration::from_millis(45));
    }
    Err(Error::new(
        FRAME_UNSTABLE,
        "画面尚未稳定，已保留当前内容。请减慢滚动或点击完成。",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{display::ColorMode, long_image::fixture};

    fn screen() -> DisplayInfo {
        DisplayInfo {
            bounds: PixelRect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1080,
            },
            name: "fixture".into(),
            scale_percent: 150,
            rotation_degrees: 0,
            refresh_hz: 60,
            color: ColorMode::Sdr,
        }
    }

    #[test]
    fn right_panel_fallback_keeps_source_area_and_navigates_to_end() {
        let screen = screen();
        let mut selection = screen.bounds;
        selection.right = -700;
        let beside = Panel::new(&screen, selection, false);
        assert!(beside.bounds.left > selection.right);
        let fallback = Panel::new(&screen, screen.bounds, false);
        assert!(fallback.bounds.intersection(screen.bounds) == Some(fallback.bounds));

        let image = fixture(1700, 0, 2000);
        let thumbnail = long_image::thumbnail(&image).unwrap();
        let mut document = Document::new(&screen, &image, thumbnail).unwrap();
        assert!(document.view.scale < 1.);
        let bar = document.scrollbar();
        document.locate(PixelPoint {
            x: bar.left,
            y: bar.bottom - 1,
        });
        assert_eq!(document.view.offset, document.view.max_offset());
        assert_eq!(document.view.band().bottom, image.bounds().bottom);
        document.scroll(120);
        assert!(document.view.offset < document.view.max_offset());
    }
}
