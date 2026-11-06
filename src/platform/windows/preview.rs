//! Native frozen-desktop selection window. Annotation/output controls are staged for P4/P5.
use super::{
    capture::CaptureResult,
    selection_render::{Canvas, PaintInput, system_dark},
    text_input::TextInput,
};
use crate::core::{
    annotation::{Editor, Shape, Tool},
    display::DisplayInfo,
    geometry::PixelRect,
    selection::{PixelPoint, Selection, contains},
};
use std::{cell::Cell, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::{Error, Result, w},
};

pub struct Preview(Rc<Cell<Option<HWND>>>);
impl Preview {
    pub fn is_open(&self) -> bool {
        self.0.get().is_some()
    }
    pub fn open(capture: CaptureResult) -> Result<Self> {
        let bounds = capture.image.bounds();
        let (width, height) = bounds
            .size()
            .ok_or_else(|| Error::new(E_FAIL, "无效截图范围"))?;
        let width = i32::try_from(width).map_err(|_| Error::new(E_FAIL, "截图画布过宽"))?;
        let height = i32::try_from(height).map_err(|_| Error::new(E_FAIL, "截图画布过高"))?;
        // SAFETY: UI thread creates and owns this popup. The window takes the Box only after initialization succeeds.
        unsafe {
            #[cfg(feature = "diagnostics")]
            let accessible = std::env::args_os().any(|a| a == "--accessible-capture");
            #[cfg(not(feature = "diagnostics"))]
            let accessible = false;
            let hwnd = CreateWindowExW(
                if accessible {
                    WS_EX_TOPMOST
                } else {
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW
                },
                w!("kiki.Preview"),
                w!("kiki 截图"),
                WS_POPUP,
                bounds.left,
                bounds.top,
                width,
                height,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            let lifetime = Rc::new(Cell::new(Some(hwnd)));
            let owner = Self(lifetime.clone());
            let canvas = Canvas::new(hwnd, capture.image)?;
            let state = State {
                lifetime,
                canvas,
                selection: Selection::new(bounds),
                editor: Editor::default(),
                parameter_down: false,
                text_input: None,
                output: None,
                busy: false,
                pending_output: None,
                screens: capture.screens,
                windows: capture
                    .windows
                    .into_iter()
                    .filter_map(|r| bounds.intersection(r))
                    .collect(),
                pointer: position().unwrap_or(PixelPoint {
                    x: bounds.left,
                    y: bounds.top,
                }),
                hover: None,
                pressed_window: None,
                down_button: None,
                focus: usize::MAX,
                keyboard_focus: false,
            };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(state)) as isize);
            let _ = ShowWindow(hwnd, SW_SHOW);
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                bounds.left,
                bounds.top,
                width,
                height,
                SWP_SHOWWINDOW,
            )?;
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
            Ok(owner)
        }
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        // SAFETY: NCDESTROY clears the UI-thread lifetime cell before the HWND may be reused.
        unsafe {
            if let Some(hwnd) = self.0.get() {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}
struct State {
    lifetime: Rc<Cell<Option<HWND>>>,
    canvas: Canvas,
    selection: Selection,
    editor: Editor,
    parameter_down: bool,
    text_input: Option<TextInput>,
    output: Option<super::output::Job>,
    busy: bool,
    pending_output: Option<usize>,
    screens: Vec<DisplayInfo>,
    windows: Vec<PixelRect>,
    pointer: PixelPoint,
    hover: Option<PixelRect>,
    pressed_window: Option<PixelRect>,
    down_button: Option<usize>,
    focus: usize,
    keyboard_focus: bool,
}
impl State {
    fn activate(&mut self, index: usize) -> bool {
        if !self.editor.enabled(index) {
            return false;
        }
        match index {
            0..=4 => {
                self.editor.cancel();
                let tool = [
                    Tool::Select,
                    Tool::Rectangle,
                    Tool::Arrow,
                    Tool::Pen,
                    Tool::Text,
                ][index];
                if tool != Tool::Select {
                    let matching = self
                        .editor
                        .selected
                        .and_then(|i| self.editor.objects().get(i))
                        .filter(|o| {
                            matches!(
                                (&o.shape, tool),
                                (Shape::Rectangle(_), Tool::Rectangle)
                                    | (Shape::Arrow(_, _), Tool::Arrow)
                                    | (Shape::Pen(_), Tool::Pen)
                                    | (Shape::Text { .. }, Tool::Text)
                            )
                        })
                        .map(|o| o.style);
                    if let Some(mut style) = matching {
                        style.width /= self.scale();
                        self.editor.styles[index - 1] = style;
                    } else {
                        self.editor.selected = None;
                    }
                }
                self.editor.tool = tool;
                if tool == Tool::Text {
                    if let Some(Shape::Text { size, .. }) = self
                        .editor
                        .selected
                        .and_then(|i| self.editor.objects().get(i))
                        .map(|o| &o.shape)
                    {
                        self.editor.text_size = *size / self.scale();
                    }
                }
            }
            11..=21 => {
                if let Err(message) = self.apply_parameter(index - 11) {
                    self.editor.warn(message);
                }
            }
            8 | 9 => {
                self.pending_output = Some(index);
            }
            5 => self.editor.undo(),
            6 => self.editor.redo(),
            10 => return true,
            _ => {}
        }
        false
    }
    fn apply_parameter(&mut self, parameter: usize) -> std::result::Result<(), &'static str> {
        let mut style = self.editor.style();
        if parameter < 8 {
            style.color = super::selection_render::ANNOTATION_COLORS[parameter];
        } else if parameter < 11 {
            if self.editor.tool == Tool::Text {
                self.editor.text_size = [18., 24., 36.][parameter - 8];
            } else {
                style.width = [1., 3., 6.][parameter - 8];
            }
        } else {
            return Ok(());
        }
        if self.editor.tool == Tool::Text {
            restyle_text(self, style, parameter >= 8)
        } else {
            self.editor.set_style(style, self.scale());
            Ok(())
        }
    }
    fn parameter_at(&self, p: PixelPoint) -> Option<usize> {
        let rect = self.selection.rect?;
        self.canvas.parameter_at(
            &self
                .canvas
                .layout(rect, &self.screens, self.editor.tool != Tool::Select),
            p,
        )
    }
    fn window_at(&self, p: PixelPoint) -> Option<PixelRect> {
        self.windows.iter().copied().find(|r| contains(*r, p))
    }
    fn button_at(&self, p: PixelPoint) -> Option<usize> {
        self.selection
            .rect
            .filter(|_| !self.selection.active())
            .and_then(|r| {
                self.canvas
                    .layout(r, &self.screens, self.editor.tool != Tool::Select)
                    .buttons
                    .iter()
                    .position(|b| contains(*b, p))
            })
    }
    fn scale(&self) -> f32 {
        self.screens
            .iter()
            .find(|s| contains(s.bounds, self.pointer))
            .map_or(1., |s| s.scale_percent as f32 / 100.)
    }
    fn radius(&self) -> i32 {
        self.screens
            .iter()
            .find(|s| contains(s.bounds, self.pointer))
            .map_or(6, |s| (6 * s.scale_percent / 100).max(4))
    }
}
fn position() -> Option<PixelPoint> {
    let mut p = POINT::default(); // SAFETY: call-scoped output pointer; PMv2 returns physical global desktop coordinates.
    unsafe {
        GetCursorPos(&mut p).ok()?;
    }
    Some(PixelPoint { x: p.x, y: p.y })
}
pub fn register(instance: HINSTANCE) -> Result<()> {
    // SAFETY: static class name/callback, system-owned cursor; all visible windows use the main UI thread.
    unsafe {
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hIcon: super::icon::application(instance, false)?,
            lpszClassName: w!("kiki.Preview"),
            hCursor: LoadCursorW(None, IDC_CROSS)?,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(Error::from_win32());
        }
    }
    Ok(())
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(|| dispatch(hwnd, msg, wp, lp)) {
        Ok(value) => value,
        Err(_) => {
            super::show_error("选区操作失败，请取消后重新截图。");
            LRESULT(0)
        }
    }
}
fn dispatch(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: userdata is this window's unique State Box. Each borrowed state is released before destroy/release-capture or error UI can reenter.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if msg == WM_NCDESTROY && !pointer.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let state = Box::from_raw(pointer);
            state.lifetime.set(None);
            drop(state);
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if pointer.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if (*pointer).busy
            && matches!(
                msg,
                WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_KEYDOWN
            )
        {
            return LRESULT(0);
        }
        match msg {
            WM_TIMER if wp.0 == 5 => {
                let result =
                    (*pointer)
                        .output
                        .as_ref()
                        .and_then(|job| match job.receiver.try_recv() {
                            Ok(result) => Some(result),
                            Err(std::sync::mpsc::TryRecvError::Empty) => None,
                            Err(_) => {
                                Some(Err(Error::new(E_FAIL, "输出线程意外退出，编辑内容已保留")))
                            }
                        });
                if let Some(result) = result {
                    let job = (*pointer).output.take();
                    drop(job);
                    (*pointer).busy = false;
                    let _ = KillTimer(Some(hwnd), 5);
                    match result {
                        Ok(()) => {
                            let _ = DestroyWindow(hwnd);
                        }
                        Err(e) => {
                            super::show_error(&format!("输出失败，编辑内容已保留：{e}"));
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                LRESULT(0)
            }
            super::text_input::SAVE => {
                if (*pointer)
                    .text_input
                    .as_ref()
                    .and_then(TextInput::hwnd)
                    .is_some_and(|child| child.0 as usize == wp.0)
                {
                    (*pointer).pending_output = Some(8);
                    if let Err(e) = begin_output(hwnd, pointer) {
                        super::show_error(&e.to_string());
                    }
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let result = {
                    let state = &mut *pointer;
                    state.canvas.paint(PaintInput {
                        selection: &state.selection,
                        editor: &state.editor,
                        editing: state.text_input.as_ref().and_then(|input| input.index),
                        text_active: state.text_input.is_some(),
                        busy: state.busy,
                        hover: state.hover,
                        screens: &state.screens,
                        pointer: state.pointer,
                        focus: if state.keyboard_focus {
                            state.focus
                        } else {
                            usize::MAX
                        },
                        pressed: state.down_button,
                    })
                };
                let _ = EndPaint(hwnd, &ps);
                if let Err(error) = result {
                    super::show_error(&format!("选区绘制失败：{error}"));
                }
                LRESULT(0)
            }
            super::text_input::COMMIT | super::text_input::CANCEL => {
                let current = (*pointer).text_input.as_ref().and_then(TextInput::hwnd);
                if current.is_some_and(|child| child.0 as usize == wp.0) {
                    if msg == super::text_input::COMMIT {
                        if let Err(error) = finish_text(hwnd, pointer) {
                            super::show_error(&error.to_string());
                        }
                    } else {
                        let input = (*pointer).text_input.take();
                        drop(input);
                        let _ = SetFocus(Some(hwnd));
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                (*pointer).pointer = position().unwrap_or((*pointer).pointer);
                if (*pointer).editor.text_at((*pointer).pointer).is_some() {
                    if let Err(error) = start_text(hwnd, pointer) {
                        super::show_error(&error.to_string());
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                if let Some(input) = (*pointer).text_input.as_ref() {
                    let color = input.style.color;
                    let dc = HDC(wp.0 as *mut _);
                    SetTextColor(
                        dc,
                        COLORREF(
                            u32::from(color[0])
                                | (u32::from(color[1]) << 8)
                                | (u32::from(color[2]) << 16),
                        ),
                    );
                    SetBkColor(dc, input.background);
                    return LRESULT(input.brush.0 as isize);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MOUSEMOVE => {
                {
                    let state = &mut *pointer;
                    state.pointer = position().unwrap_or(state.pointer);
                    state.selection.update(state.pointer);
                    state.editor.update(state.pointer);
                    state.hover = state.window_at(state.pointer);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                if let Err(error) = finish_text(hwnd, pointer) {
                    super::show_error(&error.to_string());
                    return LRESULT(0);
                }
                let mut request_text = false;
                let mut error = None;
                {
                    let state = &mut *pointer;
                    state.pointer = position().unwrap_or(state.pointer);
                    state.pressed_window = state.window_at(state.pointer);
                    state.parameter_down = false;
                    state.keyboard_focus = false;
                    if let Some(parameter) = state.parameter_at(state.pointer) {
                        state.parameter_down = true;
                        state.focus = 11 + parameter;
                        if let Err(message) = state.apply_parameter(parameter) {
                            error = Some(message);
                        }
                    } else if let Some(button) = state.button_at(state.pointer) {
                        state.down_button = Some(button);
                        if state.editor.enabled(button) {
                            state.focus = button;
                        }
                    } else {
                        state.focus = usize::MAX;
                        let object = if state
                            .selection
                            .rect
                            .is_some_and(|r| contains(r, state.pointer))
                        {
                            if state.editor.tool == Tool::Text {
                                request_text = true;
                                true
                            } else {
                                match state.editor.begin(
                                    state.pointer,
                                    state.radius(),
                                    state.scale(),
                                ) {
                                    Ok(active) => active,
                                    Err(message) => {
                                        error = Some(message);
                                        true
                                    }
                                }
                            }
                        } else {
                            false
                        };
                        if !object && state.editor.tool == Tool::Select {
                            state.selection.begin(state.pointer, state.radius());
                        }
                    }
                }
                if request_text {
                    if let Err(e) = start_text(hwnd, pointer) {
                        super::show_error(&e.to_string());
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                }
                if let Some(message) = error {
                    super::show_error(message);
                } else {
                    SetCapture(hwnd);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let warning;
                let close = {
                    let state = &mut *pointer;
                    state.pointer = position().unwrap_or(state.pointer);
                    warning = state.editor.take_warning();
                    if let Some(button) = state.down_button.take() {
                        if state.button_at(state.pointer) == Some(button) {
                            state.activate(button)
                        } else {
                            false
                        }
                    } else {
                        if state.editor.active() {
                            state.editor.finish(state.pointer);
                        } else if !state.parameter_down {
                            state.selection.finish(state.pointer, state.pressed_window);
                        }
                        state.parameter_down = false;
                        false
                    }
                };
                let _ = ReleaseCapture();
                if let Some(message) = warning {
                    super::show_error(message);
                }
                if (*pointer).pending_output.is_some() {
                    if let Err(e) = begin_output(hwnd, pointer) {
                        super::show_error(&e.to_string());
                    }
                    return LRESULT(0);
                }
                if close {
                    let _ = DestroyWindow(hwnd);
                } else {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                {
                    let state = &mut *pointer;
                    state.selection.cancel_drag();
                    state.editor.cancel();
                    state.down_button = None;
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_KEYDOWN => {
                if (*pointer).text_input.is_some() {
                    if wp.0 == 27 {
                        let input = (*pointer).text_input.take();
                        drop(input);
                        let _ = SetFocus(Some(hwnd));
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    } else if let Err(error) = finish_text(hwnd, pointer) {
                        super::show_error(&error.to_string());
                        return LRESULT(0);
                    }
                }
                let mut release = false;
                let close = {
                    let state = &mut *pointer;
                    match wp.0 as u32 {
                        27 => {
                            release = true;
                            if state.down_button.take().is_some() {
                                false
                            } else {
                                !state.editor.cancel() && !state.selection.cancel_drag()
                            }
                        }
                        70 => {
                            release = true;
                            state.down_button = None;
                            state.editor.cancel();
                            state.pointer = position().unwrap_or(state.pointer);
                            if let Some(screen) = state
                                .screens
                                .iter()
                                .find(|s| contains(s.bounds, state.pointer))
                            {
                                state.selection.fullscreen(screen.bounds);
                            }
                            false
                        }
                        9 => {
                            state.keyboard_focus = true;
                            let backwards = GetKeyState(VK_SHIFT.0 as i32) < 0;
                            let mut next = if state.focus < 22 {
                                state.focus
                            } else if backwards {
                                0
                            } else {
                                21
                            };
                            for _ in 0..22 {
                                next = if backwards {
                                    (next + 21) % 22
                                } else {
                                    (next + 1) % 22
                                };
                                if state.editor.enabled(next) {
                                    break;
                                }
                            }
                            state.focus = next;
                            false
                        }
                        13 => {
                            if state.keyboard_focus && state.focus < 22 {
                                state.activate(state.focus)
                            } else {
                                state.pending_output = Some(9);
                                false
                            }
                        }
                        32 => state.keyboard_focus && state.activate(state.focus),
                        83 if GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                            state.pending_output = Some(8);
                            false
                        }
                        46 => {
                            state.editor.delete();
                            false
                        }
                        90 if GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                            state.editor.undo();
                            false
                        }
                        89 if GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                            state.editor.redo();
                            false
                        }
                        _ => false,
                    }
                };
                if release {
                    let _ = ReleaseCapture();
                }
                if let Some(message) = (*pointer).editor.take_warning() {
                    super::show_error(message);
                }
                if (*pointer).pending_output.is_some() {
                    if let Err(e) = begin_output(hwnd, pointer) {
                        super::show_error(&e.to_string());
                    }
                    return LRESULT(0);
                }
                if close {
                    let _ = DestroyWindow(hwnd);
                } else {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_SETCURSOR => {
                let state = &*pointer;
                let cursor = if state.button_at(state.pointer).is_some()
                    || state.parameter_at(state.pointer).is_some()
                {
                    IDC_ARROW
                } else if state.editor.tool != Tool::Select {
                    IDC_CROSS
                } else if let Some(handle) =
                    state.selection.handle_at(state.pointer, state.radius())
                {
                    match handle {
                        0 | 4 => IDC_SIZENWSE,
                        2 | 6 => IDC_SIZENESW,
                        1 | 5 => IDC_SIZENS,
                        _ => IDC_SIZEWE,
                    }
                } else if state
                    .selection
                    .rect
                    .is_some_and(|r| contains(r, state.pointer))
                {
                    IDC_SIZEALL
                } else {
                    IDC_CROSS
                };
                if let Ok(cursor) = LoadCursorW(None, cursor) {
                    SetCursor(Some(cursor));
                }
                LRESULT(1)
            }
            WM_SETTINGCHANGE | WM_THEMECHANGED => {
                (*pointer).canvas.dark = system_dark();
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn start_text(hwnd: HWND, pointer: *mut State) -> Result<()> {
    // SAFETY: caller is this live preview's UI-thread dispatch. Native creation occurs after the state borrow ends.
    let (index, content, anchor, edit_bounds, size, style, desktop, wrap) = unsafe {
        let state = &mut *pointer;
        if state.text_input.is_some() {
            return Ok(());
        }
        state.editor.cancel();
        let selection = state
            .selection
            .rect
            .ok_or_else(|| Error::new(E_FAIL, "请先选择截图范围"))?;
        let desktop = state.canvas.bounds();
        if let Some(index) = state.editor.text_at(state.pointer) {
            let object = &state.editor.objects()[index];
            if let Shape::Text {
                bounds,
                content,
                size,
                wrap_width,
            } = &object.shape
            {
                let width = (*wrap_width as i32)
                    .saturating_add(64)
                    .max(32)
                    .min(desktop.right.saturating_sub(bounds.left).max(32));
                let height = (180. * state.scale()).round() as i32;
                let edit_bounds = crate::core::selection::fit_panel(
                    PixelPoint {
                        x: bounds.left,
                        y: bounds.top,
                    },
                    desktop,
                    width,
                    height,
                );
                (
                    Some(index),
                    content.clone(),
                    *bounds,
                    edit_bounds,
                    *size,
                    object.style,
                    desktop,
                    *wrap_width,
                )
            } else {
                return Ok(());
            }
        } else {
            let edit_bounds = crate::core::selection::fit_panel(
                state.pointer,
                selection,
                (480. * state.scale()).round() as i32,
                (180. * state.scale()).round() as i32,
            );
            let mut style = state.editor.styles[3];
            style.width *= state.scale();
            (
                None,
                String::new(),
                edit_bounds,
                edit_bounds,
                state.editor.text_size * state.scale(),
                style,
                desktop,
                (edit_bounds.right - edit_bounds.left) as f32,
            )
        }
    };
    let mut input = TextInput::open(hwnd, index, edit_bounds, desktop, &content, size, style)?;
    input.origin = anchor;
    // SAFETY: synchronous read of the live parent state; theme setup owns only child resources.
    let dark = unsafe { (*pointer).canvas.dark };
    input.set_theme(dark)?;
    if index.is_some() {
        input.set_wrap_width(Some(wrap));
    }
    // SAFETY: parent is retained through synchronous creation; the independent native child owns no State pointer.
    unsafe {
        (*pointer).editor.selected = index;
        (*pointer).text_input = Some(input);
    }
    Ok(())
}
fn finish_text(hwnd: HWND, pointer: *mut State) -> Result<()> {
    // SAFETY: UI thread takes ownership before native calls, so EDIT messages cannot alias a State borrow.
    let input = unsafe { (*pointer).text_input.take() };
    let Some(input) = input else {
        return Ok(());
    };
    let result = (|| {
        input.complete_composition()?;
        let content = input.content()?;
        // SAFETY: synchronous DirectWrite calls do not dispatch window messages; parent State is live.
        unsafe {
            let state = &mut *pointer;
            let object = state.canvas.text_object(
                PixelPoint {
                    x: input.origin.left,
                    y: input.origin.top,
                },
                content,
                input.size,
                input.wrap_width,
                input.style,
            )?;
            state
                .editor
                .commit_text(input.index, object)
                .map_err(|message| Error::new(E_FAIL, message))
        }
    })();
    match result {
        Ok(()) => {
            drop(input); // SAFETY: borrow ended before focus can cause callbacks.
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
            Ok(())
        }
        Err(error) => {
            // SAFETY: restore the still-owned input; failure must retain the user's draft.
            unsafe {
                (*pointer).text_input = Some(input);
            }
            Err(error)
        }
    }
}
fn restyle_text(
    state: &mut State,
    style: crate::core::annotation::Style,
    resize: bool,
) -> std::result::Result<(), &'static str> {
    state.editor.styles[3] = style;
    if let Some(index) = state.editor.selected {
        if let Some(object) = state.editor.objects().get(index) {
            if let Shape::Text {
                bounds,
                content,
                size,
                wrap_width,
            } = &object.shape
            {
                let size = if resize {
                    state.editor.text_size * state.scale()
                } else {
                    *size
                };
                let object = state
                    .canvas
                    .text_object(
                        PixelPoint {
                            x: bounds.left,
                            y: bounds.top,
                        },
                        content.clone(),
                        size,
                        *wrap_width,
                        style,
                    )
                    .map_err(|_| "无法更新文字布局，原文字已保留")?;
                state.editor.commit_text(Some(index), object)?;
            }
        }
    }
    Ok(())
}

fn begin_output(hwnd: HWND, pointer: *mut State) -> Result<()> {
    // SAFETY: UI dispatch owns this live state; no borrow is held across dialog, capture release or native callbacks.
    let (kind, lifetime) = unsafe {
        let state = &mut *pointer;
        if state.busy {
            return Ok(());
        }
        let Some(kind) = state.pending_output.take() else {
            return Ok(());
        };
        (kind, state.lifetime.clone())
    };
    finish_text(hwnd, pointer)?;
    // SAFETY: the state borrow ends before ReleaseCapture can reenter the message pump.
    unsafe {
        {
            let state = &mut *pointer;
            if state.selection.rect.is_none() {
                return Err(Error::new(E_FAIL, "请先选择截图范围"));
            }
            state.editor.finish(state.pointer);
            state.selection.finish(state.pointer, state.pressed_window);
            state.busy = true;
        }
        let _ = ReleaseCapture();
    }
    let destination = if kind == 8 {
        let result = super::output::save_dialog(hwnd);
        if lifetime.get() != Some(hwnd) {
            return Ok(());
        }
        match result {
            Ok(Some(path)) => super::output::Destination::File(path),
            Ok(None) => {
                // SAFETY: lifetime above confirms the same live window.
                unsafe {
                    (*pointer).busy = false;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                return Ok(());
            }
            Err(e) => {
                // SAFETY: same live window; return to editing on dialog failure.
                unsafe {
                    (*pointer).busy = false;
                }
                return Err(e);
            }
        }
    } else {
        super::output::Destination::Clipboard
    };
    // SAFETY: timer posts only to this live UI window; snapshot owns geometry, Arc shares immutable pixels without copying the desktop.
    unsafe {
        if SetTimer(Some(hwnd), 5, 20, None) == 0 {
            (*pointer).busy = false;
            return Err(Error::new(E_FAIL, "无法启动输出检查，请重试"));
        }
        let state = &mut *pointer;
        let rect = state
            .selection
            .rect
            .ok_or_else(|| Error::new(E_FAIL, "选区已失效"))?;
        match super::output::start(
            state.canvas.source(),
            rect,
            state.editor.snapshot(),
            hwnd.0 as usize,
            destination,
        ) {
            Ok(job) => state.output = Some(job),
            Err(e) => {
                state.busy = false;
                let _ = KillTimer(Some(hwnd), 5);
                return Err(e);
            }
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    Ok(())
}
