//! Native multiline EDIT owns IME and text-local shortcuts on the UI thread.
use crate::core::{annotation::Style, geometry::PixelRect};
use std::{cell::Cell, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_GETRECT, EM_SETLIMITTEXT, EM_SETRECTNP, EM_SETSEL},
            Input::{Ime::*, KeyboardAndMouse::*},
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::{Error, Result, w},
};
pub const COMMIT: u32 = WM_APP + 40;
pub const CANCEL: u32 = WM_APP + 41;
pub const SAVE: u32 = WM_APP + 42;
pub const PIN: u32 = WM_APP + 43;
struct TextFlags {
    composing: Cell<bool>,
    suppress_return: Cell<bool>,
}
struct Subclass {
    parent: HWND,
    flags: Rc<TextFlags>,
    alive: Rc<Cell<Option<HWND>>>,
}
pub struct TextInput {
    flags: Rc<TextFlags>,
    alive: Rc<Cell<Option<HWND>>>,
    font: HFONT,
    pub background: COLORREF,
    pub brush: HBRUSH,
    pub index: Option<usize>,
    pub origin: PixelRect,
    pub size: f32,
    pub wrap_width: f32,
    pub style: Style,
}
impl TextInput {
    pub fn hwnd(&self) -> Option<HWND> {
        self.alive.get()
    }
    pub fn open(
        parent: HWND,
        index: Option<usize>,
        bounds: PixelRect,
        desktop: PixelRect,
        content: &str,
        size: f32,
        style: Style,
    ) -> Result<Self> {
        // SAFETY: UI thread owns parent, child and font; all strings and descriptors are call scoped.
        unsafe {
            let (width, height) = bounds
                .size()
                .ok_or_else(|| Error::new(E_FAIL, "文字输入范围无效"))?;
            let wide: Vec<u16> = content
                .replace('\n', "\r\n")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let hwnd = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                windows::core::PCWSTR(wide.as_ptr()),
                WS_CHILD
                    | WS_VISIBLE
                    | WINDOW_STYLE(
                        ES_MULTILINE as u32 | ES_WANTRETURN as u32 | ES_AUTOVSCROLL as u32,
                    ),
                bounds.left - desktop.left,
                bounds.top - desktop.top,
                width as i32,
                height as i32,
                Some(parent),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            let alive = Rc::new(Cell::new(Some(hwnd)));
            let font = CreateFontW(
                -(size.round() as i32),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            );
            let flags = Rc::new(TextFlags {
                composing: Cell::new(false),
                suppress_return: Cell::new(false),
            });
            let mut input = Self {
                flags: flags.clone(),
                alive: alive.clone(),
                font,
                background: COLORREF(0xFFFFFF),
                brush: CreateSolidBrush(COLORREF(0xFFFFFF)),
                index,
                origin: bounds,
                size,
                wrap_width: width as f32,
                style,
            };
            if font.is_invalid() || input.brush.is_invalid() {
                return Err(Error::new(E_FAIL, "无法创建文字字体"));
            }
            let data = Box::into_raw(Box::new(Subclass {
                parent,
                flags,
                alive,
            }));
            if !SetWindowSubclass(hwnd, Some(subclass), 1, data as usize).as_bool() {
                drop(Box::from_raw(data));
                return Err(Error::new(E_FAIL, "无法创建文字输入处理器"));
            }
            SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
            SendMessageW(hwnd, EM_SETLIMITTEXT, Some(WPARAM(16384)), Some(LPARAM(0)));
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            input.set_wrap_width(None);
            let _ = SetFocus(Some(hwnd));
            Ok(input)
        }
    }
    pub fn set_theme(&mut self, dark: bool) -> Result<()> {
        let background = if dark {
            COLORREF(0x352F2C)
        } else {
            COLORREF(0xFFFFFF)
        };
        // SAFETY: this input exclusively owns its brush, replaced only on the UI thread; invalidation is asynchronous.
        unsafe {
            let brush = CreateSolidBrush(background);
            if brush.is_invalid() {
                return Err(Error::new(E_FAIL, "无法创建文字输入背景"));
            }
            let old = std::mem::replace(&mut self.brush, brush);
            self.background = background;
            if !old.is_invalid() {
                let _ = DeleteObject(old.into());
            }
            if let Some(hwnd) = self.hwnd() {
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
        }
        Ok(())
    }
    pub fn set_wrap_width(&mut self, desired: Option<f32>) {
        if let Some(hwnd) = self.hwnd() {
            // SAFETY: owned UI-thread EDIT with call-scoped formatting rectangle; setting it preserves native wrapping width across reediting.
            unsafe {
                let mut rect = RECT::default();
                SendMessageW(
                    hwnd,
                    EM_GETRECT,
                    None,
                    Some(LPARAM((&mut rect as *mut RECT) as isize)),
                );
                if let Some(width) = desired {
                    rect.right = (rect.left + width.ceil() as i32).min(rect.right);
                    SendMessageW(
                        hwnd,
                        EM_SETRECTNP,
                        None,
                        Some(LPARAM((&rect as *const RECT) as isize)),
                    );
                }
                self.wrap_width = (rect.right - rect.left).max(1) as f32;
            }
        }
    }
    pub fn complete_composition(&self) -> Result<()> {
        let hwnd = self
            .hwnd()
            .ok_or_else(|| Error::new(E_FAIL, "文字输入框已关闭"))?;
        // SAFETY: only this UI thread accesses subclass state. No state borrow is held while IME may synchronously send messages.
        unsafe {
            let composing = self.flags.composing.get();
            if composing {
                let context = ImmGetContext(hwnd);
                if context.is_invalid() {
                    return Err(Error::new(E_FAIL, "请先确认输入法候选，再完成文字输入"));
                }
                let completed = ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_COMPLETE, 0).as_bool();
                let _ = ImmReleaseContext(hwnd, context);
                if !completed || self.flags.composing.get() {
                    return Err(Error::new(E_FAIL, "请先确认输入法候选，再完成文字输入"));
                }
            }
        }
        Ok(())
    }
    pub fn content(&self) -> Result<String> {
        let hwnd = self
            .hwnd()
            .ok_or_else(|| Error::new(E_FAIL, "文字输入框已关闭"))?;
        // SAFETY: owned live EDIT and bounded writable UTF-16 buffer.
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if !(0..=16384).contains(&len) {
                return Err(Error::new(E_FAIL, "文字超过 16384 个字符上限"));
            }
            let mut buffer = vec![0u16; len as usize + 1];
            let read = GetWindowTextW(hwnd, &mut buffer);
            Ok(String::from_utf16_lossy(&buffer[..read as usize]).replace("\r\n", "\n"))
        }
    }
}
impl Drop for TextInput {
    fn drop(&mut self) {
        // SAFETY: child NCDESTROY clears alive even when parent destroys the child first. Font is deleted after child stops using it.
        unsafe {
            if let Some(hwnd) = self.alive.get() {
                let _ = DestroyWindow(hwnd);
            }
            if !self.brush.is_invalid() {
                let _ = DeleteObject(self.brush.into());
            }
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
        }
    }
}
unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let result = std::panic::catch_unwind(|| {
        // SAFETY: SetWindowSubclass owns this one Box until NCDESTROY; no borrowed state survives DefSubclassProc.
        unsafe {
            if msg == WM_NCDESTROY {
                let state = Box::from_raw(data as *mut Subclass);
                state.alive.set(None);
                let _ = RemoveWindowSubclass(hwnd, Some(subclass), 1);
                return DefSubclassProc(hwnd, msg, wp, lp);
            }
            let state = &mut *(data as *mut Subclass);
            if msg == WM_IME_STARTCOMPOSITION {
                state.flags.composing.set(true);
            }
            if msg == WM_IME_ENDCOMPOSITION {
                state.flags.composing.set(false);
            }
            if msg == WM_KEYDOWN
                && !state.flags.composing.get()
                && wp.0 == 65
                && GetKeyState(VK_CONTROL.0 as i32) < 0
            {
                SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                return LRESULT(0);
            }
            if msg == WM_KEYDOWN && !state.flags.composing.get() {
                let command = if wp.0 == 27 {
                    Some(CANCEL)
                } else if wp.0 == 13 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    Some(COMMIT)
                } else if wp.0 == 83 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    Some(SAVE)
                } else if wp.0 == 80 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    Some(PIN)
                } else {
                    None
                };
                if let Some(command) = command {
                    state.flags.suppress_return.set(command == COMMIT);
                    let parent = state.parent;
                    let _ = PostMessageW(Some(parent), command, WPARAM(hwnd.0 as usize), LPARAM(0));
                    return LRESULT(0);
                }
            }
            if msg == WM_CHAR && state.flags.suppress_return.get() && matches!(wp.0, 10 | 13) {
                state.flags.suppress_return.set(false);
                return LRESULT(0);
            }
            if msg == WM_CHAR
                && (wp.0 == 16
                    || wp.0 == 19
                    || wp.0 == 27
                    || ((wp.0 == 13 || wp.0 == 10)
                        && (state.flags.suppress_return.get()
                            || GetKeyState(VK_CONTROL.0 as i32) < 0)))
            {
                return LRESULT(0);
            }
            DefSubclassProc(hwnd, msg, wp, lp)
        }
    });
    result.unwrap_or(LRESULT(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_edit_roundtrip_ime_cancel_routing_and_parent_teardown() {
        // SAFETY: this test owns hidden parent and child on one thread; messages target only these test windows.
        unsafe {
            let parent = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Text input test"),
                WS_OVERLAPPED,
                0,
                0,
                640,
                480,
                None,
                None,
                Some(GetModuleHandleW(None).unwrap().into()),
                None,
            )
            .unwrap();
            let bounds = PixelRect {
                left: 0,
                top: 0,
                right: 400,
                bottom: 180,
            };
            let input = TextInput::open(
                parent,
                None,
                bounds,
                bounds,
                "中文\n😀第二行",
                30.,
                Style::default(),
            )
            .unwrap();
            let child = input.hwnd().unwrap();
            assert_eq!(input.content().unwrap(), "中文\n😀第二行");
            input.flags.suppress_return.set(true);
            SendMessageW(child, WM_KEYUP, Some(WPARAM(13)), None);
            SendMessageW(child, WM_CHAR, Some(WPARAM(10)), None);
            assert_eq!(input.content().unwrap(), "中文\n😀第二行");
            SendMessageW(child, WM_IME_STARTCOMPOSITION, None, None);
            SendMessageW(child, WM_KEYDOWN, Some(WPARAM(27)), None);
            let mut message = MSG::default();
            assert!(!PeekMessageW(&mut message, Some(parent), CANCEL, CANCEL, PM_REMOVE).as_bool());
            SendMessageW(child, WM_IME_ENDCOMPOSITION, None, None);
            SendMessageW(child, WM_KEYDOWN, Some(WPARAM(27)), None);
            assert!(PeekMessageW(&mut message, Some(parent), CANCEL, CANCEL, PM_REMOVE).as_bool());
            assert_eq!(message.wParam.0, child.0 as usize);
            DestroyWindow(parent).unwrap();
            assert!(input.hwnd().is_none());
            drop(input);
        }
    }
}
