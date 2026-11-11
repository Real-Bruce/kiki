//! Shared Win32 input and slider mapping for physical-pixel stroke widths.
use crate::core::config::{MAX_STROKE_WIDTH, MIN_STROKE_WIDTH, parse_stroke_width};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::HFONT,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Controls::*, Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, Result, w},
};

pub const EDIT_KEY: u32 = WM_APP + 64;

#[derive(Default)]
pub struct Wheel(i32);

impl Wheel {
    pub fn steps(&mut self, wp: WPARAM) -> i32 {
        let amount = self.0 + i32::from((wp.0 >> 16) as i16);
        self.0 = amount % 120;
        amount / 120
    }
}

pub fn step(width: u8, steps: i32) -> u8 {
    (i32::from(width) + steps).clamp(i32::from(MIN_STROKE_WIDTH), i32::from(MAX_STROKE_WIDTH)) as u8
}

pub fn fraction(width: u8) -> f32 {
    f32::from(width - MIN_STROKE_WIDTH) / f32::from(MAX_STROKE_WIDTH - MIN_STROKE_WIDTH)
}

pub fn slider_value(x: f32, left: f32, right: f32) -> u8 {
    MIN_STROKE_WIDTH
        + (((x - left) / (right - left).max(1.)).clamp(0., 1.)
            * f32::from(MAX_STROKE_WIDTH - MIN_STROKE_WIDTH))
        .round() as u8
}

pub fn create_edit(parent: HWND, id: u16, font: HFONT) -> Result<HWND> {
    // SAFETY: the parent and font live on the UI thread and outlive the edit child.
    unsafe {
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_CENTER as u32 | ES_AUTOHSCROLL as u32),
            0,
            0,
            1,
            1,
            Some(parent),
            Some(HMENU(usize::from(id) as *mut _)),
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?;
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
        // Allow ordinary edits and paste, including invalid input that must be rejected on commit.
        SendMessageW(edit, EM_SETLIMITTEXT, Some(WPARAM(16)), None);
        if !SetWindowSubclass(edit, Some(edit_proc), 1, 0).as_bool() {
            let error = windows::core::Error::from_win32();
            let _ = DestroyWindow(edit);
            return Err(error);
        }
        Ok(edit)
    }
}

pub fn set_edit(edit: HWND, width: u8) {
    let text: Vec<u16> = width.to_string().encode_utf16().chain(Some(0)).collect();
    // SAFETY: the caller supplies a live edit on this thread and the string is NUL-terminated.
    unsafe {
        let _ = SetWindowTextW(edit, PCWSTR(text.as_ptr()));
    }
}

pub fn read_edit(edit: HWND) -> std::result::Result<u8, &'static str> {
    let mut text = [0u16; 17];
    // SAFETY: GetWindowTextW writes within the call-scoped buffer.
    let len = unsafe { GetWindowTextW(edit, &mut text) } as usize;
    parse_stroke_width(&String::from_utf16_lossy(&text[..len]))
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: this callback is installed on a live edit. Dialog-code MSG pointers and parent
    // notifications are used synchronously, and no references survive a window call.
    unsafe {
        let commit_key = |key| matches!(key, 9 | 13 | 27);
        match msg {
            WM_GETDLGCODE if lp.0 != 0 && commit_key((*(lp.0 as *const MSG)).wParam.0) => {
                return LRESULT(DLGC_WANTALLKEYS as isize);
            }
            WM_KEYDOWN if commit_key(wp.0) => {
                if let Ok(parent) = GetParent(hwnd) {
                    SendMessageW(parent, EDIT_KEY, Some(wp), Some(LPARAM(hwnd.0 as isize)));
                }
                return LRESULT(0);
            }
            WM_CHAR if commit_key(wp.0) => return LRESULT(0),
            WM_SETFOCUS => {
                let result = DefSubclassProc(hwnd, msg, wp, lp);
                SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                return result;
            }
            WM_MOUSEWHEEL => {
                if let Ok(parent) = GetParent(hwnd) {
                    return SendMessageW(parent, msg, Some(wp), Some(lp));
                }
            }
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), 1);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
