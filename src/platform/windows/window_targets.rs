use crate::core::geometry::PixelRect;
use windows::{
    Win32::{Foundation::*, Graphics::Dwm::*, UI::WindowsAndMessaging::*},
    core::{BOOL, Result},
};

pub fn enumerate() -> Result<Vec<PixelRect>> {
    let mut handles = Vec::<HWND>::new();
    // SAFETY: EnumWindows is synchronous; callback only appends handles to the caller-owned vector.
    unsafe {
        EnumWindows(
            Some(collect),
            LPARAM((&mut handles as *mut Vec<HWND>) as isize),
        )?;
    }
    let mut rects = Vec::new();
    for hwnd in handles {
        // SAFETY: queried handles may disappear; each API validates them and failures are skipped.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                continue;
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4)
                .is_ok()
                && cloaked != 0
            {
                continue;
            }
            let mut r = RECT::default();
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                (&mut r as *mut RECT).cast(),
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
                && GetWindowRect(hwnd, &mut r).is_err()
            {
                continue;
            }
            let rect = PixelRect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            };
            if rect.size().is_some() {
                rects.push(rect);
            }
        }
    }
    Ok(rects)
}
unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: pointer came from enumerate and remains exclusively borrowed during this synchronous call.
    unsafe {
        let handles = &mut *(data.0 as *mut Vec<HWND>);
        if handles.try_reserve(1).is_err() {
            return FALSE;
        }
        handles.push(hwnd);
    }
    TRUE
}
