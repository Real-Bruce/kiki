//! Native frozen-desktop selection window. Annotation/output controls are staged for P4/P5.
use super::{
    capture::CaptureResult,
    selection_render::{Canvas, Layout, PaintInput, system_dark},
    stroke_control::{self, Wheel},
    text_input::{TextHit, TextInput, TextStart},
};
use crate::core::{
    annotation::{Editor, ObjectHit, Shape, Tool, text_center},
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

const STROKE_TIMER: usize = 6;
const STROKE_EDIT: u16 = 900;

pub struct Preview(Rc<Cell<Option<HWND>>>);
impl Preview {
    pub fn is_open(&self) -> bool {
        self.0.get().is_some()
    }
    pub fn hwnd(&self) -> Option<HWND> {
        self.0.get()
    }
    pub fn open_mode(capture: CaptureResult, screen: bool) -> Result<Self> {
        Self::open_impl(capture, true, screen)
    }
    #[cfg(test)]
    fn open_inner(capture: CaptureResult, visible: bool) -> Result<Self> {
        Self::open_impl(capture, visible, false)
    }
    fn open_impl(capture: CaptureResult, visible: bool, screen: bool) -> Result<Self> {
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
                WS_POPUP | WS_CLIPCHILDREN,
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
            // Non-text surfaces handle command keys; text composition belongs exclusively to the native EDIT.
            let _ = windows::Win32::UI::Input::Ime::ImmAssociateContextEx(
                hwnd,
                windows::Win32::UI::Input::Ime::HIMC::default(),
                0,
            );
            let canvas = Canvas::new(hwnd, capture.image)?;
            let mut selection = Selection::new(bounds);
            if screen {
                let pointer = position();
                selection.rect = capture
                    .screens
                    .iter()
                    .find(|s| pointer.is_some_and(|p| contains(s.bounds, p)))
                    .or(capture.screens.first())
                    .and_then(|s| bounds.intersection(s.bounds));
            }
            let cfg = super::preferences::current();
            let mut editor = Editor::default();
            for style in &mut editor.styles {
                style.color = cfg.color;
            }
            for (style, width) in editor.styles.iter_mut().zip(cfg.stroke_widths) {
                style.width = f32::from(width);
            }
            editor.text_size = f32::from(cfg.text_size);
            let state = State {
                lifetime,
                canvas,
                selection,
                editor,
                parameter_down: false,
                stroke_slider: false,
                stroke_edit: None,
                wheel: Wheel::default(),
                parameter_error: None,
                width_modified: false,
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
            if visible {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                bounds.left,
                bounds.top,
                width,
                height,
                if visible {
                    SWP_SHOWWINDOW
                } else {
                    SWP_NOACTIVATE
                },
            )?;
            if visible {
                let _ = SetForegroundWindow(hwnd);
                let _ = SetFocus(Some(hwnd));
            }
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
    stroke_slider: bool,
    stroke_edit: Option<StrokeEdit>,
    wheel: Wheel,
    parameter_error: Option<&'static str>,
    width_modified: bool,
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
struct StrokeEdit {
    hwnd: HWND,
    font: HFONT,
}
impl Drop for StrokeEdit {
    fn drop(&mut self) {
        // SAFETY: the edit is destroyed before deleting its UI-thread font.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteObject(self.font.into());
        }
    }
}
enum CanvasHit {
    ResizeSelection(usize),
    MoveSelection,
    Object(ObjectHit),
    Blank,
}

impl State {
    fn activate(&mut self, index: usize) -> bool {
        if !self.editor.enabled(index) {
            return false;
        }
        match index {
            0..=4 => {
                self.editor.finish_stroke_adjustment();
                self.wheel = Wheel::default();
                self.editor.cancel();
                let mut tool = [
                    Tool::Select,
                    Tool::Rectangle,
                    Tool::Arrow,
                    Tool::Pen,
                    Tool::Text,
                ][index];
                if tool == self.editor.tool {
                    tool = Tool::Select;
                }
                if tool != Tool::Select {
                    let matching = self
                        .editor
                        .selected
                        .and_then(|i| self.editor.objects().get(i))
                        .is_some_and(|o| o.matches_tool(tool));
                    if !matching {
                        self.editor.selected = None;
                    }
                }
                self.editor.tool = tool;
                if tool == Tool::Text
                    && let Some(object) = self
                        .editor
                        .selected
                        .and_then(|i| self.editor.objects().get(i))
                    && let Shape::Text { size, .. } = object.shape
                {
                    let mut style = object.style;
                    style.width /= self.scale();
                    self.editor.styles[3] = style;
                    self.editor.text_size = size / self.scale();
                }
            }
            19 if self.editor.tool.stroke_index().is_some() => {}
            11..=21 => {
                if let Err(message) = self.apply_parameter(index - 11) {
                    self.editor.warn(message);
                }
            }
            7..=9 => {
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
        } else if parameter < 11 && self.editor.tool == Tool::Text {
            self.editor.text_size = [18., 24., 36.][parameter - 8];
        } else {
            return Ok(());
        }
        if self.editor.tool == Tool::Text {
            restyle_text(self, style, parameter >= 8)
        } else {
            self.editor.set_stroke_color(style.color);
            Ok(())
        }
    }
    fn parameter_at(&self, p: PixelPoint) -> Option<usize> {
        self.canvas
            .parameter_at(&self.layout()?, p, self.editor.tool)
    }
    fn layout(&self) -> Option<Layout> {
        let rect = self.selection.rect.filter(|_| !self.selection.active())?;
        Some(self.canvas.layout(rect, &self.screens, self.editor.tool))
    }
    fn set_stroke_width(&mut self, width: u8) {
        if let Some(index) = self.editor.tool.stroke_index() {
            self.editor.set_stroke_width(width);
            self.parameter_error = None;
            match super::preferences::set_stroke_width(index, width) {
                Ok(()) => self.width_modified = true,
                Err(_) => self.parameter_error = Some("无法更新默认线宽，当前标注已保留。"),
            }
        }
    }
    fn drag_stroke_slider(&mut self) {
        if let Some((layout, slider)) = self
            .layout()
            .and_then(|l| l.stroke_slider().map(|r| (l, r)))
        {
            let inset = 6. * layout.scale;
            let width = stroke_control::slider_value(
                self.pointer.x as f32,
                slider.left as f32 + inset,
                slider.right as f32 - inset,
            );
            self.set_stroke_width(width);
        }
    }
    fn stroke_message(&self) -> Option<&'static str> {
        self.parameter_error.or_else(|| {
            (self.width_modified
                && matches!(
                    super::preferences::status(),
                    super::preferences::SaveStatus::Failed(_)
                ))
            .then_some("默认线宽保存失败，标注已保留。可在设置中重试。")
        })
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
                    .layout(r, &self.screens, self.editor.tool)
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
    fn hit_scale(&self) -> f32 {
        self.selection.rect.map_or_else(
            || self.scale(),
            |rect| {
                self.canvas
                    .layout(rect, &self.screens, self.editor.tool)
                    .scale
            },
        )
    }
    fn canvas_at(&self, p: PixelPoint, space: bool, scale: f32) -> CanvasHit {
        let radius = hit_radius(scale);
        if let Some(handle) = self.selection.resize_at(p, radius) {
            return CanvasHit::ResizeSelection(handle);
        }
        let inside = self.selection.rect.is_some_and(|r| contains(r, p));
        if inside && space {
            return CanvasHit::MoveSelection;
        }
        if let Some(hit) = self.editor.hit_at(p, radius, scale, inside) {
            return CanvasHit::Object(hit);
        }
        if inside && self.editor.tool == Tool::Select {
            CanvasHit::MoveSelection
        } else {
            CanvasHit::Blank
        }
    }
}

fn open_stroke_edit(hwnd: HWND, pointer: *mut State) -> Result<()> {
    // SAFETY: the preview and its numeric child are owned by this UI thread.
    // No Rust state references are held across focus changes or child messages.
    unsafe {
        let state = &*pointer;
        if state.stroke_edit.is_some() {
            return Ok(());
        }
        let Some(layout) = state.layout() else {
            return Ok(());
        };
        let Some(bounds) = layout.stroke_edit() else {
            return Ok(());
        };
        let desktop = state.canvas.bounds();
        let width = state.editor.style().width as u8;
        let font = CreateFontW(
            -(14. * layout.scale).round().max(1.) as i32,
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
            w!("Microsoft YaHei UI"),
        );
        if font.is_invalid() {
            return Err(Error::from_win32());
        }
        let child = match stroke_control::create_edit(hwnd, STROKE_EDIT, font) {
            Ok(child) => child,
            Err(error) => {
                let _ = DeleteObject(font.into());
                return Err(error);
            }
        };
        let edit = StrokeEdit { hwnd: child, font };
        stroke_control::set_edit(child, width);
        let mut metrics = TEXTMETRICW::default();
        let dc = GetDC(Some(child));
        let text_height = if dc.is_invalid() {
            (18. * layout.scale).round() as i32
        } else {
            let previous = SelectObject(dc, font.into());
            let _ = GetTextMetricsW(dc, &mut metrics);
            SelectObject(dc, previous);
            ReleaseDC(Some(child), dc);
            metrics.tmHeight.max(1)
        };
        let y = (bounds.top + bounds.bottom - text_height) / 2;
        let inset = (6. * layout.scale).round() as i32;
        SetWindowPos(
            child,
            None,
            bounds.left - desktop.left + inset,
            y - desktop.top,
            (bounds.right - bounds.left - 2 * inset).max(1),
            text_height,
            SWP_SHOWWINDOW | SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
        (*pointer).stroke_edit = Some(edit);
        let _ = SetFocus(Some(child));
        Ok(())
    }
}

fn close_stroke_edit(hwnd: HWND, pointer: *mut State, commit: bool, restore_focus: bool) {
    // SAFETY: removing the owned child from state first makes EN_KILLFOCUS re-entry harmless.
    unsafe {
        let Some(edit) = (*pointer).stroke_edit.take() else {
            return;
        };
        if commit {
            match stroke_control::read_edit(edit.hwnd) {
                Ok(width) => {
                    (*pointer).set_stroke_width(width);
                    (*pointer).editor.finish_stroke_adjustment();
                }
                Err(message) => (*pointer).parameter_error = Some(message),
            }
        }
        if restore_focus {
            let _ = SetFocus(Some(hwnd));
        }
        drop(edit);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn hit_radius(scale: f32) -> i32 {
    (6. * scale).round().max(4.) as i32
}

fn resize_cursor(handle: usize) -> windows::core::PCWSTR {
    match handle {
        0 | 4 => IDC_SIZENWSE,
        2 | 6 => IDC_SIZENESW,
        1 | 5 => IDC_SIZENS,
        _ => IDC_SIZEWE,
    }
}
fn position() -> Option<PixelPoint> {
    let mut p = POINT::default(); // SAFETY: call-scoped output pointer; PMv2 returns physical global desktop coordinates.
    unsafe {
        GetCursorPos(&mut p).ok()?;
    }
    Some(PixelPoint { x: p.x, y: p.y })
}
fn mouse_position(lp: LPARAM, desktop: PixelRect) -> PixelPoint {
    // Client coordinates are relative to the frozen desktop origin. Use the coordinates
    // attached to the mouse event, since the cursor may already have moved by dispatch time.
    let x = lp.0 as i16;
    let y = (lp.0 >> 16) as i16;
    if desktop
        .size()
        .is_some_and(|(w, h)| w > i16::MAX as u32 || h > i16::MAX as u32)
    {
        if let Some(p) = position() {
            return p;
        }
    }
    PixelPoint {
        x: desktop.left.saturating_add(i32::from(x)),
        y: desktop.top.saturating_add(i32::from(y)),
    }
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
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
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
            super::pinned_device::idle();
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if pointer.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        if (*pointer).busy
            && matches!(
                msg,
                WM_MOUSEMOVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_LBUTTONDBLCLK
                    | WM_KEYDOWN
                    | WM_KEYUP
            )
        {
            return LRESULT(0);
        }
        if let Some(result) = dispatch_text(hwnd, pointer, msg, wp, lp) {
            return result;
        }
        match msg {
            WM_TIMER if wp.0 == STROKE_TIMER => {
                let _ = KillTimer(Some(hwnd), STROKE_TIMER);
                (*pointer).editor.finish_stroke_adjustment();
                LRESULT(0)
            }
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
                        Ok(completed) => {
                            let opened = match completed {
                                super::output::Completed::Done => Ok(()),
                                super::output::Completed::Pinned(image) => super::pinned::open(
                                    image,
                                    (*pointer)
                                        .selection
                                        .rect
                                        .map(|r| PixelPoint {
                                            x: r.left,
                                            y: r.top,
                                        })
                                        .unwrap_or((*pointer).pointer),
                                ),
                            };
                            if let Err(e) = opened {
                                super::show_error(&format!("贴图创建失败，编辑已保留：{e}"));
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            } else {
                                let _ = DestroyWindow(hwnd);
                            }
                        }
                        Err(e) => {
                            super::show_error(&format!("输出失败，编辑内容已保留：{e}"));
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                LRESULT(0)
            }

            WM_ERASEBKGND => LRESULT(1),
            stroke_control::EDIT_KEY => {
                close_stroke_edit(hwnd, pointer, wp.0 != 27, true);
                if wp.0 == 9 {
                    SendMessageW(hwnd, WM_KEYDOWN, Some(wp), None);
                }
                LRESULT(0)
            }
            WM_COMMAND
                if (wp.0 & 65535) == usize::from(STROKE_EDIT)
                    && (wp.0 >> 16) as u32 == EN_KILLFOCUS =>
            {
                close_stroke_edit(hwnd, pointer, true, false);
                LRESULT(0)
            }
            WM_CTLCOLOREDIT => {
                let dark = (*pointer).canvas.dark;
                let dc = HDC(wp.0 as *mut _);
                let rgb = |value: u32| {
                    COLORREF(((value >> 16) & 255) | (value & 0xff00) | ((value & 255) << 16))
                };
                let bg = rgb(super::ui_palette::background(dark));
                SetTextColor(dc, rgb(super::ui_palette::foreground(dark)));
                SetBkColor(dc, bg);
                SetDCBrushColor(dc, bg);
                LRESULT(GetStockObject(DC_BRUSH).0 as isize)
            }
            super::config_store::SAVED => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let result = {
                    let state = &mut *pointer;
                    state.canvas.paint(PaintInput {
                        selection: &state.selection,
                        editor: &state.editor,
                        text_input: state.text_input.as_ref(),
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
                        editing_stroke: state.stroke_edit.is_some(),
                        parameter_error: state.stroke_message(),
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
                    state.pointer = mouse_position(lp, state.canvas.bounds());
                    state.selection.update(state.pointer);
                    state.editor.update(state.pointer);
                    if state.stroke_slider {
                        state.drag_stroke_slider();
                    }
                    state.hover = state.window_at(state.pointer);
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let state = &mut *pointer;
                if state.editor.tool.stroke_index().is_none()
                    || state.busy
                    || state.selection.rect.is_none()
                    || state.selection.active()
                    || state.stroke_slider
                    || state.stroke_edit.is_some()
                    || state.down_button.is_some()
                {
                    return LRESULT(0);
                }
                let steps = state.wheel.steps(wp);
                if steps != 0 {
                    let width = stroke_control::step(state.editor.style().width as u8, steps);
                    state.set_stroke_width(width);
                    if !state.editor.active() && SetTimer(Some(hwnd), STROKE_TIMER, 300, None) == 0
                    {
                        state.editor.finish_stroke_adjustment();
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                if (*pointer).selection.active() || (*pointer).editor.active() {
                    return LRESULT(0);
                }
                let space = (*pointer).text_input.is_none() && GetKeyState(VK_SPACE.0 as i32) < 0;
                if let Err(error) = finish_text(hwnd, pointer) {
                    super::show_error(&error.to_string());
                    return LRESULT(0);
                }
                let _ = KillTimer(Some(hwnd), STROKE_TIMER);
                (*pointer).editor.finish_stroke_adjustment();
                close_stroke_edit(hwnd, pointer, true, true);
                let mut request_text = None;
                let mut request_stroke = false;
                let mut error = None;
                {
                    let state = &mut *pointer;
                    state.pointer = mouse_position(lp, state.canvas.bounds());
                    state.pressed_window = state.window_at(state.pointer);
                    state.parameter_down = false;
                    state.keyboard_focus = false;
                    if let Some(parameter) = state.parameter_at(state.pointer) {
                        state.parameter_down = true;
                        state.focus = 11 + parameter;
                        if parameter >= 8 && state.editor.tool.stroke_index().is_some() {
                            if parameter == 8 {
                                state.stroke_slider = true;
                                state.drag_stroke_slider();
                            } else {
                                request_stroke = true;
                            }
                        } else if let Err(message) = state.apply_parameter(parameter) {
                            error = Some(message);
                        }
                    } else if let Some(button) = state.button_at(state.pointer) {
                        state.down_button = Some(button);
                        if state.editor.enabled(button) {
                            state.focus = button;
                        }
                    } else if state
                        .layout()
                        .and_then(|l| l.parameters)
                        .is_some_and(|r| contains(r, state.pointer))
                    {
                        state.parameter_down = true;
                    } else {
                        state.focus = usize::MAX;
                        let scale = state.hit_scale();
                        let p = state.pointer;
                        match state.canvas_at(p, space, scale) {
                            hit @ (CanvasHit::ResizeSelection(_) | CanvasHit::MoveSelection) => {
                                if matches!(hit, CanvasHit::MoveSelection)
                                    && state.editor.tool == Tool::Select
                                    && !space
                                {
                                    state.editor.selected = None;
                                }
                                state.selection.begin(p, hit_radius(scale));
                            }
                            CanvasHit::Object(ObjectHit::Text(index)) => {
                                request_text = Some(Some(index));
                            }
                            CanvasHit::Object(hit) => {
                                let double_clicked_text = msg == WM_LBUTTONDBLCLK
                                    && state.editor.tool == Tool::Select
                                    && matches!(hit, ObjectHit::Move(_));
                                if let Some(index) = double_clicked_text
                                    .then(|| state.editor.text_at(p))
                                    .flatten()
                                {
                                    request_text = Some(Some(index));
                                } else if let Err(message) = state.editor.begin_at(p, Some(hit)) {
                                    error = Some(message);
                                }
                            }
                            CanvasHit::Blank => {
                                if state.editor.tool == Tool::Select
                                    || state.selection.rect.is_none()
                                {
                                    state.editor.selected = None;
                                    state.selection.begin(p, hit_radius(scale));
                                } else if state.selection.rect.is_some_and(|r| contains(r, p)) {
                                    if state.editor.tool == Tool::Text {
                                        request_text = Some(None);
                                    } else if let Err(message) = state.editor.begin_at(p, None) {
                                        error = Some(message);
                                    }
                                }
                            }
                        }
                    }
                }
                if request_stroke {
                    (*pointer).parameter_down = false;
                    if let Err(error) = open_stroke_edit(hwnd, pointer) {
                        super::show_error(&error.to_string());
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                }
                if let Some(index) = request_text {
                    if let Err(e) = start_text(hwnd, pointer, index) {
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
                    state.pointer = mouse_position(lp, state.canvas.bounds());
                    warning = state.editor.take_warning();
                    if let Some(button) = state.down_button.take() {
                        if state.button_at(state.pointer) == Some(button) {
                            state.activate(button)
                        } else {
                            false
                        }
                    } else {
                        if state.stroke_slider {
                            state.drag_stroke_slider();
                            state.editor.finish_stroke_adjustment();
                            state.stroke_slider = false;
                        } else if state.editor.active() {
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
                    if state.stroke_slider {
                        state.editor.finish_stroke_adjustment();
                        state.stroke_slider = false;
                    }
                    state.parameter_down = false;
                    state.down_button = None;
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_KEYDOWN | WM_KEYUP if msg == WM_KEYDOWN || wp.0 == VK_SPACE.0 as usize => {
                if wp.0 != VK_ESCAPE.0 as usize
                    && ((*pointer).selection.active() || (*pointer).editor.active())
                {
                    return LRESULT(0);
                }
                let mut release = false;
                let close = {
                    let state = &mut *pointer;
                    match wp.0 as u32 {
                        27 => {
                            release = true;
                            if state.stroke_slider {
                                state.editor.finish_stroke_adjustment();
                                state.stroke_slider = false;
                                false
                            } else if state.down_button.take().is_some() {
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
                        13 | 32
                            if (wp.0 == 13 || msg == WM_KEYUP)
                                && state.keyboard_focus
                                && state.focus < 22 =>
                        {
                            if state.editor.tool.stroke_index().is_some() && state.focus == 20 {
                                if let Err(error) = open_stroke_edit(hwnd, pointer) {
                                    super::show_error(&error.to_string());
                                }
                                false
                            } else {
                                state.activate(state.focus)
                            }
                        }
                        37..=40
                            if state.keyboard_focus
                                && state.focus == 19
                                && state.editor.tool.stroke_index().is_some() =>
                        {
                            let step = if matches!(wp.0, 38 | 39) { 1 } else { -1 };
                            state.set_stroke_width(stroke_control::step(
                                state.editor.style().width as u8,
                                step,
                            ));
                            if SetTimer(Some(hwnd), STROKE_TIMER, 300, None) == 0 {
                                state.editor.finish_stroke_adjustment();
                            }
                            false
                        }
                        13 => {
                            state.pending_output = Some(9);
                            false
                        }
                        80 if GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                            state.pending_output = Some(7);
                            false
                        }
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
                if state.selection.active() || state.editor.active() {
                    return LRESULT(1);
                }
                let p = position().unwrap_or(state.pointer);
                let cursor = if state.editor.tool.stroke_index().is_some()
                    && state.parameter_at(p) == Some(9)
                {
                    IDC_IBEAM
                } else if state.button_at(p).is_some() || state.parameter_at(p).is_some() {
                    IDC_ARROW
                } else {
                    let space = state.text_input.is_none() && GetKeyState(VK_SPACE.0 as i32) < 0;
                    match state.canvas_at(p, space, state.hit_scale()) {
                        CanvasHit::ResizeSelection(handle) => resize_cursor(handle),
                        CanvasHit::MoveSelection
                        | CanvasHit::Object(ObjectHit::Move(_) | ObjectHit::Rotate(_)) => {
                            IDC_SIZEALL
                        }
                        CanvasHit::Object(ObjectHit::Handle(index, handle))
                            if matches!(
                                state.editor.objects()[index].shape,
                                Shape::Rectangle(_)
                            ) =>
                        {
                            resize_cursor(handle)
                        }
                        CanvasHit::Object(ObjectHit::Text(_)) => IDC_IBEAM,
                        _ => IDC_CROSS,
                    }
                };
                if let Ok(cursor) = LoadCursorW(None, cursor) {
                    SetCursor(Some(cursor));
                }
                LRESULT(1)
            }
            WM_SETTINGCHANGE | WM_THEMECHANGED => {
                let dark = system_dark();
                (*pointer).canvas.dark = dark;

                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn dispatch_text(
    hwnd: HWND,
    pointer: *mut State,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> Option<LRESULT> {
    // SAFETY: the live preview dispatch owns State. Take the service out across native calls
    // that can synchronously dispatch capture, focus and IME messages.
    unsafe {
        let input = (*pointer).text_input.as_ref()?;
        if msg == WM_KEYDOWN && !input.composing() {
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            if wp.0 == 27 {
                let input = (*pointer).text_input.take();
                drop(input);
                let _ = InvalidateRect(Some(hwnd), None, false);
                return Some(LRESULT(0));
            }
            let output = if ctrl {
                match wp.0 {
                    83 => Some(8),
                    80 => Some(7),
                    _ => None,
                }
            } else {
                None
            };
            if (ctrl && wp.0 == 13) || output.is_some() {
                let result = finish_text(hwnd, pointer).and_then(|()| {
                    if let Some(button) = output {
                        (*pointer).pending_output = Some(button);
                        begin_output(hwnd, pointer)?;
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    super::show_error(&error.to_string());
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                return Some(LRESULT(0));
            }
        }
        if matches!(
            msg,
            WM_SETCURSOR | WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK
        ) {
            let state = &*pointer;
            let p = if msg == WM_SETCURSOR {
                position().unwrap_or(state.pointer)
            } else {
                mouse_position(lp, state.canvas.bounds())
            };
            if !input.mouse_captured()
                && (state.button_at(p).is_some() || state.parameter_at(p).is_some())
            {
                return None;
            }
            let hit = input.hit_at(p, state.hit_scale())?;
            if msg == WM_SETCURSOR {
                let cursor = match hit {
                    TextHit::Content => IDC_IBEAM,
                    TextHit::Move | TextHit::Rotate => IDC_SIZEALL,
                };
                if let Ok(cursor) = LoadCursorW(None, cursor) {
                    SetCursor(Some(cursor));
                }
                return Some(LRESULT(1));
            }
            (*pointer).pointer = p;
            let result =
                if hit == TextHit::Rotate && matches!(msg, WM_LBUTTONDOWN | WM_LBUTTONDBLCLK) {
                    start_text_rotation(hwnd, pointer, p)
                } else {
                    let mut input = (*pointer).text_input.take()?;
                    let result = input.mouse(msg, wp, p, hit);
                    (*pointer).text_input = Some(input);
                    result
                };
            if let Err(error) = result {
                super::show_error(&error.to_string());
            }
            let _ = InvalidateRect(Some(hwnd), None, false);
            return Some(LRESULT(0));
        }
        let mut input = (*pointer).text_input.take()?;
        let result = match msg {
            WM_CAPTURECHANGED if input.mouse_captured() => {
                input.capture_lost();
                Ok(Some(LRESULT(0)))
            }
            WM_KEYDOWN
            | WM_KEYUP
            | WM_CHAR
            | WM_CUT
            | WM_COPY
            | WM_PASTE
            | WM_CLEAR
            | WM_UNDO
            | WM_IME_STARTCOMPOSITION
            | WM_IME_COMPOSITION
            | WM_IME_ENDCOMPOSITION
            | WM_IME_CHAR
            | WM_IME_NOTIFY
            | WM_IME_REQUEST
            | WM_IME_SETCONTEXT
            | WM_SETFOCUS
            | WM_KILLFOCUS => input.message(msg, wp, lp).map(Some),
            WM_TIMER if TextInput::owns_timer(wp.0) => input.message(msg, wp, lp).map(Some),
            _ => Ok(None),
        };
        (*pointer).text_input = Some(input);
        match result {
            Ok(Some(result)) => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                Some(result)
            }
            Ok(None) => None,
            Err(error) => {
                super::show_error(&format!("文字输入失败，草稿已保留：{error}"));
                Some(LRESULT(0))
            }
        }
    }
}

fn start_text_rotation(hwnd: HWND, pointer: *mut State, p: PixelPoint) -> Result<()> {
    finish_text(hwnd, pointer)?;
    // SAFETY: this UI-thread callback owns the live State. End the borrow before SetCapture.
    unsafe {
        let started = {
            let state = &mut *pointer;
            state.keyboard_focus = false;
            if let Some(index) = state.editor.selected {
                state
                    .editor
                    .begin_at(p, Some(ObjectHit::Rotate(index)))
                    .map_err(|message| Error::new(E_FAIL, message))?
            } else {
                false
            }
        };
        if started {
            SetCapture(hwnd);
        }
    }
    Ok(())
}

fn start_text(hwnd: HWND, pointer: *mut State, index: Option<usize>) -> Result<()> {
    // SAFETY: caller owns the live State. Open the service after releasing the borrow.
    let (index, origin, content, size, style, wrap_width, angle, desktop, area) = unsafe {
        let state = &mut *pointer;
        if state.text_input.is_some() {
            return Ok(());
        }
        state.editor.cancel();
        let area = state
            .selection
            .rect
            .ok_or_else(|| Error::new(E_FAIL, "请先选择截图范围"))?;
        let desktop = state.canvas.bounds();
        let scale = state.scale();
        let (origin, content, size, style, wrap_width, angle) = if let Some(index) = index {
            let object = &state.editor.objects()[index];
            let Shape::Text {
                bounds,
                content,
                size,
                wrap_width,
                angle,
            } = &object.shape
            else {
                return Ok(());
            };
            (
                PixelPoint {
                    x: bounds.left,
                    y: bounds.top,
                },
                content.clone(),
                *size,
                object.style,
                *wrap_width,
                *angle,
            )
        } else {
            let origin = PixelPoint {
                x: state
                    .pointer
                    .x
                    .clamp(area.left, area.right.saturating_sub(8).max(area.left)),
                y: state.pointer.y.clamp(
                    area.top,
                    area.bottom
                        .saturating_sub((state.editor.text_size * scale * 1.3).ceil() as i32)
                        .max(area.top),
                ),
            };
            let mut style = state.editor.styles[3];
            style.width *= scale;
            (
                origin,
                String::new(),
                state.editor.text_size * scale,
                style,
                area.right.saturating_sub(origin.x) as f32,
                0.,
            )
        };
        (
            index, origin, content, size, style, wrap_width, angle, desktop, area,
        )
    };
    let input = TextInput::open(
        hwnd,
        TextStart {
            index,
            origin,
            desktop,
            area,
            content: &content,
            size,
            wrap_width,
            style,
            angle,
        },
    )?;
    // SAFETY: the service and preview are retained by this UI-thread window.
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
            let center = if input.angle != 0. {
                input
                    .index
                    .and_then(|i| state.editor.objects().get(i))
                    .map(|object| {
                        let old = object.bounds();
                        let (cx, cy) = text_center(old);
                        (
                            cx + f64::from(input.origin.x) - f64::from(old.left),
                            cy + f64::from(input.origin.y) - f64::from(old.top),
                        )
                    })
            } else {
                None
            };
            let object = state.canvas.text_object(
                input.origin,
                content,
                input.size,
                input.wrap_width,
                input.style,
                (input.angle, center),
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
                angle,
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
                        (*angle, (*angle != 0.).then(|| text_center(*bounds))),
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
    } else if kind == 7 {
        super::output::Destination::Pin
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

#[cfg(feature = "diagnostics")]
pub(super) fn performance_display(hwnd: HWND) -> Result<String> {
    // SAFETY: the owning App provided this live UI-thread Preview; no window calls or callbacks while reading.
    unsafe {
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        if p.is_null() {
            return Err(Error::new(E_FAIL, "画布已关闭"));
        }
        Ok(format!(
            "Desktop: {:?}\nDisplays: {:#?}\n",
            (*p).canvas.bounds(),
            (*p).screens
        ))
    }
}

/// Drives the ordinary cancel / asynchronous composition-to-pin path in explicit performance probes.
#[cfg(feature = "diagnostics")]
pub(super) fn performance_finish(hwnd: HWND, pin: bool) -> Result<()> {
    // SAFETY: the diagnostic UI thread receives this live Preview HWND from its owning App.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if pointer.is_null() {
            return Err(Error::new(E_FAIL, "性能采样画布已关闭"));
        }
        if pin {
            (*pointer).selection.rect = Some((*pointer).canvas.bounds());
            let (_, editor) = super::composition::fixture()?;
            (*pointer).editor = editor;
            (*pointer).pending_output = Some(7);
            begin_output(hwnd, pointer)
        } else {
            SendMessageW(hwnd, WM_KEYDOWN, Some(WPARAM(VK_ESCAPE.0 as usize)), None);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{composition, output, tests::take_ui_errors};
    use super::*;
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };
    use windows::Win32::System::DataExchange::*;
    fn pump_until_done(preview: &Preview) {
        let deadline = Instant::now() + Duration::from_secs(6);
        while preview.is_open() && Instant::now() < deadline {
            // SAFETY: test thread owns this hidden window and its message queue.
            unsafe {
                let hwnd = preview.0.get().unwrap();
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
                if !(*p).busy {
                    return;
                }
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if preview.is_open() {
            // SAFETY: same checked hidden window is owned by the test.
            unsafe {
                let p = GetWindowLongPtrW(preview.0.get().unwrap(), GWLP_USERDATA) as *mut State;
                assert!(!(*p).busy, "output did not finish");
            }
        }
    }
    fn setup() -> (Preview, *mut State) {
        let (image, editor) = composition::fixture().unwrap();
        let bounds = image.bounds();
        let preview = Preview::open_inner(
            CaptureResult {
                image,
                #[cfg(feature = "diagnostics")]
                elapsed: Duration::ZERO,
                screens: Vec::new(),
                windows: Vec::new(),
            },
            false,
        )
        .unwrap();
        // SAFETY: test owns the hidden HWND and initializes its editor synchronously without a modal loop.
        unsafe {
            let p = GetWindowLongPtrW(preview.0.get().unwrap(), GWLP_USERDATA) as *mut State;
            (*p).selection.rect = Some(bounds);
            (*p).editor = editor;
            (preview, p)
        }
    }
    #[test]
    fn stroke_controls_route_native_input_and_render_exact_pixels() {
        use super::super::{
            config_store::Disk,
            preferences::{self, SaveStatus},
        };
        use std::os::windows::fs::OpenOptionsExt;

        fn wait_status() -> SaveStatus {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                preferences::poll();
                let status = preferences::status();
                if !matches!(status, SaveStatus::Saving) {
                    return status;
                }
                assert!(Instant::now() < deadline, "stroke settings save timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        super::super::tests::graphics_runtime();
        let _com = composition::Apartment::enter().unwrap();
        let path =
            std::env::temp_dir().join(format!("kiki-stroke-test-{}.ini", std::process::id()));
        std::fs::write(&path, "version=1\nstroke_width=8\nfirst_tip_seen=true\n").unwrap();
        // SAFETY: all HWNDs and messages are local to this test thread; only its own config
        // file is locked, and no borrow of Preview state crosses a window call.
        unsafe {
            register(GetModuleHandleW(None).unwrap().into()).unwrap();
            let (preview, p) = setup();
            let hwnd = preview.hwnd().unwrap();
            let (guard, warning) = preferences::initialize_at(hwnd, path.clone()).unwrap();
            assert!(warning.is_none());
            assert!(matches!(wait_status(), SaveStatus::Saved));
            assert_eq!(Disk::load(path.clone()).config.stroke_widths, [8; 3]);
            let original = std::fs::read(&path).unwrap();
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap();

            let selection = (*p).selection.rect.unwrap();
            let point = |x, y| PixelPoint {
                x: selection.left + x,
                y: selection.top + y,
            };
            let wheel = |amount: i16| {
                SendMessageW(
                    hwnd,
                    WM_MOUSEWHEEL,
                    Some(WPARAM(usize::from(amount as u16) << 16)),
                    None,
                );
            };
            (*p).editor = Editor::default();
            for (tool, y, steps, width) in [
                (Tool::Rectangle, 50, 11, 14.),
                (Tool::Arrow, 160, 19, 22.),
                (Tool::Pen, 260, 15, 18.),
            ] {
                (*p).activate(tool.button());
                (*p).editor.begin_at(point(40, y), None).unwrap();
                (*p).editor.update(point(150, y));
                wheel(60);
                assert_eq!((*p).editor.style().width, 3.);
                wheel(60);
                wheel(120 * (steps - 1));
                (*p).editor
                    .finish(point(390, if tool == Tool::Rectangle { y + 60 } else { y }));
                assert_eq!((*p).editor.style().width, width);
            }
            assert_eq!(preferences::current().stroke_widths, [14, 22, 18]);
            assert!(matches!(wait_status(), SaveStatus::Failed(_)));
            assert!((*p).stroke_message().unwrap().contains("保存失败"));
            drop(lock);
            assert_eq!(std::fs::read(&path).unwrap(), original);

            for _ in 0..100 {
                wheel(120);
                wheel(-120);
            }
            wheel(120);
            wheel(120);
            SendMessageW(hwnd, WM_TIMER, Some(WPARAM(STROKE_TIMER)), None);
            assert_eq!((*p).editor.style().width, 20.);
            (*p).editor.undo();
            assert_eq!((*p).editor.style().width, 18.);
            assert_eq!(preferences::current().stroke_widths, [14, 22, 20]);
            (*p).editor.redo();
            assert_eq!((*p).editor.style().width, 20.);
            (*p).editor.undo();
            open_stroke_edit(hwnd, p).unwrap();
            let edit = (*p).stroke_edit.as_ref().unwrap().hwnd;
            SetWindowTextW(edit, w!("65")).unwrap();
            SendMessageW(edit, WM_KEYDOWN, Some(WPARAM(13)), None);
            assert!((*p).stroke_edit.is_none());
            assert!((*p).stroke_message().unwrap().contains("1–64"));
            assert_eq!((*p).editor.style().width, 18.);
            assert!(matches!(wait_status(), SaveStatus::Saved));
            assert_eq!(Disk::load(path.clone()).config.stroke_widths, [14, 22, 20]);

            let image =
                composition::render(&(*p).canvas.source(), selection, &(*p).editor).unwrap();
            let stride = image.stride() as usize;
            let measure = |center: usize| {
                (center - 20..center + 20)
                    .filter(|y| {
                        let offset = y * stride + 130 * 4;
                        image.pixels()[offset..offset + 4] == [64, 59, 255, 255]
                    })
                    .count()
            };
            assert_eq!([measure(50), measure(160), measure(260)], [14, 22, 18]);
            drop(guard);
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn selection_edges_win_over_annotation_handles_and_space_moves_selection() {
        super::super::tests::graphics_runtime();
        let _com = composition::Apartment::enter().unwrap();
        // SAFETY: the test owns the hidden preview and borrows its state only for synchronous model calls.
        unsafe {
            register(GetModuleHandleW(None).unwrap().into()).unwrap();
            let (_preview, p) = setup();
            let state = &mut *p;
            state.editor.tool = Tool::Rectangle;
            state.editor.selected = Some(0);
            let r = state.editor.objects()[0].bounds();
            let corner = PixelPoint {
                x: r.left,
                y: r.top,
            };
            let inner = PixelPoint {
                x: r.left + 50,
                y: r.top + 20,
            };
            assert!(matches!(
                state.canvas_at(corner, false, 1.),
                CanvasHit::Object(ObjectHit::Handle(0, 0))
            ));
            assert!(matches!(
                state.canvas_at(inner, false, 1.),
                CanvasHit::Blank
            ));
            assert!(matches!(
                state.canvas_at(inner, true, 1.),
                CanvasHit::MoveSelection
            ));
            state.selection.rect.as_mut().unwrap().left = r.left;
            assert!(matches!(
                state.canvas_at(corner, false, 1.),
                CanvasHit::ResizeSelection(7)
            ));
            assert!(matches!(
                state.canvas_at(corner, true, 1.),
                CanvasHit::ResizeSelection(7)
            ));
            state.activate(Tool::Rectangle.button());
            assert_eq!(state.editor.tool, Tool::Select);
            assert!(matches!(
                state.canvas_at(inner, false, 1.),
                CanvasHit::MoveSelection
            ));
        }
    }
    #[test]
    fn draft_text_rotation_starts_on_first_press_and_keeps_the_text() {
        super::super::tests::graphics_runtime();
        let _com = composition::Apartment::enter().unwrap();
        // SAFETY: the test owns the hidden preview on this thread, with no state borrow across native calls.
        unsafe {
            register(GetModuleHandleW(None).unwrap().into()).unwrap();
            let (preview, pointer) = setup();
            let hwnd = preview.hwnd().unwrap();
            (*pointer).editor.tool = Tool::Text;
            let start = PixelPoint { x: 20, y: 90 };
            (*pointer).pointer = start;
            start_text(hwnd, pointer, None).unwrap();
            let phrase = "文字 移动";
            for c in phrase.encode_utf16() {
                dispatch_text(hwnd, pointer, WM_CHAR, WPARAM(c as usize), LPARAM(1)).unwrap();
            }
            let desktop = (*pointer).canvas.bounds();
            let send_mouse = |msg, p: PixelPoint| {
                let x = (p.x - desktop.left) as u16;
                let y = (p.y - desktop.top) as u16;
                let packed = u32::from(x) | (u32::from(y) << 16);
                SendMessageW(hwnd, msg, Some(WPARAM(0)), Some(LPARAM(packed as isize)));
            };
            let bounds = {
                let input = (*pointer).text_input.as_ref().unwrap();
                let bounds = input.content_bounds();
                assert_eq!(input.hit_at(start, 1.5), Some(TextHit::Content));
                let border = PixelPoint {
                    x: bounds.left - 10,
                    y: start.y + 6,
                };
                assert_eq!(input.hit_at(border, 1.5), Some(TextHit::Move));
                let mut grab = crate::core::annotation::text_rotation_handle(bounds, 0., 1.5);
                grab.x += 8;
                assert_eq!(input.hit_at(grab, 1.5), Some(TextHit::Rotate));
                bounds
            };
            let border = PixelPoint {
                x: bounds.left - 6,
                y: bounds.top + 12,
            };
            let moved = PixelPoint {
                x: border.x + 25,
                y: border.y + 12,
            };
            send_mouse(WM_LBUTTONDOWN, border);
            send_mouse(WM_MOUSEMOVE, moved);
            send_mouse(WM_LBUTTONUP, moved);
            let input = (*pointer).text_input.as_ref().unwrap();
            assert_eq!(
                input.origin,
                PixelPoint {
                    x: start.x + 25,
                    y: start.y + 12
                }
            );
            assert_eq!(input.content().unwrap(), phrase);
            let mut grab = crate::core::annotation::text_rotation_handle(
                input.content_bounds(),
                0.,
                (*pointer).hit_scale(),
            );
            grab.x += 5;
            send_mouse(WM_LBUTTONDOWN, grab);
            assert!((*pointer).text_input.is_none());
            assert!((*pointer).editor.active());
            send_mouse(
                WM_LBUTTONUP,
                PixelPoint {
                    x: grab.x + 80,
                    y: grab.y + 60,
                },
            );
            let state = &mut *pointer;
            let object = state.editor.objects().last().unwrap();
            assert!(
                matches!(&object.shape, Shape::Text { content, angle, .. } if content == phrase && angle.abs() > 0.5)
            );
            state.editor.undo();
            assert!(matches!(&state.editor.objects().last().unwrap().shape,
                Shape::Text { content, angle, .. } if content == phrase && *angle == 0.));
        }
    }
    #[test]
    fn real_output_failures_preserve_editing_then_retry_and_cancel() {
        super::super::tests::graphics_runtime();
        let _com = composition::Apartment::enter().unwrap();
        // SAFETY: this test owns all hidden HWNDs; only genuine output calls, clipboard locks, native EDIT and window messages are exercised.
        unsafe {
            let instance = GetModuleHandleW(None).unwrap().into();
            register(instance).unwrap();
            if let Err(error) = super::super::pinned::register(instance) {
                assert_eq!(
                    error.code(),
                    windows::core::HRESULT::from_win32(ERROR_CLASS_ALREADY_EXISTS.0)
                );
            }
            let (preview, p) = setup();
            let hwnd = preview.0.get().unwrap();
            let objects = (*p).editor.objects().to_vec();
            let sequence = GetClipboardSequenceNumber();
            output::TEST_DIALOG.with_borrow_mut(|path| *path = Some(None));
            (*p).pending_output = Some(8);
            begin_output(hwnd, p).unwrap();
            assert!(!(*p).busy);
            assert!(preview.is_open());
            assert_eq!((*p).editor.objects(), objects);
            let dir =
                std::env::temp_dir().join(format!("kiki-native-output-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("readonly.png");
            std::fs::write(&path, b"keep original").unwrap();
            let original_permissions = std::fs::metadata(&path).unwrap().permissions();
            let mut permissions = original_permissions.clone();
            permissions.set_readonly(true);
            std::fs::set_permissions(&path, permissions).unwrap();
            output::TEST_DIALOG.with_borrow_mut(|value| *value = Some(Some(path.clone())));
            (*p).pending_output = Some(8);
            begin_output(hwnd, p).unwrap();
            pump_until_done(&preview);
            assert!(preview.is_open());
            assert_eq!((*p).editor.objects(), objects);
            assert_eq!(std::fs::read(&path).unwrap(), b"keep original");
            assert_eq!(take_ui_errors().len(), 1);
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
            assert_eq!(GetClipboardSequenceNumber(), sequence);
            // Real clipboard contention, without clearing the user's current formats.
            let locker = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki clipboard contention test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )
            .unwrap();
            OpenClipboard(Some(locker)).unwrap();
            (*p).pending_output = Some(9);
            begin_output(hwnd, p).unwrap();
            pump_until_done(&preview);
            CloseClipboard().unwrap();
            DestroyWindow(locker).unwrap();
            assert!(preview.is_open());
            assert_eq!((*p).editor.objects(), objects);
            assert!(take_ui_errors()[0].contains("占用"));
            assert_eq!(GetClipboardSequenceNumber(), sequence);
            // Genuine retry after a locked target becomes writable, preserving all annotations.
            std::fs::set_permissions(&path, original_permissions).unwrap();
            use std::os::windows::fs::OpenOptionsExt;
            let locked = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&path)
                .unwrap();
            output::TEST_DIALOG.with_borrow_mut(|value| *value = Some(Some(path.clone())));
            (*p).pending_output = Some(8);
            begin_output(hwnd, p).unwrap();
            pump_until_done(&preview);
            assert!(preview.is_open());
            assert_eq!(take_ui_errors().len(), 1);
            drop(locked);
            output::TEST_DIALOG.with_borrow_mut(|value| *value = Some(Some(path.clone())));
            (*p).pending_output = Some(8);
            begin_output(hwnd, p).unwrap();
            pump_until_done(&preview);
            assert!(!preview.is_open());
            assert_eq!(GetClipboardSequenceNumber(), sequence);
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 640);
            assert!(take_ui_errors().is_empty());
            // A live native multi-line draft is committed before pinning; source desktop ownership is released.
            let (preview, p) = setup();
            let hwnd = preview.0.get().unwrap();
            let bounds = (*p).canvas.bounds();
            (*p).text_input = Some(
                TextInput::open(
                    hwnd,
                    TextStart {
                        index: None,
                        origin: PixelPoint {
                            x: bounds.left + 40,
                            y: bounds.top + 40,
                        },
                        desktop: bounds,
                        area: bounds,
                        content: "未提交中文\n第二行",
                        size: 24.,
                        wrap_width: 320.,
                        style: crate::core::annotation::Style::default(),
                        angle: 0.,
                    },
                )
                .unwrap(),
            );
            SendMessageW(hwnd, WM_PAINT, None, None);
            assert!(
                take_ui_errors().is_empty(),
                "windowless editor must render on the real preview surface"
            );
            let source = Arc::downgrade(&(*p).canvas.source());
            (*p).pending_output = Some(7);
            begin_output(hwnd, p).unwrap();
            assert!((*p).text_input.is_none());
            assert_eq!((*p).editor.objects().len(), objects.len() + 1);
            pump_until_done(&preview);
            assert!(!preview.is_open());
            assert!(source.upgrade().is_none());
            assert_eq!(GetClipboardSequenceNumber(), sequence);
            super::super::pinned::close_all();
            assert!(take_ui_errors().is_empty());
            // Closing a preview while its output worker is blocked cancels it; subsequent work remains possible.
            let (preview, p) = setup();
            let hwnd = preview.0.get().unwrap();
            let locker = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("kiki clipboard contention test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )
            .unwrap();
            OpenClipboard(Some(locker)).unwrap();
            (*p).pending_output = Some(9);
            begin_output(hwnd, p).unwrap();
            let weak = Arc::downgrade(&(*p).canvas.source());
            drop(preview);
            CloseClipboard().unwrap();
            DestroyWindow(locker).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while weak.upgrade().is_some() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(weak.upgrade().is_none());
            let (image, editor) = composition::fixture().unwrap();
            let rendered = composition::render(&image, image.bounds(), &editor).unwrap();
            output::save_atomic(&rendered, &path, &AtomicBool::new(false)).unwrap();
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
            std::fs::remove_file(path).unwrap();
            std::fs::remove_dir(dir).unwrap();
            super::super::pinned_render::shutdown();
        }
    }
}
