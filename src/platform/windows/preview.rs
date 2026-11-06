//! Native frozen-desktop selection window. Annotation/output controls are staged for P4/P5.
use super::{
    capture::CaptureResult,
    selection_render::{Canvas, PaintInput, system_dark},
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
    screens: Vec<DisplayInfo>,
    windows: Vec<PixelRect>,
    pointer: PixelPoint,
    hover: Option<PixelRect>,
    pressed_window: Option<PixelRect>,
    down_button: Option<usize>,
    focus: usize,
}
impl State {
    fn activate(&mut self, index: usize) -> bool {
        if !self.editor.enabled(index) {
            return false;
        }
        match index {
            0..=3 => {
                self.editor.cancel();
                let tool = [Tool::Select, Tool::Rectangle, Tool::Arrow, Tool::Pen][index];
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
            }
            5 => self.editor.undo(),
            6 => self.editor.redo(),
            10 => return true,
            _ => {}
        }
        false
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
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let result = {
                    let state = &mut *pointer;
                    state.canvas.paint(PaintInput {
                        selection: &state.selection,
                        editor: &state.editor,
                        hover: state.hover,
                        screens: &state.screens,
                        pointer: state.pointer,
                        focus: state.focus,
                        pressed: state.down_button,
                    })
                };
                let _ = EndPaint(hwnd, &ps);
                if let Err(error) = result {
                    super::show_error(&format!("选区绘制失败：{error}"));
                }
                LRESULT(0)
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
                let mut error = None;
                {
                    let state = &mut *pointer;
                    state.pointer = position().unwrap_or(state.pointer);
                    state.pressed_window = state.window_at(state.pointer);
                    state.parameter_down = false;
                    if let Some(parameter) = state.parameter_at(state.pointer) {
                        state.parameter_down = true;
                        let mut style = state.editor.style();
                        if parameter < 8 {
                            style.color = super::selection_render::ANNOTATION_COLORS[parameter];
                        } else {
                            style.width = [1., 3., 6.][parameter - 8];
                        }
                        state.editor.set_style(style, state.scale());
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
                            match state
                                .editor
                                .begin(state.pointer, state.radius(), state.scale())
                            {
                                Ok(active) => active,
                                Err(message) => {
                                    error = Some(message);
                                    true
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
                            let backwards = GetKeyState(VK_SHIFT.0 as i32) < 0;
                            let mut next = if state.focus < 11 {
                                state.focus
                            } else if backwards {
                                0
                            } else {
                                10
                            };
                            for _ in 0..11 {
                                next = if backwards {
                                    (next + 10) % 11
                                } else {
                                    (next + 1) % 11
                                };
                                if state.editor.enabled(next) {
                                    break;
                                }
                            }
                            state.focus = next;
                            false
                        }
                        13 | 32 => state.activate(state.focus),
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
