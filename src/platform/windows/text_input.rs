//! Windowless Windows Rich Edit. The capture canvas owns all visible pixels.
use crate::core::{annotation::Style, geometry::PixelRect, selection::PixelPoint};
use std::{cell::Cell, ffi::c_void, ptr::null_mut, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Direct2D::*, DirectWrite::*, Gdi::*},
        System::{Com::DVASPECT_CONTENT, LibraryLoader::*},
        UI::{
            Controls::{EM_SETSEL, RichEdit::*},
            Input::{Ime::*, KeyboardAndMouse::*},
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, BSTR, Error, GUID, IUnknown, Interface, PCWSTR, Result, s, w},
};

const LIMIT: u32 = 16384;
const TIMER_PREFIX: usize = 0x20000;
const CARET_TIMER: usize = 0x10001;

struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: references to DLL code are dropped before this owned module reference.
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

#[derive(Default)]
struct View {
    rect: Cell<RECT>,
    caret: Cell<RECT>,
    caret_visible: Cell<bool>,
    caret_on: Cell<bool>,
    mouse_captured: Cell<bool>,
}

#[windows::core::implement(ITextHost)]
struct Host {
    hwnd: HWND,
    view: Rc<View>,
    format: CHARFORMATW,
    paragraph: PARAFORMAT,
}

fn write_out<T>(dest: *mut T, value: T) -> Result<()> {
    if dest.is_null() {
        return Err(Error::from_hresult(E_POINTER));
    }
    // SAFETY: Rich Edit's documented host callbacks supply aligned, writable out parameters.
    unsafe {
        dest.write(value);
    }
    Ok(())
}

impl Host {
    fn repaint(&self) {
        // SAFETY: on the window thread; invalidation queues a later paint.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }
}

impl ITextHost_Impl for Host_Impl {
    fn TxGetDC(&self) -> HDC {
        // SAFETY: Rich Edit calls TxReleaseDC for this borrowed window DC.
        unsafe { GetDC(Some(self.hwnd)) }
    }
    fn TxReleaseDC(&self, dc: HDC) -> i32 {
        // SAFETY: DC comes from TxGetDC on the same thread.
        unsafe { ReleaseDC(Some(self.hwnd), dc) }
    }
    fn TxShowScrollBar(&self, _: i32, _: BOOL) -> BOOL {
        BOOL(0)
    }
    fn TxEnableScrollBar(&self, _: SCROLLBAR_CONSTANTS, _: i32) -> BOOL {
        BOOL(0)
    }
    fn TxSetScrollRange(&self, _: i32, _: i32, _: i32, _: BOOL) -> BOOL {
        BOOL(0)
    }
    fn TxSetScrollPos(&self, _: i32, _: i32, _: BOOL) -> BOOL {
        BOOL(0)
    }
    fn TxInvalidateRect(&self, _: *mut RECT, _: BOOL) {
        self.repaint();
    }
    fn TxViewChange(&self, _: BOOL) {
        self.repaint();
    }
    fn TxCreateCaret(&self, _: HBITMAP, width: i32, height: i32) -> BOOL {
        let mut r = self.view.caret.get();
        r.right = r.left.saturating_add(width.max(1));
        r.bottom = r.top.saturating_add(height.max(1));
        self.view.caret.set(r);
        BOOL(1)
    }
    fn TxShowCaret(&self, show: BOOL) -> BOOL {
        self.view.caret_visible.set(show.as_bool());
        self.view.caret_on.set(show.as_bool());
        // SAFETY: timer belongs to this window, with no callback function.
        unsafe {
            if show.as_bool() {
                let blink = GetCaretBlinkTime();
                if blink != 0 && blink != u32::MAX {
                    SetTimer(Some(self.hwnd), CARET_TIMER, blink, None);
                }
            } else {
                let _ = KillTimer(Some(self.hwnd), CARET_TIMER);
            }
        }
        self.repaint();
        BOOL(1)
    }
    fn TxSetCaretPos(&self, x: i32, y: i32) -> BOOL {
        let r = self.view.caret.get();
        self.view.caret.set(RECT {
            left: x,
            top: y,
            right: x.saturating_add(r.right.saturating_sub(r.left)),
            bottom: y.saturating_add(r.bottom.saturating_sub(r.top)),
        });
        self.view.caret_on.set(true);
        self.repaint();
        BOOL(1)
    }
    fn TxSetTimer(&self, id: u32, ms: u32) -> BOOL {
        // SAFETY: timer messages go to our window thread; reserved range isolates Rich Edit IDs.
        BOOL::from(unsafe { SetTimer(Some(self.hwnd), TIMER_PREFIX + id as usize, ms, None) } != 0)
    }
    fn TxKillTimer(&self, id: u32) {
        // SAFETY: this is the same HWND and timer ID used in TxSetTimer.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_PREFIX + id as usize);
        }
    }
    fn TxScrollWindowEx(
        &self,
        _: i32,
        _: i32,
        _: *mut RECT,
        _: *mut RECT,
        _: HRGN,
        _: *mut RECT,
        _: SCROLL_WINDOW_FLAGS,
    ) {
        self.repaint();
    }
    fn TxSetCapture(&self, capture: BOOL) {
        // Update our flag before Windows sends a synchronous capture-change message.
        self.view.mouse_captured.set(capture.as_bool());
        // SAFETY: this UI thread owns the capture window.
        unsafe {
            if capture.as_bool() {
                SetCapture(self.hwnd);
            } else if GetCapture() == self.hwnd {
                let _ = ReleaseCapture();
            }
        }
    }
    fn TxSetFocus(&self) {}
    fn TxSetCursor(&self, cursor: HCURSOR, _: BOOL) {
        // SAFETY: Rich Edit owns the cursor for this call.
        unsafe {
            SetCursor(Some(cursor));
        }
    }
    fn TxScreenToClient(&self, p: *mut POINT) -> BOOL {
        // SAFETY: Rich Edit supplies a mutable POINT for the duration of the callback.
        unsafe { ScreenToClient(self.hwnd, p) }
    }
    fn TxClientToScreen(&self, p: *mut POINT) -> BOOL {
        // SAFETY: Rich Edit supplies a mutable POINT for the duration of the callback.
        unsafe { ClientToScreen(self.hwnd, p) }
    }
    fn TxActivate(&self, old: *mut i32) -> Result<()> {
        write_out(old, 0)
    }
    fn TxDeactivate(&self, _: i32) -> Result<()> {
        Ok(())
    }
    fn TxGetClientRect(&self, p: *mut RECT) -> Result<()> {
        write_out(p, self.view.rect.get())
    }
    fn TxGetViewInset(&self, p: *mut RECT) -> Result<()> {
        write_out(p, RECT::default())
    }
    fn TxGetCharFormat(&self, p: *const *const CHARFORMATW) -> Result<()> {
        write_out(p.cast_mut(), &raw const self.format)
    }
    fn TxGetParaFormat(&self, p: *const *const PARAFORMAT) -> Result<()> {
        write_out(p.cast_mut(), &raw const self.paragraph)
    }
    fn TxGetSysColor(&self, index: SYS_COLOR_INDEX) -> COLORREF {
        // SAFETY: GetSysColor accepts the system index supplied by Rich Edit.
        COLORREF(unsafe { GetSysColor(index) })
    }
    fn TxGetBackStyle(&self, p: *mut TXTBACKSTYLE) -> Result<()> {
        write_out(p, TXTBACK_TRANSPARENT)
    }
    fn TxGetMaxLength(&self, p: *mut u32) -> Result<()> {
        write_out(p, LIMIT)
    }
    fn TxGetScrollBars(&self, p: *mut u32) -> Result<()> {
        write_out(p, ES_AUTOVSCROLL as u32)
    }
    fn TxGetPasswordChar(&self) -> Result<i8> {
        Ok(0)
    }
    fn TxGetAcceleratorPos(&self, p: *mut i32) -> Result<()> {
        write_out(p, -1)
    }
    fn TxGetExtent(&self, _: *mut SIZE) -> Result<()> {
        Err(Error::from_hresult(E_NOTIMPL))
    }
    fn OnTxCharFormatChange(&self, _: *const CHARFORMATW) -> Result<()> {
        Ok(())
    }
    fn OnTxParaFormatChange(&self, _: *const PARAFORMAT) -> Result<()> {
        Ok(())
    }
    fn TxGetPropertyBits(&self, mask: u32, p: *mut u32) -> Result<()> {
        write_out(
            p,
            mask & (TXTBIT_MULTILINE | TXTBIT_WORDWRAP | TXTBIT_DISABLEDRAG | TXTBIT_SAVESELECTION),
        )
    }
    fn TxNotify(&self, _: u32, _: *mut c_void) -> Result<()> {
        self.repaint();
        Ok(())
    }
    fn TxImmGetContext(&self) -> HIMC {
        // SAFETY: paired with TxImmReleaseContext on the window thread.
        unsafe { ImmGetContext(self.hwnd) }
    }
    fn TxImmReleaseContext(&self, context: HIMC) {
        // SAFETY: the context is obtained from this HWND in TxImmGetContext.
        unsafe {
            let _ = ImmReleaseContext(self.hwnd, context);
        }
    }
    fn TxGetSelectionBarWidth(&self, p: *mut i32) -> Result<()> {
        write_out(p, 0)
    }
}

pub struct TextInput {
    service: ITextServices,
    dwrite: IDWriteFactory,
    _host: ITextHost,
    _library: Library,
    hwnd: HWND,
    view: Rc<View>,
    composing: bool,
    desktop: PixelRect,
    area: PixelRect,
    drag: Option<(PixelPoint, PixelPoint)>,
    pub index: Option<usize>,
    pub origin: PixelPoint,
    pub size: f32,
    pub wrap_width: f32,
    pub style: Style,
    pub angle: f64,
}

pub struct TextStart<'a> {
    pub index: Option<usize>,
    pub origin: PixelPoint,
    pub desktop: PixelRect,
    pub area: PixelRect,
    pub content: &'a str,
    pub size: f32,
    pub wrap_width: f32,
    pub style: Style,
    pub angle: f64,
}

impl TextInput {
    pub fn open(hwnd: HWND, start: TextStart<'_>) -> Result<Self> {
        let [r, g, b] = start.style.color;
        let mut format = CHARFORMATW {
            cbSize: size_of::<CHARFORMATW>() as u32,
            dwMask: CFM_SIZE | CFM_COLOR | CFM_FACE | CFM_CHARSET,
            yHeight: (start.size * 15.).round() as i32,
            crTextColor: COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)),
            bCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        for (dst, unit) in format.szFaceName.iter_mut().zip("Segoe UI".encode_utf16()) {
            *dst = unit;
        }
        let view = Rc::new(View::default());
        // SAFETY: load the OS copy. Exported function and IID have the documented TextServ.h ABI.
        // The host and service are confined to this window thread and drop before the module.
        unsafe {
            let dc = GetDC(Some(hwnd));
            if !dc.is_invalid() {
                let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY);
                let _ = ReleaseDC(Some(hwnd), dc);
                if dpi > 0 {
                    format.yHeight = (start.size * 1440. / dpi as f32).round() as i32;
                }
            }
            let dwrite = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let library = Library(LoadLibraryExW(
                w!("Msftedit.dll"),
                None,
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )?);
            let create = GetProcAddress(library.0, s!("CreateTextServices"))
                .ok_or_else(Error::from_win32)?;
            let iid =
                GetProcAddress(library.0, s!("IID_ITextServices")).ok_or_else(Error::from_win32)?;
            let create: PCreateTextServices = std::mem::transmute(Some(create));
            let host: ITextHost = Host {
                hwnd,
                view: view.clone(),
                format,
                paragraph: PARAFORMAT {
                    cbSize: size_of::<PARAFORMAT>() as u32,
                    ..Default::default()
                },
            }
            .into();
            let mut unknown: Option<IUnknown> = None;
            create.ok_or_else(Error::from_win32)?(
                (&None).into(),
                (&Some(host.clone())).into(),
                (&mut unknown).into(),
            )
            .ok()?;
            let unknown = unknown.ok_or_else(|| Error::from_hresult(E_NOINTERFACE))?;
            // windows-rs has a placeholder GUID for this interface. The DLL exports the real IID.
            let mut raw = null_mut();
            unknown.query(iid as *const GUID, &mut raw).ok()?;
            let service = ITextServices::from_raw(raw);
            drop(unknown);
            let _ = ImmAssociateContextEx(hwnd, HIMC::default(), IACE_DEFAULT);
            let mut input = Self {
                service,
                dwrite,
                _host: host,
                _library: library,
                hwnd,
                view,
                composing: false,
                desktop: start.desktop,
                area: start.area,
                drag: None,
                index: start.index,
                origin: start.origin,
                size: start.size,
                wrap_width: start.wrap_width,
                style: start.style,
                angle: start.angle,
            };
            input.send(EM_SETTEXTMODE, WPARAM(TM_PLAINTEXT.0 as usize), LPARAM(0))?;
            input.send(EM_EXLIMITTEXT, WPARAM(0), LPARAM(LIMIT as isize))?;
            let wide: Vec<_> = start
                .content
                .replace('\n', "\r")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            input.service.TxSetText(PCWSTR(wide.as_ptr()))?;
            input.resize()?;
            let rect = input.view.rect.get();
            input.service.OnTxInPlaceActivate(&mut { rect })?;
            input.service.OnTxUIActivate()?;
            input.send(WM_SETFOCUS, WPARAM(0), LPARAM(0))?;
            if start.index.is_some() {
                input.send(EM_SETSEL, WPARAM(0), LPARAM(-1))?;
            }
            Ok(input)
        }
    }
    fn send(&self, msg: u32, wp: WPARAM, lp: LPARAM) -> Result<LRESULT> {
        let mut result = LRESULT(0);
        // SAFETY: active thread-local service. Any message data is live until the synchronous call returns.
        unsafe {
            self.service.TxSendMessage(msg, wp, lp, &mut result)?;
        }
        Ok(result)
    }
    pub fn resize(&mut self) -> Result<()> {
        let content = self.content()?;
        let metrics = super::annotation_render::text_metrics(
            &self.dwrite,
            &content,
            self.size,
            self.wrap_width,
        )?;
        let w = metrics.widthIncludingTrailingWhitespace.ceil() as i32;
        let h = metrics.height.ceil() as i32;
        let left = self.origin.x.saturating_sub(self.desktop.left);
        let top = self.origin.y.saturating_sub(self.desktop.top);
        let rect = RECT {
            left,
            top,
            right: left.saturating_add(
                w.saturating_add(4)
                    .clamp(6, (self.wrap_width.ceil() as i32).max(6)),
            ),
            bottom: top.saturating_add(
                h.max((self.size * 1.25).ceil() as i32)
                    .min(self.area.bottom.saturating_sub(self.origin.y).max(1)),
            ),
        };
        if self.view.rect.replace(rect) != rect {
            // SAFETY: the host rectangle has been updated and will be read synchronously.
            unsafe {
                self.service
                    .OnTxPropertyBitsChange(TXTBIT_CLIENTRECTCHANGE, TXTBIT_CLIENTRECTCHANGE)?;
            }
        }
        Ok(())
    }
    pub fn content(&self) -> Result<String> {
        let mut value = BSTR::new();
        // SAFETY: the service writes an owned BSTR to this out parameter.
        unsafe {
            self.service.TxGetText(&mut value)?;
        }
        Ok(value.to_string().replace("\r\n", "\n").replace('\r', "\n"))
    }
    pub fn composing(&self) -> bool {
        self.composing
    }
    pub fn complete_composition(&self) -> Result<()> {
        if self.composing {
            return Err(Error::new(E_FAIL, "请先确认输入法候选，再完成文字输入"));
        }
        Ok(())
    }
    pub fn message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Result<LRESULT> {
        if msg == WM_TIMER && wp.0 == CARET_TIMER {
            self.view.caret_on.set(!self.view.caret_on.get());
            return Ok(LRESULT(0));
        }
        let wp = if msg == WM_TIMER {
            WPARAM(wp.0.saturating_sub(TIMER_PREFIX))
        } else {
            wp
        };
        if msg == WM_IME_STARTCOMPOSITION {
            self.composing = true;
        }
        if msg == WM_IME_ENDCOMPOSITION {
            self.composing = false;
        }
        let result = self.send(msg, wp, lp)?;
        if matches!(
            msg,
            WM_CHAR
                | WM_KEYDOWN
                | WM_IME_COMPOSITION
                | WM_IME_ENDCOMPOSITION
                | WM_PASTE
                | WM_CUT
                | WM_CLEAR
                | WM_UNDO
        ) {
            self.resize()?;
        }
        Ok(result)
    }
    pub fn owns_timer(id: usize) -> bool {
        id == CARET_TIMER || id >= TIMER_PREFIX
    }
    pub fn bounds(&self) -> RECT {
        let r = self.view.rect.get();
        RECT {
            left: r.left.saturating_sub(3),
            top: r.top.saturating_sub(3),
            right: r.right.saturating_add(3),
            bottom: r.bottom.saturating_add(3),
        }
    }
    pub fn mouse_captured(&self) -> bool {
        self.view.mouse_captured.get() || self.drag.is_some()
    }
    pub fn mouse(&mut self, msg: u32, wp: WPARAM, p: PixelPoint) -> Result<bool> {
        let x = p.x.saturating_sub(self.desktop.left);
        let y = p.y.saturating_sub(self.desktop.top);
        let outer = self.bounds();
        let inside = |r: RECT| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        if let Some((start, origin)) = self.drag {
            if msg == WM_MOUSEMOVE || msg == WM_LBUTTONUP {
                self.origin.x = (i64::from(origin.x) + i64::from(p.x) - i64::from(start.x))
                    .clamp(i64::from(self.area.left), i64::from(self.area.right - 1))
                    as i32;
                self.origin.y = (i64::from(origin.y) + i64::from(p.y) - i64::from(start.y))
                    .clamp(i64::from(self.area.top), i64::from(self.area.bottom - 1))
                    as i32;
                self.resize()?;
                if msg == WM_LBUTTONUP {
                    self.drag = None;
                    // SAFETY: paired with SetCapture for this UI-thread drag.
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
            }
            return Ok(true);
        }
        if !inside(outer) && !self.view.mouse_captured.get() {
            return Ok(false);
        }
        if msg == WM_LBUTTONDOWN && !inside(self.view.rect.get()) {
            self.drag = Some((p, self.origin));
            // SAFETY: capture routes this drag to the live HWND on the same thread.
            unsafe {
                SetCapture(self.hwnd);
            }
        } else {
            let packed = u32::from(x as u16) | (u32::from(y as u16) << 16);
            self.send(msg, wp, LPARAM(packed as isize))?;
        }
        Ok(true)
    }
    pub fn capture_lost(&mut self) {
        if let Some((_, origin)) = self.drag.take() {
            self.origin = origin;
            let _ = self.resize();
        }
        self.view.mouse_captured.set(false);
    }
    pub fn draw(&self, interop: &ID2D1GdiInteropRenderTarget, clip: RECT) -> Result<()> {
        let mut r = self.view.rect.get();
        let mut bounds = RECTL {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };
        // SAFETY: draw into the borrowed GDI DC during BeginDraw. Always release before the next D2D call.
        unsafe {
            let dc = interop.GetDC(D2D1_DC_INITIALIZE_MODE_COPY)?;
            let saved = SaveDC(dc);
            let result = if saved != 0 {
                IntersectClipRect(dc, clip.left, clip.top, clip.right, clip.bottom);
                let result = self.service.TxDraw(
                    DVASPECT_CONTENT,
                    0,
                    null_mut(),
                    null_mut(),
                    dc,
                    HDC::default(),
                    &mut bounds,
                    null_mut(),
                    &mut r,
                    0,
                    0,
                    0,
                );
                if self.view.caret_visible.get()
                    && self.view.caret_on.get()
                    && GetFocus() == self.hwnd
                {
                    let mut caret = self.view.caret.get();
                    caret.right = caret.left.saturating_add(2);
                    let _ = InvertRect(dc, &caret);
                }
                let _ = RestoreDC(dc, saved);
                result
            } else {
                Err(Error::from_win32())
            };
            let release = interop.ReleaseDC(None);
            result.and(release)
        }
    }
}

impl Drop for TextInput {
    fn drop(&mut self) {
        // SAFETY: UI thread owns focus, IME and the service. Deactivate before destroying host and module.
        unsafe {
            let _ = self.service.OnTxUIDeactivate();
            let _ = self.service.OnTxInPlaceDeactivate();
            let _ = KillTimer(Some(self.hwnd), CARET_TIMER);
            let _ = ImmAssociateContextEx(self.hwnd, HIMC::default(), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_windowless_text_grows_shrinks_and_keeps_multiline_content() {
        // SAFETY: test uses only its own hidden window and creates/destroys it on the same thread.
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Text test"),
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
            let area = PixelRect {
                left: 0,
                top: 0,
                right: 640,
                bottom: 480,
            };
            let mut input = TextInput::open(
                hwnd,
                TextStart {
                    index: None,
                    origin: PixelPoint { x: 40, y: 40 },
                    desktop: area,
                    area,
                    content: "",
                    size: 24.,
                    wrap_width: 400.,
                    style: Style::default(),
                    angle: 0.,
                },
            )
            .unwrap();
            let empty = input.bounds();
            for c in "透明文字".encode_utf16() {
                input
                    .message(WM_CHAR, WPARAM(c as usize), LPARAM(1))
                    .unwrap();
            }
            let line = input.bounds();
            assert_eq!(input.content().unwrap(), "透明文字");
            assert!(
                line.right > empty.right && (line.bottom - empty.bottom).abs() <= 2,
                "{empty:?} {line:?}"
            );
            input.message(WM_KEYDOWN, WPARAM(13), LPARAM(1)).unwrap();
            input.message(WM_CHAR, WPARAM(13), LPARAM(1)).unwrap();
            input
                .message(WM_CHAR, WPARAM('F' as usize), LPARAM(1))
                .unwrap();
            assert_eq!(input.content().unwrap(), "透明文字\nF");
            assert!(input.bounds().bottom > line.bottom);
            input.send(EM_SETSEL, WPARAM(0), LPARAM(-1)).unwrap();
            input.message(WM_CLEAR, WPARAM(0), LPARAM(0)).unwrap();
            assert_eq!(input.bounds(), empty);
            input.message(WM_UNDO, WPARAM(0), LPARAM(0)).unwrap();
            assert_eq!(input.content().unwrap(), "透明文字\nF");
            drop(input);
            DestroyWindow(hwnd).unwrap();
        }
    }
}
