//! Real Win32 controls, native keyboard navigation and a Direct2D settings surface.
use super::{
    hotkey,
    preferences::{self, SaveStatus},
    selection_render::system_dark,
    startup,
};
use crate::core::config::{Config, Hotkey, Theme};
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
const COLORS: [[u8; 3]; 8] = super::selection_render::ANNOTATION_COLORS;
thread_local! {static WINDOW:Cell<Option<HWND>>=const{Cell::new(None)};}
struct Control {
    hwnd: HWND,
    id: u16,
    page: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
struct State {
    life: Rc<Cell<Option<HWND>>>,
    page: usize,
    scroll: i32,
    dpi: u32,
    controls: Vec<Control>,
    canvas: Option<super::settings_render::Canvas>,
    font: HFONT,
    brush: HBRUSH,
    suppress: bool,
    error: Option<String>,
    startup: std::result::Result<startup::State, String>,
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.font.into());
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
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            hIcon: super::icon::application(module.into(), false)?,
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
        let dpi = GetDpiForSystem().max(96);
        let s = dpi as f32 / 96.;
        let width = (1040. * s) as i32;
        let height = (780. * s) as i32;
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
        let font = font(dpi);
        let state = Box::new(State {
            life: Rc::new(Cell::new(Some(hwnd))),
            page: 0,
            scroll: 0,
            dpi,
            controls: Vec::new(),
            canvas: None,
            font,
            brush: CreateSolidBrush(rgb(if system_dark() { 0x2b2b2b } else { 0xffffff })),
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
    WINDOW
        .get()
        .is_some_and(|hwnd| unsafe { IsDialogMessageW(hwnd, message).as_bool() })
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
            let caption = rgb(if dark.as_bool() { 0x202020 } else { 0xf3f3f3 });
            let text = rgb(if dark.as_bool() { 0xf2f2f2 } else { 0x202020 });
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                &caption as *const _ as *const _,
                4,
            );
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, &text as *const _ as *const _, 4);
            let brush = CreateSolidBrush(rgb(if dark.as_bool() { 0x2b2b2b } else { 0xffffff }));
            let old = std::mem::replace(&mut (*p).brush, brush);
            let _ = DeleteObject(old.into());
            (*p).canvas = None;
            (*p).suppress = false;
            layout(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
}
unsafe fn pointer(hwnd: HWND) -> *mut State {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State }
}
fn font(dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -((14 * dpi / 96) as i32),
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
            DEFAULT_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
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
    unsafe {
        let p = pointer(hwnd);
        let mut items = Vec::new();
        for i in 0..6 {
            items.push((100 + i, 6, 20, 104 + i32::from(i) * 48, 164, 40));
        }
        items.extend([
            (200, 0, 464, 150, 224, 36),
            (201, 0, 464, 194, 224, 36),
            (300, 1, 244, 160, 280, 36),
            (301, 1, 544, 160, 120, 36),
            (400, 2, 244, 138, 220, 36),
            (500, 3, 244, 332, 196, 36),
            (600, 4, 244, 160, 136, 40),
            (601, 4, 396, 160, 120, 40),
            (602, 4, 532, 160, 120, 40),
            (700, 5, 244, 230, 180, 36),
            (701, 5, 244, 290, 180, 36),
            (702, 5, 444, 290, 180, 36),
            (800, 6, 0, 0, 100, 32),
        ]);
        for i in 0..8 {
            items.push((410 + i, 2, 244 + i32::from(i) * 56, 270, 48, 36));
        }
        for i in 0..4 {
            items.push((420 + i, 2, 244 + i32::from(i) * 100, 380, 88, 36));
            items.push((430 + i, 2, 244 + i32::from(i) * 100, 506, 88, 36));
        }
        for (id, page, x, y, w, h) in items {
            let child = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                if id == 300 {
                    HOTKEY_CLASSW
                } else {
                    w!("BUTTON")
                },
                w!(""),
                WS_CHILD
                    | WS_TABSTOP
                    | WINDOW_STYLE(if id == 300 { 0 } else { BS_OWNERDRAW as u32 }),
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
            }
            (*p).controls.push(Control {
                hwnd: child,
                id,
                page: page as usize,
                x,
                y,
                w,
                h,
            });
        }
        Ok(())
    }
}
fn layout(hwnd: HWND) {
    unsafe {
        let p = pointer(hwnd);
        if p.is_null() {
            return;
        }
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let s = (*p).dpi as f32 / 96.;
        let height = (rect.bottom as f32 / s) as i32;
        let width = (rect.right as f32 / s) as i32;
        let page = (*p).page;
        let extent = match page {
            0 => 676,
            1 => 350,
            2 => 570,
            3 => 420,
            4 => 330,
            _ => 400,
        };
        let viewport = (height - 132).max(1);
        let maximum = (extent - 90 - viewport).max(0);
        (*p).scroll = (*p).scroll.clamp(0, maximum);
        let scroll = (*p).scroll;
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: extent - 91,
            nPage: viewport as u32,
            nPos: scroll,
            ..Default::default()
        };
        SetScrollInfo(hwnd, SB_VERT, &info, true);
        let controls: Vec<_> = (*p)
            .controls
            .iter()
            .map(|c| (c.hwnd, c.id, c.page, c.x, c.y, c.w, c.h))
            .collect();
        let failed = matches!(preferences::status(), SaveStatus::Failed(_));
        for (child, id, pag, x, y, w, h) in controls {
            let (x, y) = if id == 800 {
                (width - 136, height - 38)
            } else if pag == 6 {
                (x, y)
            } else {
                (x, y - scroll)
            };
            let visible = if id == 201 && !matches!((*p).startup, Ok(startup::State::Moved)) {
                false
            } else if id == 800 {
                failed
            } else {
                pag == 6 || pag == page && y >= 94 && y + h <= height - 52
            };
            let _ = InvalidateRect(Some(child), None, true);
            let _ = SetWindowPos(
                child,
                None,
                (x as f32 * s) as i32,
                (y as f32 * s) as i32,
                (w as f32 * s) as i32,
                (h as f32 * s) as i32,
                SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | if visible {
                        SWP_SHOWWINDOW
                    } else {
                        SWP_HIDEWINDOW
                    },
            );
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
fn paint(hwnd: HWND, p: *mut State) -> Result<()> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        let state = &mut *p;
        if state.canvas.is_none() {
            state.canvas = Some(super::settings_render::Canvas::new(
                hwnd,
                rect.right as u32,
                rect.bottom as u32,
                state.dpi,
            )?);
        }
        // Canvas was created successfully in the immediately preceding branch.
        let c = state.canvas.as_ref().unwrap();
        let dark = system_dark();
        let fg = if dark { 0xf2f2f2 } else { 0x202020 };
        let sub = if dark { 0xb6b6b6 } else { 0x606060 };
        let bg = if dark { 0x202020 } else { 0xf3f3f3 };
        let s = state.dpi as f32 / 96.;
        let width = rect.right as f32 / s;
        let height = rect.bottom as f32 / s;
        let offset = state.scroll as f32;
        let cfg = preferences::current();
        c.begin(dark);
        c.fill(0., 0., 204., height, if dark { 0x252525 } else { 0xececec });
        c.text("kiki", 24., 24., 160., 26., fg);
        c.text("设置", 24., 62., 160., 13., sub);
        let x = 224.;
        let w = width - x - 24.;
        let card = |y, h| c.card(x, y - offset, w, h, dark);
        let text =
            |label: &str, y, size, rgb| c.text(label, x + 20., y - offset, w - 40., size, rgb);
        match state.page {
            0 => {
                card(104., 174.);
                text("启动与后台", 116., 16., fg);
                text("开机启动", 154., 14., fg);
                text("启动后驻留系统托盘", 238., 14., sub);
                card(294., 172.);
                text("截图完成后", 306., 16., fg);
                text("复制 / 保存 / 贴图成功后退出本次截图", 346., 14., fg);
                text("取消保存或操作失败时保留当前截图", 380., 14., sub);
                text("关闭设置窗口后继续在托盘运行", 414., 14., sub);
                card(482., 174.);
                text("文件保存", 494., 16., fg);
                text(
                    "图片格式                   PNG（保留透明度）",
                    536.,
                    14.,
                    fg,
                );
                text(
                    "保存位置                   每次询问，记住上次目录",
                    574.,
                    14.,
                    fg,
                );
                text(
                    "文件名称                   kiki_年月日_时分秒.png",
                    612.,
                    14.,
                    fg,
                );
                if matches!(state.startup, Ok(startup::State::Moved)) {
                    text("启动项指向旧路径，请更新或关闭", 194., 12., 0xd08030);
                }
            }
            1 => {
                card(104., 216.);
                text("开始截图", 116., 16., fg);
                text(
                    &format!(
                        "当前组合：{}{}",
                        cfg.hotkey.label(),
                        if preferences::active() {
                            ""
                        } else {
                            "（注册失败）"
                        }
                    ),
                    216.,
                    14.,
                    fg,
                );
                text(
                    "按 Ctrl 或 Alt 加字母、数字、F1–F24；冲突时保留原组合。",
                    252.,
                    13.,
                    sub,
                );
                text(
                    "聚焦输入框时暂停启动截图；更改有效后立即生效。",
                    280.,
                    13.,
                    sub,
                );
            }
            2 => {
                card(104., 94.);
                text("显示操作提示", 116., 16., fg);
                card(214., 110.);
                text("新截图的默认颜色", 226., 16., fg);
                card(340., 110.);
                text(
                    &format!("默认线宽 · 当前 {} px", cfg.stroke_width),
                    352.,
                    16.,
                    fg,
                );
                card(466., 96.);
                text(
                    &format!("默认字号 · 当前 {} px", cfg.text_size),
                    472.,
                    16.,
                    fg,
                );
            }
            3 => {
                card(104., 294.);
                text("独立贴图窗口", 116., 16., fg);
                text("拖动移动；滚轮缩放；Ctrl+0 恢复原始比例", 160., 14., fg);
                text(
                    "Ctrl+C 复制 / Ctrl+S 保存 / Esc 关闭当前贴图",
                    200.,
                    14.,
                    sub,
                );
                text("右键打开菜单；悬停显示复制、保存、关闭", 240., 14., sub);
                text("初次显示按原始大小，超出工作区时自动缩小。", 280., 13., sub);
            }
            4 => {
                card(104., 196.);
                text("主题", 116., 16., fg);
                text("立即应用到设置、截图工具、文字输入和贴图。", 224., 14., sub);
                text("跟随系统会在 Windows 主题变化时更新。", 258., 13., sub);
            }
            _ => {
                card(104., 244.);
                text(
                    &format!("kiki {}", env!("CARGO_PKG_VERSION")),
                    116.,
                    22.,
                    fg,
                );
                text("Windows 11 · x64 · 原生截图与贴图", 158., 14., sub);
                text(&format!("截图热键：{}", cfg.hotkey.label()), 196., 14., fg);
            }
        }
        // Header and footer cover scrolling content; controls are clipped by visibility rules.
        c.fill(204., 0., width - 204., 94., bg);
        c.text(PAGES[state.page], 244., 24., w - 24., 26., fg);
        c.text(
            [
                "设置立即生效，自动保存到当前用户。",
                "随时可从系统托盘开始截图或恢复默认。",
                "仅影响新截图，不更改已有标注。",
                "贴图保留原图，缩放只影响显示。",
                "选择适合当前桌面的颜色。",
                "轻量截图，随手贴图。",
            ][state.page],
            244.,
            66.,
            w - 24.,
            13.,
            sub,
        );
        c.fill(204., height - 48., width - 204., 48., bg);
        let status = state
            .error
            .clone()
            .unwrap_or_else(|| match preferences::status() {
                SaveStatus::Ready => "设置更改后自动保存".into(),
                SaveStatus::Saving => "正在保存设置…".into(),
                SaveStatus::Saved => "设置已自动保存".into(),
                SaveStatus::Failed(e) => format!("保存失败：{e}"),
            });
        c.text(
            &status,
            244.,
            height - 34.,
            (w - 138.).max(100.),
            12.,
            if state.error.is_some() || matches!(preferences::status(), SaveStatus::Failed(_)) {
                0xd08030
            } else {
                sub
            },
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
            (*p).page = usize::from(id - 100);
            (*p).scroll = 0;
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
fn draw_button(item: &DRAWITEMSTRUCT, p: *mut State) {
    unsafe {
        let dark = system_dark();
        let id = item.CtlID as u16;
        let selected = if (100..=105).contains(&id) {
            usize::from(id - 100) == (*p).page
        } else {
            let cfg = preferences::current();
            match id {
                410..=417 => cfg.color == COLORS[(id - 410) as usize],
                420..=423 => cfg.stroke_width == [1, 3, 5, 8][(id - 420) as usize],
                430..=433 => cfg.text_size == [16, 24, 32, 48][(id - 430) as usize],
                400 => cfg.show_hints,
                600..=602 => {
                    preferences::current().theme
                        == [Theme::System, Theme::Light, Theme::Dark][(id - 600) as usize]
                }
                _ => false,
            }
        };
        let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
        let nav = (100..=105).contains(&id);
        let bg = if pressed || selected {
            if dark { 0x173e60 } else { 0xdcecff }
        } else if nav {
            if dark { 0x252525 } else { 0xececec }
        } else if dark {
            0x333333
        } else {
            0xf5f5f5
        };
        let brush = CreateSolidBrush(rgb(bg));
        let background = CreateSolidBrush(rgb(if nav {
            if dark { 0x252525 } else { 0xececec }
        } else if dark {
            0x2b2b2b
        } else {
            0xffffff
        }));
        FillRect(item.hDC, &item.rcItem, background);
        let _ = DeleteObject(background.into());
        let rect = item.rcItem;
        let radius = (8 * (*p).dpi / 96) as i32;
        let region = CreateRoundRectRgn(
            rect.left,
            rect.top,
            rect.right + 1,
            rect.bottom + 1,
            radius,
            radius,
        );
        let _ = FillRgn(item.hDC, region, brush);
        if !nav {
            let edge = CreateSolidBrush(rgb(if selected {
                0x0078d4
            } else if dark {
                0x555555
            } else {
                0xcccccc
            }));
            let _ = FrameRgn(item.hDC, region, edge, 1, 1);
            let _ = DeleteObject(edge.into());
        }
        let _ = DeleteObject(region.into());
        let _ = DeleteObject(brush.into());
        SetBkMode(item.hDC, TRANSPARENT);
        SetTextColor(item.hDC, rgb(if dark { 0xf2f2f2 } else { 0x202020 }));
        let old = SelectObject(item.hDC, (*p).font.into());
        let length = GetWindowTextLengthW(item.hwndItem);
        let mut text = vec![0u16; length.max(0) as usize + 1];
        GetWindowTextW(item.hwndItem, &mut text);
        let mut rect = item.rcItem;
        rect.left += 8;
        DrawTextW(
            item.hDC,
            &mut text[..length as usize],
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        SelectObject(item.hDC, old);
        if selected && (100..=105).contains(&id) {
            let brush = CreateSolidBrush(rgb(0x0078d4));
            let mut mark = item.rcItem;
            mark.right = mark.left + 3;
            FillRect(item.hDC, &mark, brush);
            let _ = DeleteObject(brush.into());
        }
        if item.itemState.0 & ODS_FOCUS.0 != 0 {
            let mut r = item.rcItem;
            r.left += 3;
            r.top += 3;
            r.right -= 3;
            r.bottom -= 3;
            let _ = DrawFocusRect(item.hDC, &r);
        }
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
                SetTextColor(dc, rgb(if system_dark() { 0xf2f2f2 } else { 0x202020 }));
                SetBkColor(dc, rgb(if system_dark() { 0x2b2b2b } else { 0xffffff }));
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
                (*p).canvas = None;
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
                let next = font((*p).dpi);
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
                    x: (850. * s) as i32,
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
