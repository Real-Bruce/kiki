//! Real Win32 controls, native keyboard navigation and a Direct2D settings surface.
use super::{
    hotkey,
    parameter_control::{self, Wheel},
    preferences::{self, SaveStatus},
    selection_render::system_dark,
    settings_render::{self as appearance, Canvas},
    startup,
};
use crate::core::config::{CaptureParameter, Config, Hotkey, OutputFormat, Theme};
#[path = "settings_layout.rs"]
mod settings_layout;
use settings_layout::{
    FOOTER, HEADER, Item, OUTPUT_FORMAT, PARAMETER_EDIT, PARAMETER_LABELS, PARAMETER_SLIDER, Page,
    Rect, START_CAPTURE, Tone,
};
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
const HOTKEY_RECORDED: u32 = WM_APP + 63;
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
    parameter_wheel: (Option<HWND>, Wheel),
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
        // Preserve the window name and icon for shell identification.
        let flags = WTNCA_NODRAWCAPTION | WTNCA_NODRAWICON;
        let caption = WTA_OPTIONS {
            dwFlags: flags,
            dwMask: flags,
        };
        let _ = SetWindowThemeAttribute(
            hwnd,
            WTA_NONCLIENT,
            (&caption as *const WTA_OPTIONS).cast(),
            std::mem::size_of_val(&caption) as u32,
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
            parameter_wheel: (None, Wheel::default()),
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

pub fn hide_for_capture() -> Option<HWND> {
    let hwnd = WINDOW.get()?;
    // SAFETY: the settings HWND belongs to this UI thread and remains live while hidden.
    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = DwmFlush();
            Some(hwnd)
        } else {
            None
        }
    }
}

pub fn restore_after_capture_failure(window: Option<HWND>) {
    if window.is_some() && WINDOW.get() == window {
        let _ = open();
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
            refresh_parameters();
            layout(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
}

pub fn refresh_parameters() {
    let Some(hwnd) = WINDOW.get() else { return };
    // SAFETY: only copied settings-child handles are used across window calls.
    unsafe {
        let p = pointer(hwnd);
        if p.is_null() {
            return;
        }
        let config = preferences::current();
        let controls: Vec<_> = (*p).controls.iter().map(|c| (c.hwnd, c.id)).collect();
        for (child, id) in controls {
            if let Some(parameter) = setting_parameter(id, PARAMETER_EDIT) {
                if GetFocus() != child {
                    parameter_control::set_edit(child, parameter.value(&config));
                }
            } else if setting_parameter(id, PARAMETER_SLIDER).is_some() {
                let _ = InvalidateRect(Some(child), None, false);
            }
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn setting_parameter(id: u16, first: u16) -> Option<CaptureParameter> {
    match id.checked_sub(first)? {
        index @ 0..=2 => Some(CaptureParameter::Stroke(usize::from(index))),
        3 => Some(CaptureParameter::TextSize),
        4 => Some(CaptureParameter::CornerRadius),
        _ => None,
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
        201 => "更新启动位置".into(),
        START_CAPTURE => "开始截图".into(),
        301 => "恢复默认".into(),
        400 => format!("截图提示：{}", if cfg.show_hints { "开启" } else { "关闭" }),
        410..=417 => {
            let i = (id - 410) as usize;
            format!(
                "{}{}",
                ["红", "橙", "黄", "绿", "蓝", "紫", "白", "黑"][i],
                if cfg.color == COLORS[i] { " ✓" } else { "" }
            )
        }
        420..=424 => PARAMETER_LABELS[usize::from(id - PARAMETER_SLIDER)].into(),
        OUTPUT_FORMAT => format!("输出格式：{}", cfg.output_format.label()),
        500 => "关闭贴图".into(),
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
            .chain([
                200,
                201,
                START_CAPTURE,
                300,
                301,
                400,
                OUTPUT_FORMAT,
                500,
                700,
                701,
                702,
                800,
            ])
            .chain(410..=417)
            .chain(PARAMETER_SLIDER..PARAMETER_SLIDER + PARAMETER_LABELS.len() as u16)
            .chain(PARAMETER_EDIT..PARAMETER_EDIT + PARAMETER_LABELS.len() as u16)
            .chain(600..=602);
        for id in ids {
            if setting_parameter(id, PARAMETER_EDIT).is_some() {
                let child = parameter_control::create_edit(hwnd, id, (*p).font)?;
                (*p).controls.push(Control { hwnd: child, id });
                continue;
            }
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
            } else if id != BRAND
                && !SetWindowSubclass(child, Some(button_proc), 1, usize::from(id)).as_bool()
            {
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
        state.error.as_deref(),
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
        let mut current_client = RECT::default();
        if GetClientRect(hwnd, &mut current_client).is_ok()
            && (current_client.right != client.right || current_client.bottom != client.bottom)
        {
            // Showing or hiding the scrollbar synchronously sends WM_SIZE, whose
            // nested layout uses the new client area. Do not overwrite it with
            // positions measured before SetScrollInfo changed the window frame.
            return;
        }
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
            if let Some(Rect {
                mut x,
                mut y,
                mut w,
                mut h,
            }) = bounds
            {
                if setting_parameter(id, PARAMETER_EDIT).is_some() {
                    x += 5;
                    w -= 10;
                    y += 4;
                    h -= 8;
                }
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

        c.begin(dark);
        c.fill(
            0.,
            0.,
            page.sidebar as f32,
            height,
            appearance::sidebar(dark),
        );
        c.heading("kiki", 63., 21., 75., 24., fg);
        for item in &page.items {
            match item {
                Item::Heading(label, y) => c.heading(label, x, *y as f32 - offset, w, 16., fg),
                Item::Text(label, r, tone) => {
                    let (size, color) = match tone {
                        Tone::Normal => (14., fg),
                        Tone::Muted => (12., sub),
                        Tone::Warning => (12., fg),
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
                Item::CornerPreview(r) => {
                    let cfg = preferences::current();
                    c.corner_preview(
                        r.x as f32,
                        r.y as f32 - offset,
                        cfg.corner_radius,
                        cfg.output_format,
                        dark,
                    );
                }
                Item::Control(id, r)
                    if setting_parameter(*id, PARAMETER_EDIT).is_some()
                        && r.y - state.scroll >= HEADER
                        && r.y + r.h - state.scroll <= height as i32 - FOOTER =>
                {
                    let focused = state
                        .controls
                        .iter()
                        .any(|c| c.id == *id && c.hwnd == GetFocus());
                    c.input_frame(
                        r.x as f32,
                        r.y as f32 - offset,
                        r.w as f32,
                        r.h as f32,
                        if focused {
                            appearance::accent(dark)
                        } else {
                            appearance::divider(dark)
                        },
                    );
                }
                Item::Control(..) => {}
            }
        }
        let sidebar = page.sidebar as f32;
        c.fill(sidebar, 0., width - sidebar, HEADER as f32, bg);
        c.heading(PAGES[state.page], x, 19., w, 24., fg);
        let subtitle = match state.page {
            0 => Some("也可从任务栏右下角的 kiki 图标开始截图。"),
            2 => Some("默认样式只影响新截图。"),
            _ => None,
        };
        if let Some(subtitle) = subtitle {
            c.text(subtitle, x, 55., w, 12., sub);
        }
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
        let footer_error = (state.page != 1)
            .then_some(state.error.as_deref())
            .flatten();
        let message = footer_error
            .map(str::to_owned)
            .unwrap_or_else(|| match status {
                SaveStatus::Ready => String::new(),
                SaveStatus::Saving => "正在保存设置…".into(),
                SaveStatus::Saved if saved => "设置已保存".into(),
                SaveStatus::Saved => String::new(),
                SaveStatus::Failed(e) => format!("保存失败：{e}"),
            });
        let status_color = if saved {
            appearance::accent(dark)
        } else if state.error.is_some() || failed {
            fg
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
            START_CAPTURE => preferences::request_capture_from_settings(),
            300 => {
                let child = (*p).controls.iter().find(|c| c.id == 300).unwrap().hwnd;
                hotkey::control_value(child)
                    .map_err(|_| "请按 Ctrl 或 Alt，再按字母、数字或功能键。".to_string())
                    .and_then(preferences::set_shortcut)
            }
            301 => preferences::set_shortcut(Hotkey::default()),
            400 => {
                cfg.show_hints = !cfg.show_hints;
                preferences::update(cfg)
            }
            410..=417 => {
                cfg.color = COLORS[(id - 410) as usize];
                preferences::update(cfg)
            }
            OUTPUT_FORMAT => choose_output_format(hwnd, cfg.output_format),
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
            if id == 300 && result.is_ok() && hotkey_focused() {
                // Finish recording after the native control returns from its key handler.
                let _ = PostMessageW(Some(hwnd), HOTKEY_RECORDED, WPARAM(0), LPARAM(0));
            }
            (*p).error = result.err();
            refresh();
        }
    }
}

fn choose_output_format(hwnd: HWND, selected: OutputFormat) -> std::result::Result<(), String> {
    let mut bounds = RECT::default();
    // SAFETY: the settings button is live on this thread. No state reference
    // crosses the native menu's modal loop.
    unsafe {
        let button = GetDlgItem(Some(hwnd), i32::from(OUTPUT_FORMAT)).map_err(|e| e.to_string())?;
        GetWindowRect(button, &mut bounds).map_err(|e| e.to_string())?;
    }
    let entries = [
        (
            1,
            if selected == OutputFormat::Png {
                "✓ PNG"
            } else {
                "   PNG"
            },
            true,
        ),
        (
            2,
            if selected == OutputFormat::Jpeg {
                "✓ JPEG"
            } else {
                "   JPEG"
            },
            true,
        ),
    ];
    let format = match super::menu::popup(
        hwnd,
        bounds.left,
        bounds.bottom,
        super::menu::Style::Settings,
        &entries,
    ) {
        1 => OutputFormat::Png,
        2 => OutputFormat::Jpeg,
        _ => return Ok(()),
    };
    let mut config = preferences::current();
    config.output_format = format;
    preferences::update(config)
}
fn commit_parameter_edit(parent: HWND, edit: HWND, parameter: CaptureParameter, commit: bool) {
    // SAFETY: the settings window owns the edit and the preference change is UI-thread-only.
    unsafe {
        let p = pointer(parent);
        if p.is_null() || (*p).suppress {
            return;
        }
        if commit {
            (*p).error = parameter_control::read_edit(edit, parameter)
                .map_err(str::to_owned)
                .and_then(|value| preferences::set_parameter(parameter, value))
                .err();
        }
        parameter_control::set_edit(edit, parameter.value(&preferences::current()));
        let _ = InvalidateRect(Some(parent), None, false);
    }
}

fn set_slider_value(parent: HWND, parameter: CaptureParameter, value: u16) {
    // SAFETY: this function is called by live settings controls on the window thread.
    unsafe {
        let p = pointer(parent);
        if !p.is_null() {
            (*p).error = preferences::set_parameter(parameter, value).err();
        }
    }
    refresh_parameters();
}

fn slider_input(
    hwnd: HWND,
    parent: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    parameter: CaptureParameter,
) -> Option<LRESULT> {
    // SAFETY: the button, its parent and mouse capture belong to the current window thread.
    unsafe {
        match msg {
            WM_GETDLGCODE => return Some(LRESULT(DLGC_WANTARROWS as isize)),
            WM_KEYDOWN => {
                let value = parameter.value(&preferences::current());
                let (min, max) = parameter.bounds();
                let next = match wp.0 {
                    37 | 40 => parameter_control::step(parameter, value, -1),
                    38 | 39 => parameter_control::step(parameter, value, 1),
                    35 => max,
                    36 => min,
                    _ => return None,
                };
                set_slider_value(parent, parameter, next);
                return Some(LRESULT(0));
            }
            WM_MOUSEWHEEL => return Some(SendMessageW(parent, msg, Some(wp), Some(lp))),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let _ = SetFocus(Some(hwnd));
                SetCapture(hwnd);
            }
            WM_MOUSEMOVE | WM_LBUTTONUP if GetCapture() == hwnd => {}
            _ => return None,
        }
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_ok() {
            let inset = (6. * GetDpiForWindow(hwnd) as f32 / 96.).round();
            let value = parameter_control::slider_value(
                parameter,
                f32::from(lp.0 as i16),
                inset,
                rect.right as f32 - inset,
            );
            set_slider_value(parent, parameter, value);
        }
        if msg == WM_LBUTTONUP {
            let _ = ReleaseCapture();
        }
        Some(LRESULT(0))
    }
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    id: usize,
) -> LRESULT {
    // SAFETY: the subclass belongs to a live settings child. The parent owns the state and
    // removes its pointer before freeing it. This callback does not retain a state reference.
    unsafe {
        if let Ok(parent) = GetParent(hwnd) {
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
            if let Some(parameter) = setting_parameter(id as u16, PARAMETER_SLIDER)
                && let Some(result) = slider_input(hwnd, parent, msg, wp, lp, parameter)
            {
                return result;
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
        let dpi_scale = state.dpi as f32 / 96.;
        let dip = |v: i32| (v as f32 * dpi_scale).round() as i32;
        let r = item.rcItem;
        let dc = item.hDC;
        let nav = (100..=105).contains(&id);
        let backdrop = if nav || id == BRAND {
            appearance::sidebar(dark)
        } else {
            appearance::background(dark)
        };
        if id == BRAND {
            fill_rect(dc, &r, backdrop);
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
        let labels = appearance::paint_control(dc, r, backdrop, |shapes| {
            let mut labels = Vec::new();
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
                    shapes.rounded(r, dip(12), appearance::hover(dark));
                }
                draw_toggle(shapes, r, dip, dark, on, pressed);
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
                let needs_outline = matches!((id, dark), (416, false) | (417, true));
                if hovered || pressed || needs_outline {
                    shapes.rounded(
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
                    shapes.rounded(inner, size - 2, color);
                } else {
                    shapes.rounded(swatch, size, color);
                }
                if selected {
                    let mark = if red * 3 + green * 6 + blue > 1400 {
                        0x202226
                    } else {
                        0xffffff
                    };
                    labels.push((state.icon_font, "\u{e73e}".to_owned(), r, mark, DT_CENTER));
                }
            } else if let Some(parameter) = setting_parameter(id, PARAMETER_SLIDER) {
                let left = r.left + dip(6);
                let right = r.right - dip(6);
                let y = (r.top + r.bottom) / 2;
                let radius = dip(5).max(1);
                let x = left
                    + (parameter_control::fraction(parameter, parameter.value(&cfg))
                        * (right - left) as f32)
                        .round() as i32;
                let track = RECT {
                    left,
                    top: y - dip(1),
                    right,
                    bottom: y + dip(2),
                };
                shapes.rounded(track, dip(3), appearance::divider(dark));
                if x > left {
                    shapes.rounded(RECT { right: x, ..track }, dip(3), accent);
                }
                shapes.rounded(
                    RECT {
                        left: x - radius,
                        top: y - radius,
                        right: x + radius,
                        bottom: y + radius,
                    },
                    2 * radius,
                    accent,
                );
            } else {
                let selected = match id {
                    100..=105 => usize::from(id - 100) == state.page,
                    600..=602 => {
                        cfg.theme
                            == [Theme::System, Theme::Light, Theme::Dark][usize::from(id - 600)]
                    }
                    _ => false,
                };
                let theme = (600..=602).contains(&id);
                let primary = id == START_CAPTURE && !disabled;
                let fill = match (pressed, selected && !theme, hovered && !disabled) {
                    (true, _, _) if primary => Some(appearance::accent_pressed(dark)),
                    (_, _, true) if primary => Some(appearance::accent_hover(dark)),
                    _ if primary => Some(accent),
                    (true, _, _) => Some(appearance::pressed(dark)),
                    (_, true, _) => Some(appearance::selected(dark)),
                    (_, _, true) => Some(appearance::hover(dark)),
                    _ if matches!(id, 201 | 301 | 500 | 700..=702 | 800) => {
                        Some(appearance::sidebar(dark))
                    }
                    _ => None,
                };
                if let Some(fill) = fill {
                    shapes.rounded(r, dip(12), fill);
                }
                let text_color = if disabled {
                    appearance::muted(dark)
                } else if primary {
                    appearance::on_accent(dark)
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
                        labels.push((
                            state.icon_font,
                            ICONS[usize::from(id - 100)].to_owned(),
                            icon,
                            text_color,
                            DT_LEFT,
                        ));
                        label_bounds.left += dip(42);
                        if selected {
                            let mark = RECT {
                                left: r.left + dip(1),
                                top: r.top + dip(6),
                                right: r.left + dip(4),
                                bottom: r.bottom - dip(6),
                            };
                            shapes.rounded(mark, dip(3), accent);
                        }
                        PAGES[usize::from(id - 100)].to_owned()
                    }
                    600..=602 => {
                        draw_preview(shapes, r, dip, id);
                        label_bounds.top = r.bottom - dip(36);
                        let name = ["跟随系统", "浅色", "深色"][usize::from(id - 600)];
                        if selected {
                            format!("✓ {name}")
                        } else {
                            name.to_owned()
                        }
                    }
                    OUTPUT_FORMAT => {
                        shapes.focus_ring(r, dip(6) as f32, dpi_scale, appearance::divider(dark));
                        label_bounds.right -= dip(20);
                        let center_x = r.right - dip(15);
                        let center_y = (r.top + r.bottom) / 2;
                        shapes.chevron_down(
                            RECT {
                                left: center_x - dip(4),
                                right: center_x + dip(4),
                                top: center_y - dip(2),
                                bottom: center_y + dip(2),
                            },
                            1.25 * dpi_scale,
                            text_color,
                        );
                        cfg.output_format.label().to_owned()
                    }
                    _ => caption(id, &cfg, &state.startup),
                };
                labels.push((
                    state.font,
                    label,
                    label_bounds,
                    text_color,
                    if nav { DT_LEFT } else { DT_CENTER },
                ));
            }
            if focused && !disabled {
                let inset = dip(2);
                let frame = RECT {
                    left: r.left + inset,
                    top: r.top + inset,
                    right: r.right - inset,
                    bottom: r.bottom - inset,
                };
                let focus = if id == START_CAPTURE {
                    appearance::on_accent(dark)
                } else {
                    accent
                };
                shapes.focus_ring(frame, dip(4) as f32, dpi_scale, focus);
            }
            labels
        });
        if let Some(labels) = labels {
            for (font, label, bounds, color, align) in labels {
                draw_label(dc, font, &label, bounds, color, align);
            }
        } else {
            // Keep the native action readable during graphics-target recovery.
            fill_rect(dc, &r, backdrop);
            draw_label(
                dc,
                state.font,
                &caption(id, &cfg, &state.startup),
                r,
                fg,
                DT_CENTER,
            );
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

fn draw_preview(
    shapes: &appearance::ControlShapes<'_>,
    r: RECT,
    dip: impl Fn(i32) -> i32,
    id: u16,
) {
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
        shapes.rounded(thumb, dip(12), appearance::sidebar(dark));
        let x = thumb.left + w / 3;
        let y = thumb.top + h / 6;
        let panel = RECT {
            left: x,
            top: y,
            right: thumb.right - w / 14,
            bottom: thumb.bottom - h / 8,
        };
        shapes.rounded(panel, dip(6), appearance::background(dark));
        for (index, length) in [(0, 43), (1, 34), (2, 39)] {
            let top = y + (index + 1) * h / 6;
            shapes.rounded(
                RECT {
                    left: x + w / 16,
                    top,
                    right: x + w * length / 100,
                    bottom: top + dip(4),
                },
                dip(4),
                appearance::divider(dark),
            );
        }
        let left = thumb.left + w / 13;
        for index in 0..3 {
            let top = y + (index + 1) * h / 6;
            shapes.rounded(
                RECT {
                    left,
                    top,
                    right: left + w / 7,
                    bottom: top + dip(4),
                },
                dip(4),
                if index == 0 {
                    appearance::accent(dark)
                } else {
                    appearance::divider(dark)
                },
            );
        }
    };
    if id == 600 {
        shapes.clipped(
            RECT {
                right: split,
                ..thumb
            },
            || miniature(false),
        );
        shapes.clipped(
            RECT {
                left: split,
                ..thumb
            },
            || miniature(true),
        );
    } else {
        miniature(id == 602);
    }
}

fn draw_toggle(
    shapes: &appearance::ControlShapes<'_>,
    r: RECT,
    dip: impl Fn(i32) -> i32,
    dark: bool,
    on: bool,
    pressed: bool,
) {
    let (width, height) = (dip(42), dip(22));
    let left = r.left + (r.right - r.left - width) / 2;
    let top = r.top + (r.bottom - r.top - height) / 2;
    let track = match (on, dark, pressed) {
        (true, _, true) => appearance::accent_pressed(dark),
        (true, _, false) => appearance::accent(dark),
        (false, true, _) => 0x797166,
        (false, false, true) => 0x776f63,
        (false, false, false) => 0x8b8275,
    };
    shapes.rounded(
        RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
        height,
        track,
    );
    let padding = dip(2);
    let size = height - 2 * padding;
    let x = if on {
        left + width - padding - size
    } else {
        left + padding
    };
    let y = top + padding;
    shapes.rounded(
        RECT {
            left: x,
            top: y,
            right: x + size,
            bottom: y + size,
        },
        size,
        if on {
            appearance::on_accent(dark)
        } else {
            0xffffff
        },
    );
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
        if matches!(msg, WM_MEASUREITEM | WM_DRAWITEM)
            && let Some(result) = super::menu::handle(msg, lp)
        {
            return result;
        }
        match msg {
            RESTORE_HOTKEY => {
                restore_hotkey_input();
                LRESULT(0)
            }
            HOTKEY_RECORDED => {
                if hotkey_focused() {
                    let navigation = (*p).controls.iter().find(|c| c.id == 101).map(|c| c.hwnd);
                    if let Some(navigation) = navigation {
                        let _ = SetFocus(Some(navigation));
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wp.0 & 65535) as u16;
                let code = (wp.0 >> 16) as u32;
                if let Some(parameter) = setting_parameter(id, PARAMETER_EDIT) {
                    let child = HWND(lp.0 as *mut _);
                    if code == EN_KILLFOCUS && !child.is_invalid() {
                        commit_parameter_edit(hwnd, child, parameter, true);
                    } else if code == EN_SETFOCUS {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                } else if code == BN_CLICKED || id == 300 && code == EN_CHANGE {
                    action(hwnd, id);
                }
                LRESULT(0)
            }
            parameter_control::EDIT_KEY => {
                let edit = HWND(lp.0 as *mut _);
                let id = GetDlgCtrlID(edit) as u16;
                if let Some(parameter) = setting_parameter(id, PARAMETER_EDIT) {
                    commit_parameter_edit(hwnd, edit, parameter, wp.0 != 27);
                    SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
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
                let focus = GetFocus();
                let mut point = POINT::default();
                let hovered = GetCursorPos(&mut point)
                    .is_ok()
                    .then(|| WindowFromPoint(point));
                let id = if hovered == Some(focus) && GetAncestor(focus, GA_ROOT) == hwnd {
                    GetDlgCtrlID(focus) as u16
                } else {
                    0
                };
                let edit = setting_parameter(id, PARAMETER_EDIT);
                if let Some(parameter) = edit.or_else(|| setting_parameter(id, PARAMETER_SLIDER)) {
                    if (*p).parameter_wheel.0 != Some(focus) {
                        (*p).parameter_wheel = (Some(focus), Wheel::default());
                    }
                    let steps = (*p).parameter_wheel.1.steps(wp);
                    if steps != 0 {
                        if edit.is_some() {
                            commit_parameter_edit(hwnd, focus, parameter, true);
                            if (*p).error.is_some() {
                                return LRESULT(0);
                            }
                        }
                        let width = parameter_control::step(
                            parameter,
                            parameter.value(&preferences::current()),
                            steps,
                        );
                        set_slider_value(hwnd, parameter, width);
                        if edit.is_some() {
                            parameter_control::set_edit(focus, width);
                            SendMessageW(focus, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                        }
                    }
                    return LRESULT(0);
                }
                (*p).parameter_wheel = (None, Wheel::default());
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
                appearance::release_control_surface();
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
        "开始截图：{}，或点击系统托盘图标。\r\n\r\n拖动选择区域；拖动边框调整。\r\n选择矩形、箭头、画笔或文字进行标注。\r\nCtrl+Z 撤销；Ctrl+Y 重做；Esc 取消。\r\n复制后可用 Ctrl+V 粘贴。保存可选择 PNG 或 JPEG。贴图会把截图置顶。\r\n\r\n贴图：拖动移动，滚轮缩放，Ctrl+0 原始比例。\r\nCtrl+C 复制，Ctrl+S 保存，Esc 关闭。\r\n\r\n右键托盘可打开设置、关闭贴图或退出。",
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
            assert!(warning.is_none());
            assert!(!preferences::active());
            let mut unrelated = cfg.clone();
            unrelated.show_hints = false;
            preferences::update(unrelated.clone()).unwrap();
            assert_eq!(preferences::current(), unrelated);
            assert!(!preferences::active());
            drop(initial_registration);
            DestroyWindow(startup_blocker).unwrap();
            preferences::set_shortcut(unrelated.hotkey).unwrap();
            assert!(preferences::active());
            preferences::update(cfg.clone()).unwrap();
            open_impl(false).unwrap();
            let hwnd = WINDOW.get().unwrap();
            let life = (*pointer(hwnd)).life.clone();
            // Entering a page that first shows the scrollbar must lay out against
            // the resulting client width, including native EDIT text and painted frames.
            SetWindowPos(hwnd, None, 0, 0, 1000, 700, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
            for page in [2, 0, 2] {
                SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(100 + page)), None);
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client).unwrap();
                let state = &*pointer(hwnd);
                let scale = state.dpi as f32 / 96.;
                let px = |value: i32| (value as f32 * scale).round() as i32;
                let geometry = page_geometry(state, (client.right as f32 / scale).round() as i32);
                for item in &geometry.items {
                    let Item::Control(id, bounds) = item else {
                        continue;
                    };
                    let child = GetDlgItem(Some(hwnd), i32::from(*id)).unwrap();
                    let top = bounds.y - state.scroll;
                    if top < HEADER
                        || top + bounds.h > (client.bottom as f32 / scale).round() as i32 - FOOTER
                    {
                        continue;
                    }
                    let inset = if setting_parameter(*id, PARAMETER_EDIT).is_some() {
                        (5, 4)
                    } else {
                        (0, 0)
                    };
                    let mut actual = RECT::default();
                    GetWindowRect(child, &mut actual).unwrap();
                    let mut origin = POINT {
                        x: actual.left,
                        y: actual.top,
                    };
                    assert!(ScreenToClient(hwnd, &mut origin).as_bool());
                    assert_eq!(
                        origin.x,
                        px(bounds.x + inset.0),
                        "control {id} left after page {page}"
                    );
                    assert_eq!(
                        origin.y,
                        px(top + inset.1),
                        "control {id} top after page {page}"
                    );
                    assert_eq!(
                        actual.right - actual.left,
                        px(bounds.x + bounds.w - inset.0) - origin.x,
                        "control {id} width"
                    );
                }
            }
            for page in 0..6 {
                SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(100 + page)), None);
                assert_eq!((*pointer(hwnd)).page, page);
                assert!(paint(hwnd, pointer(hwnd)).is_ok());
                if page == 1 {
                    let input = (*pointer(hwnd))
                        .controls
                        .iter()
                        .find(|c| c.id == 300)
                        .unwrap()
                        .hwnd;
                    let mut window = RECT::default();
                    let mut client = RECT::default();
                    GetWindowRect(input, &mut window).unwrap();
                    GetClientRect(input, &mut client).unwrap();
                    assert_eq!(client.right, window.right - window.left);
                    assert_eq!(client.bottom, window.bottom - window.top);
                }
            }
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(602)), None);
            assert!(system_dark());
            assert_eq!(preferences::current().theme, Theme::Dark);
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(400)), None);
            assert!(!preferences::hints());
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(414)), None);
            let width_edit = GetDlgItem(Some(hwnd), i32::from(PARAMETER_EDIT)).unwrap();
            parameter_control::set_edit(width_edit, 8);
            SendMessageW(width_edit, WM_KEYDOWN, Some(WPARAM(13)), None);
            let size_edit = GetDlgItem(Some(hwnd), i32::from(PARAMETER_EDIT + 3)).unwrap();
            parameter_control::set_edit(size_edit, 256);
            SendMessageW(size_edit, WM_KEYDOWN, Some(WPARAM(13)), None);
            let radius_edit = GetDlgItem(Some(hwnd), i32::from(PARAMETER_EDIT + 4)).unwrap();
            assert_eq!(preferences::current().corner_radius, 0);
            parameter_control::set_edit(radius_edit, 256);
            SendMessageW(radius_edit, WM_KEYDOWN, Some(WPARAM(13)), None);
            parameter_control::set_edit(radius_edit, 257);
            SendMessageW(radius_edit, WM_KEYDOWN, Some(WPARAM(13)), None);
            assert!((*pointer(hwnd)).error.is_some());
            assert_eq!(preferences::current().corner_radius, 256);
            assert_eq!(
                (
                    preferences::current().color,
                    preferences::current().stroke_widths,
                    preferences::current().text_size
                ),
                (COLORS[4], [8, 3, 3], 256)
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
            SendMessageW(hwnd, WM_COMMAND, Some(WPARAM(101)), None);
            let child = (*pointer(hwnd))
                .controls
                .iter()
                .find(|c| c.id == 300)
                .unwrap()
                .hwnd;
            let _ = SetFocus(Some(child));
            assert!(hotkey_focused());
            let mut gui = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            GetGUIThreadInfo(GetWindowThreadProcessId(child, None), &mut gui).unwrap();
            assert_ne!(gui.hwndCaret, child);
            hotkey::set_control(child, candidate);
            SendMessageW(
                hwnd,
                WM_COMMAND,
                Some(WPARAM(300 | ((EN_CHANGE as usize) << 16))),
                Some(LPARAM(child.0 as isize)),
            );
            assert_eq!(preferences::current().hotkey, candidate);
            let mut recorded = MSG::default();
            assert!(
                PeekMessageW(
                    &mut recorded,
                    Some(hwnd),
                    HOTKEY_RECORDED,
                    HOTKEY_RECORDED,
                    PM_REMOVE
                )
                .as_bool()
            );
            DispatchMessageW(&recorded);
            assert!(!hotkey_focused());
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
