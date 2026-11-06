use std::cell::RefCell;
// Owned, resizable, read-only native text window for help and bundled licenses.
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::EM_SETLIMITTEXT, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, Result, w},
};
thread_local! {static WINDOWS:RefCell<Vec<HWND>>=const{RefCell::new(Vec::new())};}
struct State {
    edit: HWND,
    font: HFONT,
    brush: HBRUSH,
}
pub fn open(owner: HWND, title: &str, text: &str) -> Result<()> {
    open_impl(owner, title, text, true)
}
fn open_impl(owner: HWND, title: &str, text: &str, visible: bool) -> Result<()> {
    // SAFETY: window, edit and owned font are created and destroyed on this UI thread; native EDIT copies the text.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("kiki.TextViewer"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_win32());
        }
        let title: Vec<_> = title.encode_utf16().chain(Some(0)).collect();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            760,
            600,
            Some(owner),
            None,
            Some(module.into()),
            None,
        )?;
        let content = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let text: Vec<_> = content.encode_utf16().chain(Some(0)).collect();
        let edit = match CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            w!(""),
            WS_CHILD
                | WS_VISIBLE
                | WS_VSCROLL
                | WS_HSCROLL
                | WINDOW_STYLE(
                    (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | ES_AUTOHSCROLL) as u32,
                ),
            0,
            0,
            1,
            1,
            Some(hwnd),
            None,
            Some(module.into()),
            None,
        ) {
            Ok(edit) => edit,
            Err(error) => {
                let _ = DestroyWindow(hwnd);
                return Err(error);
            }
        };
        SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(text.len())), None);
        if let Err(error) = SetWindowTextW(edit, PCWSTR(text.as_ptr())) {
            let _ = DestroyWindow(hwnd);
            return Err(error);
        }
        let font = CreateFontW(
            -((16 * GetDpiForWindow(hwnd).max(96) / 96) as i32),
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
        );
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            Box::into_raw(Box::new(State {
                edit,
                font,
                brush: CreateSolidBrush(color(0xffffff)),
            })) as isize,
        );
        WINDOWS.with_borrow_mut(|windows| windows.push(hwnd));
        refresh(hwnd);
        let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        let _ = MoveWindow(edit, 0, 0, rect.right, rect.bottom, true);
        if visible {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetFocus(Some(edit));
        }
        Ok(())
    }
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: userdata is installed once and reclaimed only on NCDESTROY. Resize uses a copied child HWND.
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        match msg {
            WM_THEMECHANGED | WM_SETTINGCHANGE if !p.is_null() => {
                refresh(hwnd);
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT if !p.is_null() => {
                let dc = HDC(wp.0 as *mut _);
                let dark = super::selection_render::system_dark();
                SetTextColor(dc, color(if dark { 0xf2f2f2 } else { 0x202020 }));
                SetBkColor(dc, color(if dark { 0x252525 } else { 0xffffff }));
                LRESULT((*p).brush.0 as isize)
            }
            WM_SIZE if !p.is_null() => {
                let edit = (*p).edit;
                let _ = MoveWindow(
                    edit,
                    0,
                    0,
                    (lp.0 & 65535) as i32,
                    ((lp.0 >> 16) & 65535) as i32,
                    true,
                );
                LRESULT(0)
            }
            WM_NCDESTROY if !p.is_null() => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                WINDOWS.with_borrow_mut(|windows| windows.retain(|w| *w != hwnd));
                let state = Box::from_raw(p);
                let _ = DeleteObject(state.brush.into());
                let _ = DeleteObject(state.font.into());
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_KEYDOWN if wp.0 == usize::from(VK_ESCAPE.0) => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

pub fn theme_changed() {
    let windows = WINDOWS.with_borrow(Clone::clone);
    for hwnd in windows {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_THEMECHANGED, WPARAM(0), LPARAM(0));
        }
    }
}
fn color(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 255) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 255))
}
fn refresh(hwnd: HWND) {
    // SAFETY: live UI-owned window; resource replacement ends before invalidation can schedule repaint.
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if p.is_null() {
            return;
        }
        let dark = super::selection_render::system_dark();
        let brush = CreateSolidBrush(color(if dark { 0x252525 } else { 0xffffff }));
        let old = std::mem::replace(&mut (*p).brush, brush);
        let _ = DeleteObject(old.into());
        let caption = color(if dark { 0x202020 } else { 0xf3f3f3 });
        let text = color(if dark { 0xf2f2f2 } else { 0x202020 });
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption as *const _ as *const _,
            4,
        );
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, &text as *const _ as *const _, 4);
        let _ = InvalidateRect(Some((*p).edit), None, true);
    }
}
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // SAFETY: no borrowed application state; Escape is forwarded as a value-only close request to this edit's owner.
    unsafe {
        if msg == WM_KEYDOWN && wp.0 == usize::from(VK_ESCAPE.0) {
            if let Ok(owner) = GetParent(hwnd) {
                let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            return LRESULT(0);
        }
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), 1);
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_bundled_licenses_load_in_owned_readonly_edit() {
        // SAFETY: the hidden parent, owned viewer and child edit are created and destroyed exclusively by this test thread.
        unsafe {
            let owner = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("license test"),
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
            let text = include_str!("../../../resources/THIRD_PARTY_LICENSES.txt");
            open_impl(owner, "license test", text, false).unwrap();
            let hwnd = WINDOWS.with_borrow(|windows| *windows.last().unwrap());
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
            let expected = text.replace("\r\n", "\n").replace('\n', "\r\n");
            assert_eq!(
                GetWindowTextLengthW((*p).edit) as usize,
                expected.encode_utf16().count()
            );
            let mut actual = vec![0u16; expected.encode_utf16().count() + 1];
            GetWindowTextW((*p).edit, &mut actual);
            assert_eq!(
                String::from_utf16_lossy(&actual[..actual.len() - 1]),
                expected
            );
            assert_ne!(GetWindowLongW((*p).edit, GWL_STYLE) & ES_READONLY, 0);
            DestroyWindow(owner).unwrap();
            assert!(WINDOWS.with_borrow(Vec::is_empty));
        }
    }
}
