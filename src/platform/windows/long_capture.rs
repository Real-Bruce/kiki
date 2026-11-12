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
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
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
const PAUSED: &str = "未找到可靠接缝，已保留已拼接部分。请缓慢滚回已捕获内容，或点击完成。";
const WHEEL_RETRY: &str = "目标窗口暂时无法接收滚轮，请点击正文后重试。";
const SCROLL_TAIL: Duration = Duration::from_millis(320);
const SCROLL_MAX_TAIL: Duration = Duration::from_millis(700);
const FRAME_QUIET: Duration = Duration::from_millis(100);
const FRAME_WAIT: Duration = Duration::from_millis(12);
const PREVIEW_INTERVAL: Duration = Duration::from_millis(50);

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
                    // Floating windows and shell surfaces can temporarily cover the
                    // selection. Only the source's movement or closure stops capture.
                    if root.0 as usize != route.target.window {
                        return LRESULT(1);
                    }
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
                    let native = GetAncestor(GetForegroundWindow(), GA_ROOT) == root;
                    let forwarded = native
                        || PostMessageW(
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
                    if native {
                        return CallNextHookEx(None, code, wp, lp);
                    }
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

fn target_at(owner: HWND, area: PixelRect) -> Option<Target> {
    let center = POINT {
        x: area.left + (area.right - area.left) / 2,
        y: area.top + (area.bottom - area.top) / 2,
    };
    // SAFETY: the preview's region is open at the selection center before hit testing.
    unsafe {
        let window = GetAncestor(WindowFromPoint(center), GA_ROOT);
        if window == owner || window == GetDesktopWindow() || window == GetShellWindow() {
            return None;
        }
        Target::read(window)
    }
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
        let mut left = if !edit && selection.right + margin + width <= right {
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
        let mut bounds = PixelRect {
            left,
            right: left + width,
            top,
            bottom: top + height,
        };
        if !edit
            && bounds.intersection(selection) == Some(selection)
            && selection.left - margin - width >= s.left + margin
        {
            left = selection.left - margin - width;
            bounds.left = left;
            bounds.right = left + width;
        }
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
    Stopped(Error),
    Finished(Image, Image),
}

#[derive(Clone, Copy)]
struct Scroll {
    down: bool,
    at: Instant,
}

struct ScrollBurst {
    scroll: Scroll,
    last: Option<Step>,
    changed: bool,
    previewed: bool,
    last_progress: Instant,
}

impl ScrollBurst {
    fn new(scroll: Scroll) -> Self {
        Self {
            scroll,
            last: None,
            changed: false,
            previewed: false,
            last_progress: scroll.at,
        }
    }

    fn observe(&mut self, stitch: &mut Stitcher, frame: &Image) -> Result<Step> {
        let previous = stitch.viewport();
        let result = stitch
            .accept(frame, self.scroll.down)
            .map_err(|e| fail(&e))?;
        self.last = Some(result);
        if previous != stitch.viewport() {
            self.changed = true;
            self.last_progress = Instant::now();
        }
        Ok(result)
    }

    fn settled(&self, now: Instant) -> bool {
        let since_wheel = now.duration_since(self.scroll.at);
        since_wheel >= SCROLL_MAX_TAIL
            || since_wheel >= SCROLL_TAIL && now.duration_since(self.last_progress) >= FRAME_QUIET
    }
}

#[derive(Default)]
struct Control {
    scroll: Option<Scroll>,
    finish: bool,
    stop: bool,
}

#[derive(Default)]
struct Shared {
    control: Mutex<Control>,
    latest: Mutex<Option<Reply>>,
}

impl Shared {
    fn publish(&self, reply: Reply) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(reply);
    }
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
    shared: Arc<Shared>,
    wake: SyncSender<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
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
    pub requested: bool,
    worker: Worker,
    hook: Option<Hook>,
}

impl Capture {
    pub fn start(hwnd: HWND, area: PixelRect, screen: DisplayInfo) -> Result<Self> {
        // SAFETY: all calls operate on the preview's live top-level HWND. Region ownership
        // transfers to Windows only on success. Do not borrow preview state across these calls.
        unsafe {
            let result = (|| {
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)?;
                verify_exclusion(hwnd.0 as usize)?;
                let panel = Panel::new(&screen, area, false);
                let mut window = RECT::default();
                GetWindowRect(hwnd, &mut window)?;
                // Leave the overlay active for Esc and open the source for hit testing
                // before adding the panel, which may cover the center of a narrow selection.
                set_capture_mask(hwnd, window, screen.bounds, area, None)?;
                let target = target_at(hwnd, area).ok_or_else(|| fail("请框选可滚动的正文窗口"))?;
                set_capture_mask(hwnd, window, screen.bounds, area, Some(panel.bounds))?;
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
                    requested: false,
                    worker,
                    hook: Some(hook),
                })
            })();
            if result.is_err() {
                Self::reset_window(hwnd);
            }
            result
        }
    }

    pub fn scrolled(&mut self, direction: usize) -> bool {
        if self.hook.is_none() || self.requested {
            return false;
        }
        if direction == 0 {
            let changed = self.message != WHEEL_RETRY;
            if changed {
                self.message = WHEEL_RETRY.into();
            }
            return changed;
        }
        self.worker.signal(|control| {
            control.scroll = Some(Scroll {
                down: direction == 1,
                at: Instant::now(),
            });
        });
        if self.message == WHEEL_RETRY
            && let Some(update) = self.update.as_ref()
        {
            self.message = update.message.into();
            return true;
        }
        false
    }

    pub fn stop(&mut self, reason: &str) {
        self.hook = None;
        self.requested = false;
        self.worker.signal(|control| control.stop = true);
        self.message = reason.to_owned();
    }

    pub fn request_finish(&mut self) {
        self.requested = true;
        if let Some(mut route) = ROUTE.get() {
            route.ready = false;
            ROUTE.set(Some(route));
        }
        self.worker.signal(|control| control.finish = true);
    }

    pub fn tick(&mut self) -> Option<Reply> {
        if self.hook.is_some() && !self.target.valid() {
            self.stop("目标窗口已移动或关闭。请结束长截图。");
            return Some(Reply::Stopped(fail("目标窗口已移动或关闭。请结束长截图。")));
        }
        self.worker
            .shared
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
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

fn set_capture_mask(
    hwnd: HWND,
    window: RECT,
    screen: PixelRect,
    area: PixelRect,
    panel: Option<PixelRect>,
) -> Result<()> {
    let region = capture_mask(window, screen, area, panel)?;
    // SAFETY: hwnd belongs to the UI thread. Windows owns region after success only.
    unsafe {
        if SetWindowRgn(hwnd, Some(region), false) == 0 {
            let _ = DeleteObject(region.into());
            return Err(fail("无法隔离长截图预览窗口"));
        }
    }
    Ok(())
}

fn capture_mask(
    window: RECT,
    screen: PixelRect,
    area: PixelRect,
    panel: Option<PixelRect>,
) -> Result<HRGN> {
    // SAFETY: each temporary GDI region is deleted here. The caller owns the returned
    // region until it transfers ownership to Windows with SetWindowRgn.
    unsafe {
        let create = |r: PixelRect| {
            CreateRectRgn(
                r.left - window.left,
                r.top - window.top,
                r.right - window.left,
                r.bottom - window.top,
            )
        };
        let region = create(screen);
        let hole = create(area);
        let valid = !region.is_invalid() && !hole.is_invalid();
        let combined = valid
            && CombineRgn(Some(region), Some(region), Some(hole), RGN_DIFF) != RGN_ERROR
            && panel.is_none_or(|panel| {
                let preview = create(panel);
                if preview.is_invalid() {
                    return false;
                }
                let added =
                    CombineRgn(Some(region), Some(region), Some(preview), RGN_OR) != RGN_ERROR;
                let _ = DeleteObject(preview.into());
                added
            });
        let _ = DeleteObject(hole.into());
        if !combined {
            let _ = DeleteObject(region.into());
            return Err(fail("无法建立长截图遮罩"));
        }
        Ok(region)
    }
}

impl Worker {
    fn start(owner: usize, area: PixelRect, screen: DisplayInfo, target: Target) -> Result<Self> {
        let permit = Permit::acquire()?;
        let (wake, input) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("kiki-long-capture".into())
            .spawn(move || {
                let _permit = permit;
                if let Err(error) =
                    run_capture(owner, area, &screen, target, &token, &worker, &input)
                    && !token.load(Ordering::Acquire)
                {
                    worker.publish(Reply::Stopped(error));
                }
            })
            .map_err(|e| fail(&format!("无法启动长截图：{e}")))?;
        Ok(Self {
            cancel,
            shared,
            wake,
        })
    }

    fn signal(&self, update: impl FnOnce(&mut Control)) {
        {
            let mut control = self
                .shared
                .control
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            update(&mut control);
        }
        let _ = self.wake.try_send(());
    }
}

fn check_target(owner: usize, target: Target) -> Result<()> {
    verify_exclusion(owner)?;
    if !target.valid() {
        return Err(fail("目标窗口已移动或关闭"));
    }
    Ok(())
}

fn run_capture(
    owner: usize,
    area: PixelRect,
    screen: &DisplayInfo,
    target: Target,
    cancel: &AtomicBool,
    shared: &Shared,
    wake: &Receiver<()>,
) -> Result<()> {
    use super::capture::RegionStream;
    let mut initial = Image::new(area).map_err(|e| fail(&e))?;
    check_target(owner, target)?;
    let first_stream = RegionStream::open(area, screen)?;
    let start = Instant::now();
    loop {
        if first_stream.read(&mut initial, FRAME_WAIT, cancel)? {
            break;
        }
        if start.elapsed() >= Duration::from_secs(3) {
            return Err(fail("等待长截图首帧超时，请重新截图"));
        }
    }
    check_target(owner, target)?;
    let mut stitch = Stitcher::new(initial).map_err(|e| fail(&e))?;
    let mut frame = Image::new(area).map_err(|e| fail(&e))?;
    let mut stream = Some(first_stream);
    shared.publish(Reply::Update(make_update(
        &stitch,
        "在正文区域滚动，避开固定页头和底栏。",
    )?));
    let mut last_preview = Instant::now();
    let mut last_display_check = last_preview;
    let mut burst: Option<ScrollBurst> = None;
    let mut was_paused = false;
    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        let (scroll, finish, stop) = {
            let mut control = shared
                .control
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            (control.scroll.take(), control.finish, control.stop)
        };
        if stop {
            stream = None;
            burst = None;
        }
        if let Some(scroll) = scroll.filter(|_| stream.is_some()) {
            match burst.as_mut() {
                Some(active) => active.scroll = scroll,
                None => burst = Some(ScrollBurst::new(scroll)),
            }
        }
        let Some(active) = burst.as_mut() else {
            if finish {
                drop(stream);
                match long_image::thumbnail(stitch.image()) {
                    Ok(thumbnail) => {
                        shared.publish(Reply::Finished(stitch.into_image(), thumbnail));
                        return Ok(());
                    }
                    Err(error) => {
                        stream = None;
                        let mut control = shared
                            .control
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        control.finish = false;
                        control.stop = true;
                        drop(control);
                        shared.publish(Reply::Stopped(fail(&error)));
                    }
                }
            }
            if wake.recv().is_err() {
                return Ok(());
            }
            continue;
        };
        let Some(source) = stream.as_ref() else {
            burst = None;
            continue;
        };
        let sampled = if last_display_check.elapsed() >= Duration::from_secs(1) {
            last_display_check = Instant::now();
            RegionStream::validate_display(area, screen)
        } else {
            Ok(())
        }
        .and_then(|()| check_target(owner, target))
        .and_then(|()| source.read(&mut frame, FRAME_WAIT, cancel))
        .and_then(|read| {
            if read {
                check_target(owner, target).map(|()| true)
            } else {
                Ok(false)
            }
        });
        let status = match sampled {
            Ok(true) => active.observe(&mut stitch, &frame).map(Some),
            Ok(false) => Ok(None),
            Err(error) => Err(error),
        };
        match status {
            Ok(Some(Step::Limit)) => {
                stream = None;
                burst = None;
                let reply = match make_update(&stitch, "已达到长图上限。点击完成保留当前长图。")
                {
                    Ok(mut update) => {
                        update.full = true;
                        Reply::Update(update)
                    }
                    Err(error) => Reply::Stopped(error),
                };
                shared.publish(reply);
                continue;
            }
            Err(error) => {
                stream = None;
                burst = None;
                shared.publish(Reply::Stopped(error));
                continue;
            }
            _ => {}
        }
        let now = Instant::now();
        if active.changed && now.duration_since(last_preview) >= PREVIEW_INTERVAL {
            if let Err(error) = publish_preview(shared, &stitch, "正在拼接长图，可继续滚动。")
            {
                stream = None;
                burst = None;
                shared.publish(Reply::Stopped(error));
                continue;
            }
            last_preview = now;
            active.changed = false;
            active.previewed = true;
            was_paused = false;
        }
        if active.settled(now) {
            if shared
                .control
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .scroll
                .is_some()
            {
                continue;
            }
            // An animation can produce temporary mismatches. Only announce a
            // gap if the last frame still cannot be aligned after scrolling.
            let gap = active.last == Some(Step::Unmatched);
            if gap || active.changed || active.previewed || was_paused && active.last.is_some() {
                let message = if gap {
                    PAUSED
                } else {
                    "当前位置已捕获。可继续滚动或点击完成。"
                };
                if let Err(error) = publish_preview(shared, &stitch, message) {
                    stream = None;
                    shared.publish(Reply::Stopped(error));
                }
                last_preview = now;
            }
            if active.last.is_some() {
                was_paused = gap;
            }
            burst = None;
        }
    }
}

fn publish_preview(shared: &Shared, stitch: &Stitcher, message: &'static str) -> Result<()> {
    shared.publish(Reply::Update(make_update(stitch, message)?));
    Ok(())
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

#[cfg(test)]
pub(super) mod tests {
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
    fn mask_keeps_surround_and_panel_but_leaves_live_content_open() {
        let screen = screen().bounds;
        let window = RECT {
            left: -2100,
            top: -400,
            right: 1920,
            bottom: 1080,
        };
        let area = PixelRect {
            left: -1700,
            top: 220,
            right: -100,
            bottom: 700,
        };
        let panel = PixelRect {
            left: -290,
            top: 140,
            right: -12,
            bottom: 780,
        };
        let mask = capture_mask(window, screen, area, Some(panel)).unwrap();
        let fullscreen = capture_mask(window, screen, screen, Some(panel)).unwrap();
        // SAFETY: the test owns the native region and deletes it before returning.
        unsafe {
            let includes = |x, y| PtInRegion(mask, x - window.left, y - window.top).as_bool();
            let in_fullscreen =
                |x, y| PtInRegion(fullscreen, x - window.left, y - window.top).as_bool();
            let results = [
                includes(-1850, 500),
                !includes(-1400, 500),
                includes(-200, 500),
                includes(-1701, 220),
                !includes(-1700, 220),
                !includes(500, 500),
                !in_fullscreen(-1850, 500),
                !in_fullscreen(-1400, 500),
                in_fullscreen(-200, 500),
            ];
            let _ = DeleteObject(mask.into());
            let _ = DeleteObject(fullscreen.into());
            assert!(results.into_iter().all(|valid| valid));
        }
    }

    #[test]
    fn continuous_scroll_can_append_frames_even_without_a_stable_pair() {
        let start = Instant::now();
        let mut burst = ScrollBurst::new(Scroll {
            down: true,
            at: start,
        });
        let mut stitch = Stitcher::new(fixture(240, 0, 220)).unwrap();
        let jump = fixture(240, 360, 220);
        assert_eq!(stitch.accept(&jump, true).unwrap(), Step::Unmatched);
        let bad = fixture(240, 10_000, 220);
        assert_eq!(burst.observe(&mut stitch, &bad).unwrap(), Step::Unmatched);
        for offset in [90, 180, 270, 360] {
            assert_eq!(
                burst
                    .observe(&mut stitch, &fixture(240, offset, 220))
                    .unwrap(),
                Step::Added
            );
        }
        burst.last_progress = start + Duration::from_millis(290);
        assert!(!burst.settled(start + Duration::from_millis(100)));
        assert!(!burst.settled(start + SCROLL_TAIL));
        assert!(burst.settled(start + Duration::from_millis(390)));
        assert_eq!(stitch.image().pixels(), fixture(240, 0, 580).pixels());
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
        let narrow_right = PixelRect {
            left: -115,
            right: -20,
            top: 300,
            bottom: 540,
        };
        let narrow_panel = Panel::new(&screen, narrow_right, false);
        assert!(narrow_panel.bounds.intersection(narrow_right).is_none());

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

    pub(in crate::platform::windows) struct NativeTestWindow(pub HWND);

    impl Drop for NativeTestWindow {
        fn drop(&mut self) {
            // SAFETY: this HWND was created on the current test thread.
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    pub(in crate::platform::windows) fn native_document(
        screen: &DisplayInfo,
    ) -> (NativeTestWindow, HWND, PixelRect, i32) {
        use windows::Win32::UI::Controls::{EM_GETRECT, EM_LINEINDEX, EM_POSFROMCHAR};
        use windows::core::{PCWSTR, w};
        let left = screen.bounds.left + 45;
        let top = screen.bounds.top + 85;
        let text: Vec<_> = (1..=150)
            .map(|n| {
                format!(
                    "{n:03}   F11 test {:06X}   {}\r\n",
                    n * 17011,
                    "#".repeat(3 + n % 23)
                )
            })
            .collect::<String>()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: a non-activating, read-only test EDIT receives only test-generated content.
        unsafe {
            let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let parent = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("kiki F11 numbered document fixture"),
                WS_POPUP | WS_CLIPCHILDREN,
                left,
                top,
                530,
                290,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();
            let window = NativeTestWindow(parent);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
                0,
                0,
                530,
                290,
                Some(parent),
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();
            let font = GetStockObject(ANSI_FIXED_FONT);
            SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
            SetWindowTextW(hwnd, PCWSTR(text.as_ptr())).unwrap();
            let _ = ShowWindow(parent, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(parent);
            let _ = UpdateWindow(hwnd);
            windows::Win32::Graphics::Dwm::DwmFlush().unwrap();

            let mut text_area = RECT::default();
            SendMessageW(
                hwnd,
                EM_GETRECT,
                None,
                Some(LPARAM((&raw mut text_area) as isize)),
            );
            let y_at_line = |line| {
                let start = SendMessageW(hwnd, EM_LINEINDEX, Some(WPARAM(line)), None).0;
                assert!(start >= 0);
                let position =
                    SendMessageW(hwnd, EM_POSFROMCHAR, Some(WPARAM(start as usize)), None).0;
                assert_ne!(position, -1);
                i32::from((position >> 16) as i16)
            };
            let first_y = y_at_line(0);
            let line_height = y_at_line(1) - first_y;
            assert!(line_height > 0);
            let rows = (text_area.bottom - first_y) / line_height;
            assert!(rows >= 10);
            let area = PixelRect {
                left: left + text_area.left,
                top: top + first_y,
                right: left + text_area.right,
                bottom: top + first_y + rows * line_height,
            };
            (window, hwnd, area, line_height)
        }
    }

    pub(in crate::platform::windows) fn route_test_wheel(point: PixelPoint, delta: i16) -> LRESULT {
        let event = MSLLHOOKSTRUCT {
            pt: POINT {
                x: point.x,
                y: point.y,
            },
            mouseData: u32::from(delta as u16) << 16,
            ..Default::default()
        };
        // SAFETY: the test supplies the same call-scoped structure as WH_MOUSE_LL.
        unsafe {
            wheel_hook(
                0,
                WPARAM(WM_MOUSEWHEEL as usize),
                LPARAM((&raw const event) as isize),
            )
        }
    }

    #[test]
    #[ignore = "uses a visible native scrollable document and a temporary floating window"]
    fn wheel_route_ignores_an_unrelated_window_then_forwards_to_the_document() {
        use windows::Win32::UI::Controls::EM_GETFIRSTVISIBLELINE;
        use windows::core::w;
        super::super::tests::graphics_runtime();
        let screen = super::super::display::enumerate().unwrap().remove(0).info;
        let (source, edit, area, _) = native_document(&screen);
        let center = PixelPoint {
            x: area.left + 250,
            y: area.top + 100,
        };
        // SAFETY: only the test's own native windows and thread-local route are used.
        unsafe {
            let cover = NativeTestWindow(
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("BUTTON"),
                    w!("Temporary floating window"),
                    WS_POPUP,
                    center.x - 20,
                    center.y - 20,
                    80,
                    40,
                    None,
                    None,
                    Some(
                        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                            .unwrap()
                            .into(),
                    ),
                    None,
                )
                .unwrap(),
            );
            let _ = ShowWindow(cover.0, SW_SHOWNOACTIVATE);
            ROUTE.set(Some(Route {
                hwnd: source.0,
                target: Target::read(source.0).unwrap(),
                area,
                panel: PixelRect {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                ready: true,
            }));
            let mut msg = MSG::default();
            let scrolled = |msg: &mut MSG| {
                PeekMessageW(msg, Some(source.0), SCROLLED, SCROLLED, PM_REMOVE).as_bool()
            };
            route_test_wheel(center, -120);
            assert!(!scrolled(&mut msg), "unrelated window stopped the capture");
            drop(cover);
            route_test_wheel(center, -120);
            assert!(scrolled(&mut msg));
            assert_eq!(msg.wParam.0, 1);
            assert!(
                PeekMessageW(
                    &mut msg,
                    Some(edit),
                    WM_MOUSEWHEEL,
                    WM_MOUSEWHEEL,
                    PM_REMOVE
                )
                .as_bool()
            );
            DispatchMessageW(&msg);
            assert!(SendMessageW(edit, EM_GETFIRSTVISIBLELINE, None, None).0 > 0);
            ROUTE.set(None);
        }
    }

    fn wait_worker(worker: &Worker, mut accepts: impl FnMut(Reply) -> Option<Reply>) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let reply = worker
                .shared
                .latest
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(reply) = reply.and_then(&mut accepts) {
                return reply;
            }
            assert!(
                Instant::now() < deadline,
                "native document capture reply timed out"
            );
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    #[test]
    #[ignore = "uses a visible non-activating Win32 EDIT and a real SDR capture stream"]
    fn native_document_scroll_tracks_numbered_lines_then_stops_and_restarts() {
        use windows::Win32::UI::Controls::{EM_GETFIRSTVISIBLELINE, EM_LINESCROLL};
        use windows::core::w;
        super::super::tests::graphics_runtime();
        let screen = super::super::display::enumerate()
            .unwrap()
            .into_iter()
            .map(|s| s.info)
            .find(|s| {
                s.color == ColorMode::Sdr
                    && s.bounds.size().is_some_and(|(w, h)| w >= 700 && h >= 480)
            })
            .expect("native document test needs a real SDR display");
        println!("display={screen:?}");
        let (document, edit, area, line_height) = native_document(&screen);
        let first_height = area.size().unwrap().1;
        println!("native_edit_area={area:?}, line_height_px={line_height}");
        // SAFETY: only our own hidden owner is marked excluded and destroyed by the test guard.
        let owner = unsafe {
            let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("kiki F11 test owner"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();
            let owner = NativeTestWindow(hwnd);
            SetWindowDisplayAffinity(owner.0, WDA_EXCLUDEFROMCAPTURE).unwrap();
            owner
        };
        let start = |window: &NativeTestWindow, area| {
            Worker::start(
                owner.0.0 as usize,
                area,
                screen.clone(),
                Target::read(window.0).unwrap(),
            )
            .unwrap()
        };
        let worker = start(&document, area);
        let accept_update = |reply| match reply {
            Reply::Update(update) => Some(Reply::Update(update)),
            Reply::Stopped(error) => panic!("native source stopped: {error}"),
            _ => None,
        };
        let Reply::Update(initial) = wait_worker(&worker, accept_update) else {
            unreachable!()
        };
        assert_eq!(initial.height, first_height);
        println!("initial_capture_height_px={first_height}");
        let signal = |worker: &Worker, down| {
            worker.signal(|control| {
                control.scroll = Some(Scroll {
                    down,
                    at: Instant::now(),
                });
            })
        };
        let scroll = |lines: i32| {
            // SAFETY: all messages target the test-owned read-only document HWND.
            let line = unsafe {
                let result = SendMessageW(edit, EM_LINESCROLL, None, Some(LPARAM(lines as isize)));
                assert_ne!(result.0, 0, "native EDIT rejected scrolling");
                let _ = UpdateWindow(edit);
                windows::Win32::Graphics::Dwm::DwmFlush().unwrap();
                SendMessageW(edit, EM_GETFIRSTVISIBLELINE, None, None).0
            };
            signal(&worker, lines > 0);
            println!("requested_lines={lines}, edit_first_line={line}");
            usize::try_from(line).unwrap() * line_height as usize
        };
        let mut full_height = first_height;
        for lines in [4; 17].into_iter().chain([-4]) {
            let position = scroll(lines);
            full_height = full_height.max(first_height + position as u32);
            let reply = wait_worker(&worker, |reply| match accept_update(reply) {
                Some(Reply::Update(ref update)) if update.message == PAUSED => panic!(
                    "native document seam rejected at {position}, last viewport {:?}",
                    update.viewport
                ),
                Some(Reply::Update(update)) if update.viewport.0 == position => {
                    Some(Reply::Update(update))
                }
                Some(Reply::Update(update)) => {
                    println!(
                        "intermediate_viewport={:?}, expected_top={position}",
                        update.viewport
                    );
                    None
                }
                _ => None,
            });
            let Reply::Update(update) = reply else {
                unreachable!()
            };
            assert_eq!(update.height, full_height);
            println!("lines={lines}, viewport_top_px={position}, stitched_height_px={full_height}");
        }
        drop(document);
        signal(&worker, true);
        let stopped = wait_worker(&worker, |reply| {
            matches!(reply, Reply::Stopped(_)).then_some(reply)
        });
        let Reply::Stopped(error) = stopped else {
            unreachable!()
        };
        assert!(error.message().contains("移动或关闭"));
        println!("source_closed={}", error.message());
        worker.signal(|control| control.finish = true);
        let finished = wait_worker(&worker, |reply| {
            matches!(reply, Reply::Finished(..)).then_some(reply)
        });
        let Reply::Finished(partial, _) = finished else {
            unreachable!()
        };
        assert_eq!(partial.bounds().size().unwrap().1, full_height);
        drop(worker);

        let (next, _, next_area, _) = native_document(&screen);
        let worker = start(&next, next_area);
        let Reply::Update(initial) = wait_worker(&worker, accept_update) else {
            unreachable!()
        };
        assert_eq!(initial.height, first_height);
        assert_eq!(initial.viewport.0, 0);
        println!(
            "restarted_height_px={}, viewport_top_px={}",
            initial.height, initial.viewport.0
        );
    }
}
