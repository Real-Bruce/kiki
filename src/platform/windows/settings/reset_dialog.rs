//! Owned Win32 confirmation for resetting capture preferences.
use super::{appearance, apply_window_theme, draw_label, fill_rect, font, rgb};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::SetFocus, WindowsAndMessaging::*},
    },
    core::{Error, PCWSTR, Result, w},
};

const STYLE: WINDOW_STYLE =
    WINDOW_STYLE(WS_POPUP.0 | WS_CAPTION.0 | WS_SYSMENU.0 | DS_MODALFRAME as u32);
const EX_STYLE: WINDOW_EX_STYLE = WS_EX_DLGMODALFRAME;
const WIDTH: i32 = 398;
const HEIGHT: i32 = 206;

// Win32 requires a DWORD-aligned header followed by three WORDs for the
// default menu/class/title. Child controls are created in WM_INITDIALOG.
#[repr(C, align(4))]
struct Template {
    header: DLGTEMPLATE,
    menu: u16,
    class: u16,
    title: u16,
}

const LABELS: [(PCWSTR, i32, i32); 4] = [
    (w!("恢复截图默认设置"), 18, 30),
    (
        w!("恢复工具条、操作提示、标注样式、保存格式和圆角。"),
        60,
        22,
    ),
    (w!("保留快捷键、主题、开机启动和保存位置。"), 85, 22),
    (w!("保留当前截图的标注和文字草稿。"), 110, 22),
];

struct Dialog {
    dark: bool,
    dpi: u32,
    brush: HBRUSH,
    fonts: [HFONT; 2],
    children: [HWND; 6],
    error: Option<Error>,
}

impl Drop for Dialog {
    fn drop(&mut self) {
        // SAFETY: the modal call has returned and no child uses the owned GDI objects.
        unsafe {
            let _ = DeleteObject(self.brush.into());
            for font in self.fonts {
                let _ = DeleteObject(font.into());
            }
        }
    }
}

pub(super) fn confirm(owner: HWND, dark: bool) -> Result<bool> {
    let template = Template {
        header: DLGTEMPLATE {
            style: STYLE.0,
            dwExtendedStyle: EX_STYLE.0,
            cx: 1,
            cy: 1,
            ..Default::default()
        },
        menu: 0,
        class: 0,
        title: 0,
    };
    // SAFETY: the brush and the dialog's stack context live until the synchronous
    // modal loop ends. The owner and all children belong to the calling UI thread.
    unsafe {
        let brush = CreateSolidBrush(rgb(appearance::background(dark)));
        if brush.is_invalid() {
            return Err(Error::from_win32());
        }
        let mut dialog = Dialog {
            dark,
            dpi: GetDpiForWindow(owner).max(96),
            brush,
            fonts: [HFONT::default(); 2],
            children: [HWND::default(); 6],
            error: None,
        };
        let result = DialogBoxIndirectParamW(
            Some(GetModuleHandleW(None)?.into()),
            &template.header,
            Some(owner),
            Some(dialog_proc),
            LPARAM((&raw mut dialog) as isize),
        );
        match (result, dialog.error.take()) {
            (_, Some(error)) => Err(error),
            (-1, None) => Err(Error::from_win32()),
            (result, None) => Ok(result == IDOK.0 as isize),
        }
    }
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    // SAFETY: this callback runs during the modal call on the owning UI thread.
    // End the dialog on an unexpected panic instead of unwinding across Win32.
    std::panic::catch_unwind(|| dispatch(hwnd, msg, wp, lp)).unwrap_or_else(|_| unsafe {
        let _ = EndDialog(hwnd, IDCANCEL.0 as isize);
        1
    })
}

fn dispatch(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> isize {
    // SAFETY: the dialog context is supplied by confirm and remains valid throughout
    // the modal loop. DRAWITEMSTRUCT and DPI bounds are borrowed from their messages.
    unsafe {
        let p = if msg == WM_INITDIALOG {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lp.0);
            lp.0 as *mut Dialog
        } else {
            GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dialog
        };
        if p.is_null() {
            return 0;
        }
        match msg {
            WM_INITDIALOG => {
                if let Err(error) = initialize(hwnd, p) {
                    (*p).error = Some(error);
                    let _ = EndDialog(hwnd, IDCANCEL.0 as isize);
                }
                // initialize explicitly focuses Cancel.
                0
            }
            WM_DPICHANGED => {
                let suggested = *(lp.0 as *const RECT);
                if let Err(error) = layout(hwnd, p, (wp.0 as u16 as u32).max(96), suggested, false)
                {
                    (*p).error = Some(error);
                    let _ = EndDialog(hwnd, IDCANCEL.0 as isize);
                }
                1
            }
            WM_CTLCOLORDLG => (*p).brush.0 as isize,
            WM_CTLCOLORSTATIC => {
                let dc = HDC(wp.0 as *mut _);
                let dark = (*p).dark;
                let _ = SetBkMode(dc, TRANSPARENT);
                let _ = SetTextColor(dc, rgb(appearance::foreground(dark)));
                (*p).brush.0 as isize
            }
            WM_DRAWITEM => {
                draw_button(
                    &*(lp.0 as *const DRAWITEMSTRUCT),
                    (*p).dpi,
                    (*p).dark,
                    (*p).fonts[0],
                );
                1
            }
            WM_COMMAND if (wp.0 >> 16) as u16 == BN_CLICKED as u16 => {
                let id = i32::from(wp.0 as u16);
                if id == IDOK.0 || id == IDCANCEL.0 {
                    let _ = EndDialog(hwnd, id as isize);
                    return 1;
                }
                0
            }
            WM_CLOSE => {
                let _ = EndDialog(hwnd, IDCANCEL.0 as isize);
                1
            }
            _ => 0,
        }
    }
}

fn initialize(hwnd: HWND, p: *mut Dialog) -> Result<()> {
    // SAFETY: hwnd and p belong to the current modal call. No context reference
    // is held across Win32 calls that may dispatch messages to this dialog.
    unsafe {
        apply_window_theme(hwnd, (*p).dark);
        SetWindowTextW(hwnd, w!("kiki 设置"))?;
        let instance = Some(GetModuleHandleW(None)?.into());
        let controls = LABELS
            .map(|(label, _, _)| (w!("STATIC"), label, WINDOW_STYLE(0), 0))
            .into_iter()
            .chain([
                (
                    w!("BUTTON"),
                    w!("取消"),
                    WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                    IDCANCEL.0,
                ),
                (
                    w!("BUTTON"),
                    w!("恢复默认"),
                    WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                    IDOK.0,
                ),
            ]);
        for (index, (class, label, style, id)) in controls.enumerate() {
            let child = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                label,
                WS_CHILD | style,
                0,
                0,
                1,
                1,
                Some(hwnd),
                (id != 0).then_some(HMENU(id as usize as *mut _)),
                instance,
                None,
            )?;
            (*p).children[index] = child;
        }
        let owner = GetWindow(hwnd, GW_OWNER)?;
        let mut bounds = RECT::default();
        GetWindowRect(owner, &mut bounds)?;
        layout(hwnd, p, (*p).dpi, bounds, true)?;
        SendMessageW(hwnd, DM_SETDEFID, Some(WPARAM(IDCANCEL.0 as usize)), None);
        let _ = SetFocus(Some((*p).children[4]));
        Ok(())
    }
}

fn layout(hwnd: HWND, p: *mut Dialog, dpi: u32, bounds: RECT, center: bool) -> Result<()> {
    let dip = |n: i32| (n * dpi as i32 + 48) / 96;
    let mut frame = RECT {
        right: dip(WIDTH),
        bottom: dip(HEIGHT),
        ..Default::default()
    };
    // SAFETY: all windows and fonts are used only on the dialog thread. Children
    // switch to the new fonts before the old fonts are released.
    unsafe {
        AdjustWindowRectExForDpi(&mut frame, STYLE, false, EX_STYLE, dpi)?;
        let width = frame.right - frame.left;
        let height = frame.bottom - frame.top;
        let mut x = bounds.left;
        let mut y = bounds.top;
        if center {
            x += (bounds.right - bounds.left - width) / 2;
            y += (bounds.bottom - bounds.top - height) / 2;
        }
        let monitor = MonitorFromRect(&bounds, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let work = info.rcWork;
            x = x.clamp(work.left, (work.right - width).max(work.left));
            y = y.clamp(work.top, (work.bottom - height).max(work.top));
        }
        (*p).dpi = dpi;
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
        let fonts = [
            font(dpi, 14, w!("Microsoft YaHei UI")),
            font(dpi, 16, w!("Microsoft YaHei UI")),
        ];
        let old = std::mem::replace(&mut (*p).fonts, fonts);
        let children = (*p).children;
        let positions = LABELS
            .map(|(_, y, h)| (24, y, WIDTH - 48, h))
            .into_iter()
            .chain([(WIDTH - 224, 152, 92, 34), (WIDTH - 122, 152, 98, 34)]);
        for (index, (child, (x, y, w, h))) in children.into_iter().zip(positions).enumerate() {
            let font = fonts[usize::from(index == 0)];
            SendMessageW(child, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
            let _ = SetWindowPos(
                child,
                None,
                dip(x),
                dip(y),
                dip(w),
                dip(h),
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        for font in old {
            let _ = DeleteObject(font.into());
        }
    }
    Ok(())
}

fn draw_button(item: &DRAWITEMSTRUCT, dpi: u32, dark: bool, font: HFONT) {
    let primary = item.CtlID == IDOK.0 as u32;
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let focused = item.itemState.0 & ODS_FOCUS.0 != 0;
    let background = appearance::background(dark);
    let accent = appearance::accent(dark);
    let scale = dpi as f32 / 96.;
    let radius = 6. * scale;
    let r = item.rcItem;
    let fill = match (primary, pressed) {
        (true, true) => appearance::accent_pressed(dark),
        (true, false) => accent,
        (false, true) => appearance::pressed(dark),
        (false, false) => background,
    };
    if appearance::paint_control(item.hDC, r, background, |shapes| {
        shapes.rounded(r, (2. * radius).round() as i32, fill);
        let border = if primary && focused {
            appearance::on_accent(dark)
        } else if focused {
            accent
        } else {
            appearance::divider(dark)
        };
        if focused || !primary {
            shapes.focus_ring(r, radius, if focused { 2. * scale } else { scale }, border);
        }
    })
    .is_none()
    {
        fill_rect(item.hDC, &r, fill);
    }
    let (label, color) = if primary {
        ("恢复默认", appearance::on_accent(dark))
    } else {
        ("取消", appearance::foreground(dark))
    };
    draw_label(item.hDC, font, label, r, color, DT_CENTER);
}
