//! Real Win32 controls, native keyboard navigation and a Direct2D settings surface.
use super::{
    hotkey,
    preferences::{self, SaveStatus},
    selection_render::system_dark,
    settings_render::{self as appearance, Canvas},
    startup,
};
use crate::core::config::{Config, Hotkey, Theme};
#[path = "settings_layout.rs"]
mod settings_layout;
use settings_layout::{FOOTER, HEADER, Item, Page, Rect, Tone};
use std::{cell::Cell, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
    core::{BOOL, PCWSTR, Result, w},
};
pub(super) const RESTORE_HOTKEY: u32 = WM_APP + 62;
const PAGES: [&str; 6] = ["通用", "快捷键", "截图", "贴图", "外观", "关于"];
const BRAND: u16 = 90;
const COLORS: [[u8; 3]; 8] = super::selection_render::ANNOTATION_COLORS;
thread_local! {static WINDOW:Cell<Option<HWND>>=const{Cell::new(None)};}
struct Control {
    hwnd: HWND,
    id: u16,
}
struct State {
    life: Rc<Cell<Option<HWND>>>,
    page: usize,
    scroll: i32,
    page_scroll: [i32; 6],
    hover: Option<HWND>,
    keyboard_focus: bool,
    app_icon: HICON,
    dpi: u32,
    controls: Vec<Control>,
    canvas: Option<Canvas>,
    font: HFONT,
    icon_font: HFONT,
    brush: HBRUSH,
    suppress: bool,
    error: Option<String>,
    startup: std::result::Result<startup::State, String>,
}
impl Drop for State {
    fn drop(&mut self) {
        // SAFETY: the state owns both fonts and the brush on this window thread.
        unsafe {
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.icon_font.into());
            let _ = DeleteObject(self.brush.into());
        }
    }
}
pub fn open() -> Result<()> {
    open_impl(true)
}
fn open_impl(visible: bool) -> Result<()> {
    // SAFETY: this window and its controls live exclusively on the application's STA.
    unsafe {
        if let Some(hwnd) = WINDOW.get() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
            return Ok(());
        }
        let module = GetModuleHandleW(None)?;
        let app_icon = super::icon::application(module.into(), false)?;
        let small_icon = super::icon::application(module.into(), true)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            hIcon: app_icon,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("kiki.Settings"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_win32());
        }
        let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_WIN95_CLASSES,
        });
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let mut dpi = GetDpiForSystem().max(96);
        let mut dpi_y = dpi;
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let dpi = dpi.max(96);
        let s = dpi as f32 / 96.;
        let width = (960. * s) as i32;
        let height = (820. * s) as i32;
        let work = info.rcWork;
        let width = width.min(work.right - work.left);
        let height = height.min(work.bottom - work.top);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            w!("kiki 设置"),
            WS_OVERLAPPEDWINDOW | WS_VSCROLL | WS_CLIPCHILDREN,
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        let dpi = GetDpiForWindow(hwnd).max(96);
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(small_icon.0 as isize)),
        );
        let font = font(dpi, 14, w!("Microsoft YaHei UI"));
        let state = Box::new(State {
            life: Rc::new(Cell::new(Some(hwnd))),
            page: 0,
            scroll: 0,
            page_scroll: [0; 6],
            hover: None,
            keyboard_focus: false,
            app_icon,
            dpi,
            controls: Vec::new(),
            canvas: None,
            font,
            icon_font: font_for_icons(dpi),
            brush: CreateSolidBrush(rgb(appearance::background(system_dark()))),
            suppress: true,
            error: None,
            startup: startup::state().map_err(|e| e.to_string()),
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        WINDOW.set(Some(hwnd));
        if let Err(error) = create_controls(hwnd) {
            let _ = DestroyWindow(hwnd);
            return Err(error);
        }
        refresh();
        if visible {
            let _ = ShowWindow(hwnd, SW_SHOW);
            // Explicitly reveal a user-requested settings window even when STARTUPINFO requested SW_HIDE.
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
            )?;
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }
}
pub fn close() {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
pub fn route(message: &MSG) -> bool {
    // SAFETY: called by the UI thread with a live message and its settings window.
    WINDOW.get().is_some_and(|hwnd| unsafe {
        if message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_TAB.0)
            && GetAncestor(GetFocus(), GA_ROOT) == hwnd
            && GetKeyState(i32::from(VK_CONTROL.0)) >= 0
        {
            focus_next(hwnd, GetKeyState(i32::from(VK_SHIFT.0)) < 0);
            true
        } else {
            IsDialogMessageW(hwnd, message).as_bool()
        }
    })
}

fn focus_next(hwnd: HWND, backwards: bool) {
    // SAFETY: all handles and geometry are copied before layout and SetFocus send window messages.
    unsafe {
        let p = pointer(hwnd);
        if p.is_null() {
            return;
        }
        let state = &*p;
        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() {
            return;
        }
        let dip = |value| (value as f32 * 96. / state.dpi as f32).round() as i32;
        let page = page_geometry(state, dip(client.right));
        let target = |id| state.controls.iter().find(|c| c.id == id).map(|c| c.hwnd);
        let mut stops: Vec<_> = (100..=105)
            .filter_map(|id| target(id).map(|h| (h, None)))
            .collect();
        stops.extend(page.items.iter().filter_map(|item| match item {
            Item::Control(id, bounds) => target(*id).map(|h| (h, Some(*bounds))),
            _ => None,
        }));
        if matches!(preferences::status(), SaveStatus::Failed(_))
            && let Some(retry) = target(800)
        {
            stops.push((retry, None));
        }
        let current = stops.iter().position(|(h, _)| *h == GetFocus());
        let index = match (current, backwards) {
            (Some(i), true) => (i + stops.len() - 1) % stops.len(),
            (Some(i), false) => (i + 1) % stops.len(),
            (None, true) => stops.len().saturating_sub(1),
            (None, false) => 0,
        };
        if let Some(&(next, bounds)) = stops.get(index) {
            if let Some(bounds) = bounds {
                let top = state.scroll + HEADER;
                let bottom = state.scroll + dip(client.bottom) - FOOTER;
                (*p).scroll = if bounds.y < top {
                    bounds.y - HEADER
                } else if bounds.y + bounds.h > bottom {
                    bounds.y + bounds.h - dip(client.bottom) + FOOTER
                } else {
                    state.scroll
                };
                layout(hwnd);
            }
            (*p).keyboard_focus = true;
            SendMessageW(
                hwnd,
                WM_CHANGEUISTATE,
                Some(WPARAM(
                    UIS_CLEAR as usize | ((UISF_HIDEFOCUS as usize) << 16),
                )),
                None,
            );
            let _ = SetFocus(Some(next));
        }
    }
}

pub fn hotkey_focused() -> bool {
    WINDOW.get().is_some_and(|hwnd| unsafe {
        let p = pointer(hwnd);
        !p.is_null()
            && (*p)
                .controls
                .iter()
                .any(|c| c.id == 300 && GetFocus() == c.hwnd)
    })
}
pub fn restore_hotkey_input() {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let p = pointer(hwnd);
            if p.is_null() {
                return;
            }
            let child = (*p).controls.iter().find(|c| c.id == 300).map(|c| c.hwnd);
            if let Some(child) = child {
                (*p).suppress = true;
                hotkey::set_control(child, preferences::current().hotkey);
                (*p).suppress = false;
                (*p).error = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
    }
}
pub fn refresh() {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let p = pointer(hwnd);
            if p.is_null() {
                return;
            }
            (*p).suppress = true;
            let cfg = preferences::current();
            let controls: Vec<_> = (*p).controls.iter().map(|c| (c.hwnd, c.id)).collect();
            for (child, id) in controls {
                let caption = caption(id, &cfg, &(*p).startup);
                if !caption.is_empty() {
                    let wide: Vec<_> = caption.encode_utf16().chain(Some(0)).collect();
                    let _ = SetWindowTextW(child, PCWSTR(wide.as_ptr()));
                }
                if id == 300 && GetFocus() != child {
                    hotkey::set_control(child, cfg.hotkey);
                }
            }
            let dark = BOOL::from(system_dark());
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as *const _,
                std::mem::size_of::<BOOL>() as u32,
            );
            let caption = rgb(appearance::sidebar(dark.as_bool()));
            let text = rgb(appearance::foreground(dark.as_bool()));
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &caption as *const _ as *const _,
                4,
            );
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, &text as *const _ as *const _, 4);
            let brush = CreateSolidBrush(rgb(appearance::background(dark.as_bool())));
            let old = std::mem::replace(&mut (*p).brush, brush);
            let _ = DeleteObject(old.into());
            (*p).suppress = false;
            layout(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
}
unsafe fn pointer(hwnd: HWND) -> *mut State {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State }
}
fn font_for_icons(dpi: u32) -> HFONT {
    font(dpi, 17, w!("Segoe Fluent Icons"))
}
fn font(dpi: u32, size: u32, face: PCWSTR) -> HFONT {
    // SAFETY: the font name is NUL-terminated and the font is owned by the caller.
    unsafe {
        CreateFontW(
            -((size * dpi / 96) as i32),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            face,
        )
    }
}
fn caption(id: u16, cfg: &Config, start: &std::result::Result<startup::State, String>) -> String {
    match id {
        100..=105 => PAGES[(id - 100) as usize].into(),
        200 => match start {
            Ok(startup::State::Enabled) => "开机启动：开启",
            Ok(startup::State::Moved) => "关闭旧的开机启动项",
            _ => "开机启动：关闭",
        }
        .into(),
        201 => "更新为当前程序路径".into(),
        301 => "恢复默认".into(),
        400 => format!("操作提示：{}", if cfg.show_hints { "开启" } else { "关闭" }),
        410..=417 => {
            let i = (id - 410) as usize;
            format!(
                "{}{}",
                ["红", "橙", "黄", "绿", "蓝", "紫", "白", "黑"][i],
                if cfg.color == COLORS[i] { " ✓" } else { "" }
            )
        }
        420..=423 => {
            let n = [1, 3, 5, 8][(id - 420) as usize];
            format!("{n} px{}", if cfg.stroke_width == n { " ✓" } else { "" })
        }
        430..=433 => {
            let n = [16, 24, 32, 48][(id - 430) as usize];
            format!("{n} px{}", if cfg.text_size == n { " ✓" } else { "" })
        }
        500 => "关闭所有贴图".into(),
        600..=602 => {
            let i = (id - 600) as usize;
            format!(
                "{}{}",
                ["跟随系统", "浅色", "深色"][i],
                if cfg.theme == [Theme::System, Theme::Light, Theme::Dark][i] {
                    " ✓"
                } else {
                    ""
                }
            )
        }
        700 => "打开配置目录".into(),
        701 => "使用帮助".into(),
        702 => "第三方许可".into(),
        800 => "重试保存".into(),
        _ => String::new(),
    }
}
fn create_controls(hwnd: HWND) -> Result<()> {
    // SAFETY: native controls and synchronous messages are owned by this settings window thread.
    unsafe {
        let p = pointer(hwnd);
        let ids = [BRAND]
            .into_iter()
            .chain(100..=105)
            .chain([200, 201, 300, 301, 400, 500, 700, 701, 702, 800])
            .chain(410..=417)
            .chain(420..=423)
            .chain(430..=433)
            .chain(600..=602);
        for id in ids {
            let (class, style) = match id {
                // SS_OWNERDRAW (WinUser.h) is not exported by the current bindings.
                BRAND => (w!("STATIC"), WINDOW_STYLE(0x000d)),
                300 => (HOTKEY_CLASSW, WS_TABSTOP),
                _ => (w!("BUTTON"), WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32)),
            };
            let child = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!(""),
                WS_CHILD | style,
                0,
                0,
                1,
                1,
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            SendMessageW(child, WM_SETFONT, Some(WPARAM((*p).font.0 as usize)), None);
            if id == 300 {
                hotkey::theme_control(child)?;
            } else if id != BRAND && !SetWindowSubclass(child, Some(button_proc), 1, 0).as_bool() {
                return Err(windows::core::Error::from_win32());
            }
            (*p).controls.push(Control { hwnd: child, id });
        }
        Ok(())
    }
}

fn page_geometry(state: &State, width: i32) -> Page {
    Page::new(
        width,
        state.page,
        &preferences::current(),
        &state.startup,
        preferences::active(),
    )
}

fn layout(hwnd: HWND) {
    // SAFETY: snapshot child handles and bounds before SetWindowPos sends messages to them.
    unsafe {
        let p = pointer(hwnd);
        if p.is_null() {
            return;
        }
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let scale = (*p).dpi as f32 / 96.;
        let dip = |v| (v as f32 / scale).round() as i32;
        let px = |v| (v as f32 * scale).round() as i32;
        let height = dip(client.bottom);
        let width = dip(client.right);
        let geometry = page_geometry(&*p, width);
        let viewport = (height - HEADER - FOOTER).max(1);
        let max_scroll = (geometry.end - HEADER - viewport).max(0);
        (*p).scroll = (*p).scroll.clamp(0, max_scroll);
        let scroll = (*p).scroll;
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: geometry.end - HEADER - 1,
            nPage: viewport as u32,
            nPos: scroll,
            ..Default::default()
        };
        SetScrollInfo(hwnd, SB_VERT, &info, true);
        let controls: Vec<_> = (*p).controls.iter().map(|c| (c.hwnd, c.id)).collect();
        let failed = matches!(preferences::status(), SaveStatus::Failed(_));
        for (child, id) in controls {
            let bounds = match id {
                BRAND => Some(Rect {
                    x: 20,
                    y: 20,
                    w: 32,
                    h: 32,
                }),
                100..=105 => Some(Rect {
                    x: 10,
                    y: 76 + i32::from(id - 100) * 46,
                    w: geometry.sidebar - 20,
                    h: 40,
                }),
                800 if failed => Some(Rect {
                    x: geometry.left + geometry.width - 108,
                    y: height - 43,
                    w: 108,
                    h: 32,
                }),
                _ => geometry.items.iter().find_map(|item| match item {
                    Item::Control(control_id, r) if *control_id == id => {
                        let y = r.y - scroll;
                        (y >= HEADER && y + r.h <= height - FOOTER).then_some(Rect { y, ..*r })
                    }
                    _ => None,
                }),
            };
            if let Some(Rect { x, y, w, h }) = bounds {
                let _ = SetWindowPos(
                    child,
                    None,
                    px(x),
                    px(y),
                    px(x + w) - px(x),
                    px(y + h) - px(y),
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                let _ = InvalidateRect(Some(child), None, false);
            } else {
                if (*p).hover == Some(child) {
                    (*p).hover = None;
                }
                let _ = ShowWindow(child, SW_HIDE);
            }
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn paint(hwnd: HWND, p: *mut State) -> Result<()> {
    // SAFETY: the HWND and state are live for this UI-thread paint dispatch.
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        let state = &mut *p;
        if state.canvas.is_none() {
            state.canvas = Some(Canvas::new(
                hwnd,
                rect.right as u32,
                rect.bottom as u32,
                state.dpi,
            )?);
        }
        let Some(c) = state.canvas.as_ref() else {
            return Ok(());
        };
        let scale = state.dpi as f32 / 96.;
        let width = (rect.right as f32 / scale).round();
        let height = (rect.bottom as f32 / scale).round();
        let page = page_geometry(state, width as i32);
        let x = page.left as f32;
        let w = page.width as f32;
        let offset = state.scroll as f32;
        let dark = system_dark();
        let fg = appearance::foreground(dark);
        let sub = appearance::muted(dark);
        let bg = appearance::background(dark);
        let warning = appearance::warning(dark);
        c.begin(dark);
        c.fill(
            0.,
            0.,
            page.sidebar as f32,
            height,
            appearance::sidebar(dark),
        );
        c.heading("kiki", 63., 22., 75., 21., fg);
        for item in &page.items {
            match item {
                Item::Heading(label, y) => c.heading(label, x, *y as f32 - offset, w, 16., fg),
                Item::Text(label, r, tone) => {
                    let (size, color) = match tone {
                        Tone::Normal => (14., fg),
                        Tone::Muted => (12., sub),
                        Tone::Warning => (12., warning),
                    };
                    c.text(
                        label,
                        r.x as f32,
                        r.y as f32 - offset,
                        r.w as f32,
                        size,
                        color,
                    );
                }
                Item::Value(label, r) => {
                    c.value(label, r.x as f32, r.y as f32 - offset, r.w as f32, sub)
                }
                Item::Divider(y) => c.fill(x, *y as f32 - offset, w, 1., appearance::divider(dark)),
                Item::Control(..) => {}
            }
        }
        let sidebar = page.sidebar as f32;
        c.fill(sidebar, 0., width - sidebar, HEADER as f32, bg);
        c.heading(PAGES[state.page], x, 19., w, 24., fg);
        c.text(
            [
                "管理启动方式与截图后的行为",
                "设置全局截图快捷键。",
                "仅影响新截图，不更改已有标注。",
                "贴图保留原图，缩放只影响显示。",
                "选择适合当前桌面的主题。",
                "轻量截图与置顶贴图。",
            ][state.page],
            x,
            58.,
            w,
            13.,
            sub,
        );
        c.fill(
            sidebar,
            height - FOOTER as f32,
            width - sidebar,
            FOOTER as f32,
            bg,
        );
        let status = preferences::status();
        let failed = matches!(status, SaveStatus::Failed(_));
        let saved = state.error.is_none() && matches!(status, SaveStatus::Saved);
        let message = state.error.clone().unwrap_or_else(|| match status {
            SaveStatus::Ready => "更改后自动保存".into(),
            SaveStatus::Saving => "正在保存设置…".into(),
            SaveStatus::Saved => "设置已自动保存".into(),
            SaveStatus::Failed(e) => format!("保存失败：{e}"),
        });
        let status_color = if saved {
            if dark { 0x8bdca3 } else { 0x217139 }
        } else if state.error.is_some() || failed {
            warning
        } else {
            sub
        };
        if saved {
            c.glyph("\u{e73e}", x, height - 35., status_color);
        }
        let indent = if saved { 23. } else { 0. };
        let reserved = if failed { 120. } else { 0. };
        c.text(
            &message,
            x + indent,
            height - 36.,
            (w - indent - reserved).max(1.),
            12.,
            status_color,
        );
        c.end()
    }
}

fn action(hwnd: HWND, id: u16) {
    unsafe {
        let p = pointer(hwnd);
        if p.is_null() || (*p).suppress {
            return;
        }
        if (100..=105).contains(&id) {
            (*p).page_scroll[(*p).page] = (*p).scroll;
            (*p).page = usize::from(id - 100);
            (*p).scroll = (*p).page_scroll[(*p).page];
            (*p).error = None;
            refresh();
            return;
        }
        let life = (*p).life.clone();
        (*p).error = None;
        let mut cfg = preferences::current();
        let result: std::result::Result<(), String> = match id {
            200 | 201 => {
                let enabled = id == 201
                    || !matches!(
                        (*p).startup,
                        Ok(startup::State::Enabled | startup::State::Moved)
                    );
                let result = startup::set(enabled).map_err(|e| e.to_string());
                (*p).startup = startup::state().map_err(|e| e.to_string());
                if result.is_ok() {
                    preferences::external_saved();
                }
                result
            }
            300 => {
                let child = (*p).controls.iter().find(|c| c.id == 300).unwrap().hwnd;
                hotkey::control_value(child)
                    .map_err(|_| "请按完整组合：Ctrl 或 Alt 加字母、数字或功能键".to_string())
                    .and_then(|key| {
                        cfg.hotkey = key;
                        preferences::update(cfg)
                    })
            }
            301 => {
                cfg.hotkey = Hotkey::default();
                preferences::update(cfg)
            }
            400 => {
                cfg.show_hints = !cfg.show_hints;
                preferences::update(cfg)
            }
            410..=417 => {
                cfg.color = COLORS[(id - 410) as usize];
                preferences::update(cfg)
            }
            420..=423 => {
                cfg.stroke_width = [1, 3, 5, 8][(id - 420) as usize];
                preferences::update(cfg)
            }
            430..=433 => {
                cfg.text_size = [16, 24, 32, 48][(id - 430) as usize];
                preferences::update(cfg)
            }
            500 => {
                super::pinned::close_all();
                Ok(())
            }
            600..=602 => {
                cfg.theme = [Theme::System, Theme::Light, Theme::Dark][(id - 600) as usize];
                preferences::update(cfg)
            }
            700 => open_folder(hwnd),
            701 => super::text_viewer::open(hwnd, "kiki 使用帮助", &help(&cfg))
                .map_err(|e| e.to_string()),
            702 => super::text_viewer::open(
                hwnd,
                "第三方许可",
                include_str!("../../../resources/THIRD_PARTY_LICENSES.txt"),
            )
            .map_err(|e| e.to_string()),
            800 => {
                preferences::retry();
                Ok(())
            }
            _ => Ok(()),
        };
        if life.get() == Some(hwnd) {
            (*p).error = result.err();
            refresh();
        }
    }
}
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: the subclass belongs to a live settings child. The parent owns the state and
    // removes its pointer before freeing it. This callback does not retain a state reference.
    unsafe {
        if matches!(msg, WM_MOUSEMOVE | WM_MOUSELEAVE | WM_LBUTTONDOWN)
            && let Ok(parent) = GetParent(hwnd)
        {
            let p = pointer(parent);
            if !p.is_null() {
                if msg == WM_LBUTTONDOWN {
                    (*p).keyboard_focus = false;
                    let focused = GetFocus();
                    if GetAncestor(focused, GA_ROOT) == parent {
                        let _ = InvalidateRect(Some(focused), None, false);
                    }
                } else if msg == WM_MOUSEMOVE && (*p).hover != Some(hwnd) {
                    let previous = (*p).hover.replace(hwnd);
                    if let Some(previous) = previous {
                        let _ = InvalidateRect(Some(previous), None, false);
                    }
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut track);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                } else if msg == WM_MOUSELEAVE && (*p).hover == Some(hwnd) {
                    (*p).hover = None;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(button_proc), 1);
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

fn draw_button(item: &DRAWITEMSTRUCT, p: *mut State) {
    // SAFETY: WM_DRAWITEM supplies a borrowed DC and bounds for the live UI-thread control.
    unsafe {
        let state = &*p;
        let dark = system_dark();
        let id = item.CtlID as u16;
        let dip = |v: i32| (v as f32 * state.dpi as f32 / 96.).round() as i32;
        let r = item.rcItem;
        let dc = item.hDC;
        let nav = (100..=105).contains(&id);
        let backdrop = if nav || id == BRAND {
            appearance::sidebar(dark)
        } else {
            appearance::background(dark)
        };
        fill_rect(dc, &r, backdrop);
        if id == BRAND {
            let _ = DrawIconEx(
                dc,
                r.left,
                r.top,
                state.app_icon,
                r.right - r.left,
                r.bottom - r.top,
                0,
                None,
                DI_NORMAL,
            );
            return;
        }

        let cfg = preferences::current();
        let hovered = state.hover == Some(item.hwndItem);
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let focused = state.keyboard_focus
            && item.itemState.0 & (ODS_FOCUS.0 | ODS_NOFOCUSRECT.0) == ODS_FOCUS.0;
        let accent = appearance::accent(dark);
        let fg = appearance::foreground(dark);
        if matches!(id, 200 | 400) {
            let on = if id == 400 {
                cfg.show_hints
            } else {
                matches!(
                    state.startup,
                    Ok(startup::State::Enabled | startup::State::Moved)
                )
            };
            if hovered && !disabled {
                rounded_fill(dc, r, dip(12), if dark { 0x37393e } else { 0xeff2f6 });
            }
            draw_toggle(item, dip, dark, on);
        } else if (410..=417).contains(&id) {
            let color = COLORS[usize::from(id - 410)];
            let selected = cfg.color == color;
            let [red, green, blue] = color.map(u32::from);
            let color = red << 16 | green << 8 | blue;
            let size = dip(22).min(r.right - r.left - dip(3)).max(1);
            let left = (r.left + r.right - size) / 2;
            let top = (r.top + r.bottom - size) / 2;
            let swatch = RECT {
                left,
                top,
                right: left + size,
                bottom: top + size,
            };
            if hovered || pressed || color == 0xffffff && !dark || color == 0x000000 && dark {
                rounded_fill(
                    dc,
                    swatch,
                    size,
                    if hovered || pressed {
                        accent
                    } else {
                        appearance::muted(dark)
                    },
                );
                let inner = RECT {
                    left: left + 1,
                    top: top + 1,
                    right: left + size - 1,
                    bottom: top + size - 1,
                };
                rounded_fill(dc, inner, size - 2, color);
            } else {
                rounded_fill(dc, swatch, size, color);
            }
            if selected {
                let mark = if red * 3 + green * 6 + blue > 1400 {
                    0x202226
                } else {
                    0xffffff
                };
                draw_label(dc, state.icon_font, "\u{e73e}", r, mark, DT_CENTER);
            }
        } else {
            let selected = match id {
                100..=105 => usize::from(id - 100) == state.page,
                420..=423 => cfg.stroke_width == [1, 3, 5, 8][usize::from(id - 420)],
                430..=433 => cfg.text_size == [16, 24, 32, 48][usize::from(id - 430)],
                600..=602 => {
                    cfg.theme == [Theme::System, Theme::Light, Theme::Dark][usize::from(id - 600)]
                }
                _ => false,
            };
            let theme = (600..=602).contains(&id);
            let fill = match (pressed, selected && !theme, hovered && !disabled) {
                (true, _, _) => Some(if dark { 0x3c5773 } else { 0xc7e0f7 }),
                (_, true, _) => Some(if dark { 0x334d68 } else { 0xdcecfb }),
                (_, _, true) => Some(if dark { 0x37393e } else { 0xeff2f6 }),
                _ if matches!(id, 201 | 301 | 500 | 700..=702 | 800) => {
                    Some(if dark { 0x303236 } else { 0xf0f2f5 })
                }
                _ => None,
            };
            if let Some(fill) = fill {
                rounded_fill(dc, r, dip(12), fill);
            }
            let text_color = if disabled {
                appearance::muted(dark)
            } else if selected {
                accent
            } else {
                fg
            };
            let mut label_bounds = r;
            let label = match id {
                100..=105 => {
                    const ICONS: [&str; 6] = [
                        "\u{e713}", "\u{e765}", "\u{e739}", "\u{e718}", "\u{e790}", "\u{e946}",
                    ];
                    let icon = RECT {
                        left: r.left + dip(13),
                        ..r
                    };
                    draw_label(
                        dc,
                        state.icon_font,
                        ICONS[usize::from(id - 100)],
                        icon,
                        text_color,
                        DT_LEFT,
                    );
                    label_bounds.left += dip(42);
                    if selected {
                        let mark = RECT {
                            left: r.left + dip(1),
                            top: r.top + dip(6),
                            right: r.left + dip(4),
                            bottom: r.bottom - dip(6),
                        };
                        rounded_fill(dc, mark, dip(3), accent);
                    }
                    PAGES[usize::from(id - 100)].to_owned()
                }
                420..=423 => {
                    let n = [1, 3, 5, 8][usize::from(id - 420)];
                    let h = dip(n).max(1);
                    let y = (r.top + r.bottom - h) / 2;
                    let x = r.left + dip(7);
                    if r.right - r.left >= dip(56) {
                        rounded_fill(
                            dc,
                            RECT {
                                left: x,
                                top: y,
                                right: x + dip(16),
                                bottom: y + h,
                            },
                            h,
                            text_color,
                        );
                        label_bounds.left += dip(22);
                    }
                    format!("{n} px")
                }
                430..=433 => [16, 24, 32, 48][usize::from(id - 430)].to_string(),
                600..=602 => {
                    draw_preview(dc, r, dip, id);
                    label_bounds.top = r.bottom - dip(36);
                    let name = ["跟随系统", "浅色", "深色"][usize::from(id - 600)];
                    if selected {
                        format!("✓ {name}")
                    } else {
                        name.to_owned()
                    }
                }
                _ => caption(id, &cfg, &state.startup),
            };
            draw_label(
                dc,
                state.font,
                &label,
                label_bounds,
                text_color,
                if nav { DT_LEFT } else { DT_CENTER },
            );
        }
        if focused && !disabled {
            let mut frame = r;
            let _ = InflateRect(&mut frame, -dip(2), -dip(2));
            let brush = CreateSolidBrush(rgb(accent));
            FrameRect(dc, &frame, brush);
            let _ = DeleteObject(brush.into());
        }
    }
}

fn fill_rect(dc: HDC, r: &RECT, color: u32) {
    // SAFETY: callers provide a live paint DC. FillRect does not retain the temporary brush.
    unsafe {
        let brush = CreateSolidBrush(rgb(color));
        FillRect(dc, r, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn rounded_fill(dc: HDC, r: RECT, corner: i32, color: u32) {
    // SAFETY: both temporary GDI objects are local to this paint call and are not selected into the DC.
    unsafe {
        let region = CreateRoundRectRgn(
            r.left,
            r.top,
            r.right + 1,
            r.bottom + 1,
            corner.max(1),
            corner.max(1),
        );
        let brush = CreateSolidBrush(rgb(color));
        let _ = FillRgn(dc, region, brush);
        let _ = DeleteObject(brush.into());
        let _ = DeleteObject(region.into());
    }
}

fn draw_label(dc: HDC, font: HFONT, label: &str, mut r: RECT, color: u32, align: DRAW_TEXT_FORMAT) {
    // SAFETY: the UTF-16 buffer and DC live through DrawTextW; restore the caller's font.
    unsafe {
        let mut text: Vec<_> = label.encode_utf16().collect();
        let old = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, rgb(color));
        DrawTextW(
            dc,
            &mut text,
            &mut r,
            align | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        SelectObject(dc, old);
    }
}

fn draw_preview(dc: HDC, r: RECT, dip: impl Fn(i32) -> i32, id: u16) {
    let thumb = RECT {
        left: r.left + dip(5),
        top: r.top + dip(5),
        right: r.right - dip(5),
        bottom: r.bottom - dip(44),
    };
    let w = thumb.right - thumb.left;
    let h = thumb.bottom - thumb.top;
    let split = thumb.left + w / 2;
    let miniature = |dark| {
        rounded_fill(dc, thumb, dip(12), if dark { 0x36383d } else { 0xe7edf4 });
        let x = thumb.left + w / 3;
        let y = thumb.top + h / 6;
        let panel = RECT {
            left: x,
            top: y,
            right: thumb.right - w / 14,
            bottom: thumb.bottom - h / 8,
        };
        rounded_fill(dc, panel, dip(6), if dark { 0x26272a } else { 0xffffff });
        for (index, length) in [(0, 43), (1, 34), (2, 39)] {
            let top = y + (index + 1) * h / 6;
            rounded_fill(
                dc,
                RECT {
                    left: x + w / 16,
                    top,
                    right: x + w * length / 100,
                    bottom: top + dip(4),
                },
                dip(4),
                if dark { 0x54575e } else { 0xd8e0ea },
            );
        }
        let left = thumb.left + w / 13;
        for index in 0..3 {
            let top = y + (index + 1) * h / 6;
            rounded_fill(
                dc,
                RECT {
                    left,
                    top,
                    right: left + w / 7,
                    bottom: top + dip(4),
                },
                dip(4),
                match (dark, index) {
                    (true, 0) => 0x578fc7,
                    (false, 0) => 0x86b9ee,
                    (true, _) => 0x686b72,
                    _ => 0xb7c4d4,
                },
            );
        }
    };
    miniature(id == 602);
    if id == 600 {
        // SAFETY: the DC is live for this paint. Restore the clip before returning.
        unsafe {
            let saved = SaveDC(dc);
            if saved != 0 {
                IntersectClipRect(dc, split, thumb.top, thumb.right + 1, thumb.bottom + 1);
                miniature(true);
                let _ = RestoreDC(dc, saved);
            }
        }
    }
}

fn draw_toggle(item: &DRAWITEMSTRUCT, dip: impl Fn(i32) -> i32, dark: bool, on: bool) {
    // SAFETY: the DC is borrowed from WM_DRAWITEM. Restore the original pen and brush before releasing temporary objects.
    unsafe {
        let r = item.rcItem;
        let dc = item.hDC;
        let (width, height) = (dip(42), dip(22));
        let left = r.left + (r.right - r.left - width) / 2;
        let top = r.top + (r.bottom - r.top - height) / 2;
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let track = CreateSolidBrush(rgb(match (on, dark, pressed) {
            (true, _, true) => 0x0054b8,
            (true, _, false) => 0x0875e6,
            (false, true, _) => 0x667487,
            (false, false, true) => 0xa9b4c3,
            (false, false, false) => 0xc7ced8,
        }));
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        let old_brush = SelectObject(dc, track.into());
        let _ = RoundRect(dc, left, top, left + width, top + height, height, height);
        let knob = CreateSolidBrush(rgb(0xffffff));
        SelectObject(dc, knob.into());
        let padding = dip(2);
        let size = height - 2 * padding;
        let x = if on {
            left + width - padding - size
        } else {
            left + padding
        };
        let _ = Ellipse(dc, x, top + padding, x + size, top + padding + size);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        let _ = DeleteObject(knob.into());
        let _ = DeleteObject(track.into());
    }
}
fn rgb(value: u32) -> COLORREF {
    COLORREF(((value & 255) << 16) | (value & 0xff00) | ((value >> 16) & 255))
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    std::panic::catch_unwind(|| dispatch(hwnd, msg, wp, lp)).unwrap_or(LRESULT(0))
}
fn dispatch(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let p = pointer(hwnd);
        if p.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        match msg {
            RESTORE_HOTKEY => {
                restore_hotkey_input();
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wp.0 & 65535) as u16;
                let code = (wp.0 >> 16) as u32;
                if code == BN_CLICKED || id == 300 && code == EN_CHANGE {
                    action(hwnd, id);
                }
                LRESULT(0)
            }
            WM_DRAWITEM if lp.0 != 0 => {
                draw_button(&*(lp.0 as *const DRAWITEMSTRUCT), p);
                LRESULT(1)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                let dc = HDC(wp.0 as *mut _);
                SetTextColor(dc, rgb(appearance::foreground(system_dark())));
                SetBkColor(dc, rgb(appearance::background(system_dark())));
                LRESULT((*p).brush.0 as isize)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                if paint(hwnd, p).is_err() {
                    (*p).canvas = None;
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                let mut client = RECT::default();
                if GetClientRect(hwnd, &mut client).is_ok() {
                    if let Some(canvas) = (*p).canvas.as_ref() {
                        if canvas
                            .resize(
                                client.right.max(1) as u32,
                                client.bottom.max(1) as u32,
                                (*p).dpi,
                            )
                            .is_err()
                        {
                            (*p).canvas = None;
                        }
                    }
                }
                layout(hwnd);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                (*p).scroll -= i32::from((wp.0 >> 16) as u16 as i16) / 120 * 48;
                layout(hwnd);
                LRESULT(0)
            }
            WM_VSCROLL => {
                let code = (wp.0 & 65535) as i32;
                (*p).scroll += match code {
                    0 => -32,
                    1 => 32,
                    2 => -160,
                    3 => 160,
                    _ => 0,
                };
                if code == 4 || code == 5 {
                    let mut info = SCROLLINFO {
                        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                        fMask: SIF_TRACKPOS,
                        ..Default::default()
                    };
                    let _ = GetScrollInfo(hwnd, SB_VERT, &mut info);
                    (*p).scroll = info.nTrackPos;
                }
                layout(hwnd);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                (*p).dpi = (wp.0 & 65535) as u32;
                let next = font((*p).dpi, 14, w!("Microsoft YaHei UI"));
                let old_icons = std::mem::replace(&mut (*p).icon_font, font_for_icons((*p).dpi));
                let _ = DeleteObject(old_icons.into());
                let old = std::mem::replace(&mut (*p).font, next);
                let controls: Vec<_> = (*p).controls.iter().map(|c| c.hwnd).collect();
                for child in controls {
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(next.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
                let _ = DeleteObject(old.into());
                let rect = *(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                refresh();
                LRESULT(0)
            }
            WM_THEMECHANGED | WM_SETTINGCHANGE => {
                refresh();
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let m = &mut *(lp.0 as *mut MINMAXINFO);
                let s = (*p).dpi as f32 / 96.;
                m.ptMinTrackSize = POINT {
                    x: (660. * s) as i32,
                    y: (480. * s) as i32,
                };
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let state = Box::from_raw(p);
                state.life.set(None);
                WINDOW.set(None);
                drop(state);
                super::pinned_device::idle();
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn open_folder(hwnd: HWND) -> std::result::Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let path = preferences::path()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .ok_or("配置路径不可用")?;
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: explicit user action opens the known configuration folder, never a shell command.
    let result = unsafe {
        ShellExecuteW(
            Some(hwnd),
            w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        Err("无法打开配置目录".into())
    } else {
        Ok(())
    }
}
fn help(cfg: &Config) -> String {
    format!(
        "开始截图：{}，或点击系统托盘图标。\r\n\r\n拖动选择区域；拖动边框调整。\r\n选择矩形、箭头、画笔或文字进行标注。\r\nCtrl+Z 撤销；Ctrl+Y 重做；Esc 取消。\r\n复制、保存或贴图成功后结束本次截图。\r\n\r\n贴图：拖动移动，滚轮缩放，Ctrl+0 原始比例。\r\nCtrl+C 复制，Ctrl+S 保存，Esc 关闭。\r\n\r\n右键托盘可打开设置、关闭所有贴图或退出。",
        cfg.hotkey.label()
    )
}
#[cfg(test)]
mod tests {
    use super::super::{composition, config_store, preferences};
    use super::*;
    use std::time::{Duration, Instant};
    fn wait_saved() {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            preferences::poll();
            if !matches!(preferences::status(), SaveStatus::Saving) {
                return;
            }
            assert!(Instant::now() < deadline, "writer timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn native_settings_apply_rollback_scroll_retry_and_teardown() {
        super::super::tests::graphics_runtime();
        let _com = composition::Apartment::enter().unwrap();
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                close();
                super::super::pinned_render::shutdown();
            }
        }
        let _cleanup = Cleanup;
        let folder = std::env::temp_dir().join(format!("kiki-p6-native-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("settings.ini");
        let mut cfg = Config::default();
        cfg.hotkey.key = crate::core::config::Key::Function(18);
        cfg.first_tip_seen = true;
        std::fs::write(&path, cfg.encode().unwrap()).unwrap();
        // SAFETY: hidden owner, native child controls, messages and registrations are all test-owned on this thread.
        unsafe {
            let owner = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki preferences test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(GetModuleHandleW(None).unwrap().into()),
                None,
            )
            .unwrap();
            let startup_blocker = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki startup conflict"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(GetModuleHandleW(None).unwrap().into()),
                None,
            )
            .unwrap();
            let mut initial_registration = super::super::hotkey::Registration::new(startup_blocker);
            initial_registration.replace(cfg.hotkey).unwrap();
            let (guard, warning) = preferences::initialize_at(owner, path.clone()).unwrap();
            assert!(warning.is_some());
            assert!(!preferences::active());
            drop(initial_registration);
            DestroyWindow(startup_blocker).unwrap();
            preferences::update(cfg.clone()).unwrap();
            assert!(preferences::active());
            open_impl(false).unwrap();
            let hwnd = WINDOW.get().unwrap();
            let life = (*pointer(hwnd)).life.clone();
            for page in 0..6 {
                SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(100 + page)), None);
                assert_eq!((*pointer(hwnd)).page, page);
                assert!(paint(hwnd, pointer(hwnd)).is_ok());
            }
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(602)), None);
            assert!(system_dark());
            assert_eq!(preferences::current().theme, Theme::Dark);
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(400)), None);
            assert!(!preferences::hints());
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(414)), None);
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(423)), None);
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(433)), None);
            assert_eq!(
                (
                    preferences::current().color,
                    preferences::current().stroke_width,
                    preferences::current().text_size
                ),
                (COLORS[4], 8, 48)
            );
            let blocked = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki conflict test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(GetModuleHandleW(None).unwrap().into()),
                None,
            )
            .unwrap();
            let mut registration = super::super::hotkey::Registration::new(blocked);
            let mut candidate = cfg.hotkey;
            candidate.key = crate::core::config::Key::Function(17);
            registration.replace(candidate).unwrap();
            let before = preferences::current();
            let mut attempted = before.clone();
            attempted.hotkey = candidate;
            assert!(preferences::update(attempted).is_err());
            assert_eq!(preferences::current(), before);
            assert!(preferences::active());
            drop(registration);
            DestroyWindow(blocked).unwrap();
            let child = (*pointer(hwnd))
                .controls
                .iter()
                .find(|c| c.id == 300)
                .unwrap()
                .hwnd;
            hotkey::set_control(child, candidate);
            SendMessageW(
                hwnd,
                WM_COMMAND,
                Some(WPARAM(300 | ((EN_CHANGE as usize) << 16))),
                Some(LPARAM(child.0 as isize)),
            );
            assert_eq!(preferences::current().hotkey, candidate);
            // Short logical work areas must scroll; controls outside the viewport must be hidden.
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(102)), None);
            SetWindowPos(hwnd, None, 0, 0, 860, 480, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
            SendMessageW(hwnd, WM_VSCROLL, Some(WPARAM(SB_PAGEDOWN.0 as usize)), None);
            assert!((*pointer(hwnd)).scroll > 0);
            assert!(paint(hwnd, pointer(hwnd)).is_ok());
            wait_saved();
            assert!(matches!(preferences::status(), SaveStatus::Saved));
            // Real failure and retry: a directory at the destination cannot be atomically replaced by a file.
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir(&path).unwrap();
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(601)), None);
            wait_saved();
            assert!(matches!(preferences::status(), SaveStatus::Failed(_)));
            std::fs::remove_dir(&path).unwrap();
            preferences::retry();
            wait_saved();
            assert!(matches!(preferences::status(), SaveStatus::Saved));
            let expected = preferences::current();
            close();
            assert!(life.get().is_none());
            assert!(WINDOW.get().is_none());
            drop(guard);
            assert_eq!(config_store::Disk::load(path.clone()).config, expected);
            DestroyWindow(owner).unwrap();
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
