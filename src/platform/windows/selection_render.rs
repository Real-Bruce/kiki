use super::{
    capture_toolbar::{self, Layout},
    parameter_control,
    text_input::TextInput,
    toolbar_icons::{BarPaint, BarState, ToolbarIcons},
    ui_palette as palette,
};
use crate::core::{
    annotation::{
        Editor, Shape, Tool, rotate_point, text_center, text_frame, text_rotation_handle,
    },
    config::CaptureParameter,
    display::DisplayInfo,
    geometry::PixelRect,
    image::Image,
    selection::{PixelPoint, Selection, contains, fit_panel, selection_label},
};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
        },
    },
    core::{Interface, Result, w},
};
use windows_numerics::{Matrix3x2, Vector2};

const BAR_RADIUS: f32 = 10.;
const COLOR_START: f32 = 10.;
const COLOR_STEP: f32 = 30.;

pub struct PaintInput<'a> {
    pub selection: &'a Selection,
    pub editor: &'a Editor,
    pub corner_radius: u16,
    pub text_input: Option<&'a TextInput>,
    pub busy: bool,
    pub hover: Option<PixelRect>,
    pub screens: &'a [DisplayInfo],
    pub drawing_tools: [bool; 4],
    pub pointer: PixelPoint,
    pub focus: usize,
    pub pressed: Option<usize>,
    pub editing_parameter: bool,
    pub parameter: Option<(CaptureParameter, u16)>,
    pub parameter_error: Option<&'static str>,
}
pub struct Canvas {
    target: ID2D1RenderTarget,
    factory: ID2D1Factory,
    gdi: ID2D1GdiInteropRenderTarget,
    context: ID2D1DeviceContext,
    swap: windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    bitmap: ID2D1Bitmap,
    brush: ID2D1SolidColorBrush,
    dw: IDWriteFactory,
    text: IDWriteTextFormat,
    parameter_text: Option<(f32, IDWriteTextFormat)>,
    icons: ToolbarIcons,
    scale: f32,
    pub dark: bool,
    source: std::sync::Arc<Image>,
    _device: std::rc::Rc<super::pinned_device::Device>,
}
pub fn system_dark() -> bool {
    match super::preferences::theme() {
        Some(crate::core::config::Theme::Dark) => return true,
        Some(crate::core::config::Theme::Light) => return false,
        Some(crate::core::config::Theme::System) => {}
        None => {
            if std::env::args_os().any(|a| a == "--theme=dark") {
                return true;
            }
            if std::env::args_os().any(|a| a == "--theme=light") {
                return false;
            }
        }
    }
    super::preferences::system_is_dark()
}
impl Canvas {
    pub fn source(&self) -> std::sync::Arc<Image> {
        self.source.clone()
    }
    pub fn bounds(&self) -> PixelRect {
        self.source.bounds()
    }
    pub fn new(hwnd: HWND, image: Image) -> Result<Self> {
        // SAFETY: UI-thread COM objects, live HWND and call-scoped pixel slice; CreateBitmap copies the source.
        unsafe {
            let (width, height) = image
                .bounds()
                .size()
                .ok_or_else(windows::core::Error::from_win32)?;
            let device = super::pinned_device::get()?;
            let (context, swap) = device.surface(hwnd, width, height, true)?;
            let target: ID2D1RenderTarget = context.cast()?;
            let factory = target.GetFactory()?;
            let gdi = target.cast()?;
            let bitmap = target.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(image.pixels().as_ptr().cast()),
                image.stride(),
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.,
                    dpiY: 96.,
                },
            )?;
            let brush = target.CreateSolidColorBrush(&color(0, 0, 0, 1.), None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let text = format(&dw, 1.)?;
            let icons = ToolbarIcons::new(&factory)?;
            Ok(Self {
                target,
                factory,
                gdi,
                context,
                swap,
                _device: device,
                bitmap,
                brush,
                dw,
                text,
                parameter_text: None,
                icons,
                scale: 1.,
                dark: system_dark(),
                source: std::sync::Arc::new(image),
            })
        }
    }
    pub fn layout(
        &self,
        rect: PixelRect,
        screens: &[DisplayInfo],
        tool: Tool,
        drawing_tools: [bool; 4],
    ) -> Layout {
        let (screen, preferred) = self.display(rect, screens);
        Layout::new(rect, screen, preferred, tool != Tool::Select, drawing_tools)
    }
    fn display(&self, rect: PixelRect, screens: &[DisplayInfo]) -> (PixelRect, f32) {
        let center = PixelPoint {
            x: ((i64::from(rect.left) + i64::from(rect.right)) / 2) as i32,
            y: ((i64::from(rect.top) + i64::from(rect.bottom)) / 2) as i32,
        };
        let screen = screens
            .iter()
            .find(|screen| contains(screen.bounds, center))
            .or_else(|| {
                screens.iter().max_by_key(|screen| {
                    screen
                        .bounds
                        .intersection(rect)
                        .and_then(|r| r.size())
                        .map_or(0, |(w, h)| u64::from(w) * u64::from(h))
                })
            });
        screen.map_or((self.source.bounds(), 1.), |s| {
            (s.bounds, s.scale_percent as f32 / 100.)
        })
    }
    pub fn paint(&mut self, input: PaintInput<'_>) -> Result<()> {
        let PaintInput {
            selection,
            editor,
            corner_radius,
            text_input,
            busy,
            hover,
            screens,
            drawing_tools,
            pointer,
            focus,
            pressed,
            editing_parameter,
            parameter,
            parameter_error,
        } = input;
        let editing = text_input.and_then(|input| input.index);
        let text_active = text_input.is_some();
        let layout = selection
            .rect
            .filter(|_| !selection.active())
            .map(|r| self.layout(r, screens, editor.tool, drawing_tools));
        let anchor = selection.rect.or(hover).unwrap_or(PixelRect {
            left: pointer.x,
            top: pointer.y,
            right: pointer.x,
            bottom: pointer.y,
        });
        let (screen, preferred) = self.display(anchor, screens);
        let scale = layout.as_ref().map_or(preferred, |l| l.scale);
        if let Some(layout) = &layout
            && layout.parameters.is_some()
            && (layout.parameter_scale - scale).abs() > 0.001
            && self
                .parameter_text
                .as_ref()
                .is_none_or(|(old, _)| (old - layout.parameter_scale).abs() > 0.001)
        {
            self.parameter_text = Some((
                layout.parameter_scale,
                format(&self.dw, layout.parameter_scale)?,
            ));
        }
        let text_layouts = super::annotation_render::layouts(&self.dw, editor)?;
        let geometry = super::annotation_render::geometry(&self.factory, self.bounds(), editor)?;
        let selected = selection.rect.or(hover);
        let radius = selected.map_or(0., |rect| rect.corner_radius(corner_radius));
        let rounded = selected
            .filter(|_| radius > 0.)
            .map(|rect| self.rounded_selection(rect, radius))
            .transpose()?;
        // SAFETY: all drawing resources belong to this UI thread; BeginDraw is always paired with EndDraw.
        unsafe {
            if (scale - self.scale).abs() > 0.001 {
                self.text = format(&self.dw, scale)?;
                self.scale = scale;
            }
            self.target.BeginDraw();
            self.target.Clear(Some(&color(0, 0, 0, 1.)));
            self.target.DrawBitmap(
                &self.bitmap,
                None,
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
            let (width, height) = self.source.bounds().size().unwrap_or((1, 1));
            let w = width as f32;
            let h = height as f32;
            if let Some(rect) = selected {
                let r = self.local(rect);
                let shade = tone(palette::foreground(false), 0.50);
                if let Some((_, exterior)) = &rounded {
                    self.brush.SetColor(&shade);
                    self.target.FillGeometry(exterior, &self.brush, None);
                } else {
                    for mask in [
                        D2D_RECT_F {
                            left: 0.,
                            top: 0.,
                            right: w,
                            bottom: r.top,
                        },
                        D2D_RECT_F {
                            left: 0.,
                            top: r.bottom,
                            right: w,
                            bottom: h,
                        },
                        D2D_RECT_F {
                            left: 0.,
                            top: r.top,
                            right: r.left,
                            bottom: r.bottom,
                        },
                        D2D_RECT_F {
                            left: r.right,
                            top: r.top,
                            right: w,
                            bottom: r.bottom,
                        },
                    ] {
                        self.fill(mask, shade);
                    }
                }
                self.outline(r, radius, 2.5, tone(palette::on_accent(self.dark), 1.));
                self.outline(r, radius, 1.5, tone(palette::accent(self.dark), 1.));
                if selection.rect.is_some() {
                    for point in Selection::handles(rect) {
                        let x = (i64::from(point.x) - i64::from(self.source.bounds().left)) as f32;
                        let y = (i64::from(point.y) - i64::from(self.source.bounds().top)) as f32;
                        let size = 3. * scale;
                        let handle = D2D_RECT_F {
                            left: x - size,
                            top: y - size,
                            right: x + size,
                            bottom: y + size,
                        };
                        self.fill(handle, tone(palette::background(self.dark), 1.));
                        self.brush.SetColor(&tone(palette::accent(self.dark), 1.));
                        self.target.DrawRectangle(&handle, &self.brush, 1., None);
                    }
                }
                if let Some((rw, rh)) = rect.size() {
                    let label = self.local(selection_label(
                        rect,
                        screen,
                        layout.as_ref().map(|l| l.bounds),
                        (132. * scale).round() as i32,
                        (26. * scale).round() as i32,
                        (3. * scale).round() as i32,
                    ));
                    self.round(label, 5. * scale, tone(palette::accent(self.dark), 1.));
                    self.label(
                        &format!("{rw} × {rh} px"),
                        label,
                        tone(palette::on_accent(self.dark), 1.),
                    );
                }
            } else {
                self.fill(
                    D2D_RECT_F {
                        left: 0.,
                        top: 0.,
                        right: w,
                        bottom: h,
                    },
                    tone(palette::foreground(false), 0.50),
                );
            }
            self.paint_annotations(
                editor,
                selection.rect,
                rounded.as_ref().map(|(mask, _)| mask),
                editing,
                (&text_layouts, &geometry),
            );
            let text_draw = if let (Some(input), Some(area)) = (text_input, selection.rect) {
                let area = self.local(area);
                let result = input.draw(
                    &self.gdi,
                    RECT {
                        left: area.left as i32,
                        top: area.top as i32,
                        right: area.right as i32,
                        bottom: area.bottom as i32,
                    },
                    (2. * radius) as i32,
                );
                self.paint_text_frame(input.content_bounds(), 0.);
                result
            } else {
                Ok(())
            };
            if let Some(layout) = layout {
                self.paint_toolbar(&layout, editor, pointer, focus, pressed, busy);
                self.paint_parameters(&layout, editor, focus);
                if let Some(parameter) = parameter {
                    self.paint_parameter_slider(&layout, parameter, focus, editing_parameter);
                }
            }
            if busy || parameter_error.is_some() || super::preferences::hints() {
                let selected = selection.rect.is_some();
                let hint_width = if text_active || selected { 480. } else { 350. };
                let display = self.local(screen);
                let left = display.left + 16. * scale;
                // Keep the cue clear of the taskbar, on the display being used.
                let bottom = display.bottom - 56. * scale;
                let hint = D2D_RECT_F {
                    left,
                    top: bottom - 32. * scale,
                    right: (left + hint_width * scale).min(display.right - 16. * scale),
                    bottom,
                };
                self.round(hint, 6. * scale, tone(palette::sidebar(true), 0.96));
                self.label(
                    if busy {
                        "正在输出，请稍候…"
                    } else if let Some(message) = parameter_error {
                        message
                    } else if text_active {
                        "Enter 换行　Ctrl+Enter 完成　Esc 取消文字"
                    } else if selected {
                        "复制后可粘贴　　Ctrl+S 保存　　Ctrl+P 贴图"
                    } else {
                        "拖动选择截图区域　　F 当前屏幕　　Esc 取消"
                    },
                    hint,
                    tone(palette::foreground(true), 1.),
                );
            }
            self.target.EndDraw(None, None)?;
            text_draw?;
            self.swap
                .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0))
                .ok()
        }
    }
    fn rounded_selection(
        &self,
        rect: PixelRect,
        radius: f32,
    ) -> Result<(ID2D1Geometry, ID2D1GeometryGroup)> {
        // SAFETY: geometries use the canvas's UI-thread factory. The group retains both inputs.
        unsafe {
            let interior: ID2D1Geometry = self
                .factory
                .CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                    rect: self.local(rect),
                    radiusX: radius,
                    radiusY: radius,
                })?
                .into();
            let desktop: ID2D1Geometry = self
                .factory
                .CreateRectangleGeometry(&self.local(self.bounds()))?
                .into();
            let exterior = self.factory.CreateGeometryGroup(
                D2D1_FILL_MODE_ALTERNATE,
                &[Some(desktop), Some(interior.clone())],
            )?;
            Ok((interior, exterior))
        }
    }
    fn paint_toolbar(
        &self,
        layout: &Layout,
        editor: &Editor,
        pointer: PixelPoint,
        focus: usize,
        pressed: Option<usize>,
        busy: bool,
    ) {
        let s = layout.scale;
        let dark = self.dark;
        let desktop = self.bounds();
        let hover = layout.button_at(pointer);
        BarPaint {
            target: &self.target,
            brush: &self.brush,
            icons: &self.icons,
            text: &self.text,
            dark,
        }
        .draw(
            layout,
            PixelPoint {
                x: desktop.left,
                y: desktop.top,
            },
            BarState {
                selected: editor.tool.button(),
                hover,
                pressed,
                focus,
                undo: editor.can_undo(),
                redo: editor.can_redo(),
                busy,
            },
        );
        if let Some(button) = layout.buttons().find(|button| hover == Some(button.id))
            && super::preferences::hints()
        {
            let b = button.bounds;
            let name = capture_toolbar::name(button.id);
            let width = ((name.chars().count() as f32 * 14. + 16.).max(90.) * s).round() as i32;
            let center = ((i64::from(b.left) + i64::from(b.right)) / 2) as i32;
            let t = self.local(fit_panel(
                PixelPoint {
                    x: center.saturating_sub(width / 2),
                    y: layout.toolbar.top.saturating_sub((34. * s).round() as i32),
                },
                layout.screen,
                width,
                (28. * s).round() as i32,
            ));
            self.panel(t, 6. * s);
            self.label(name, t, tone(palette::foreground(dark), 1.));
        }
    }

    pub fn text_object(
        &self,
        origin: PixelPoint,
        content: String,
        size: f32,
        width: f32,
        style: crate::core::annotation::Style,
        (angle, center): (f64, Option<(f64, f64)>),
    ) -> Result<crate::core::annotation::Object> {
        let bounds = self.text_bounds(origin, &content, size, width, center)?;
        Ok(crate::core::annotation::Object {
            shape: Shape::Text {
                bounds,
                content,
                size,
                wrap_width: width,
                angle,
            },
            style,
        })
    }
    pub fn text_bounds(
        &self,
        origin: PixelPoint,
        content: &str,
        size: f32,
        width: f32,
        center: Option<(f64, f64)>,
    ) -> Result<PixelRect> {
        let metrics = super::annotation_render::text_metrics(&self.dw, content, size, width)?;
        let w = metrics.widthIncludingTrailingWhitespace.ceil().max(1.);
        let h = metrics.height.ceil().max(1.);
        if !w.is_finite() || !h.is_finite() || w > i32::MAX as f32 || h > i32::MAX as f32 {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "文字尺寸过大",
            ));
        }
        let origin = if let Some((x, y)) = center {
            PixelPoint {
                x: (x - f64::from(w) / 2.).round() as i32,
                y: (y - f64::from(h) / 2.).round() as i32,
            }
        } else {
            origin
        };
        let right = origin.x.checked_add(w as i32).ok_or_else(|| {
            windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "文字范围溢出")
        })?;
        let bottom = origin.y.checked_add(h as i32).ok_or_else(|| {
            windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "文字范围溢出")
        })?;
        Ok(PixelRect {
            left: origin.x,
            top: origin.y,
            right,
            bottom,
        })
    }
    pub fn parameter_at(&self, layout: &Layout, p: PixelPoint) -> Option<usize> {
        let r = layout.parameters?;
        if !contains(r, p) {
            return None;
        }
        let x = (p.x - r.left) as f32 / layout.parameter_scale;
        if (COLOR_START..COLOR_START + 8. * COLOR_STEP).contains(&x) {
            Some(((x - COLOR_START) / COLOR_STEP) as usize)
        } else if layout.parameter_slider().is_some_and(|r| contains(r, p)) {
            Some(8)
        } else if layout.parameter_edit().is_some_and(|r| contains(r, p)) {
            Some(9)
        } else {
            None
        }
    }
    fn paint_parameters(&self, layout: &Layout, editor: &Editor, focus: usize) {
        let Some(bounds) = layout.parameters else {
            return;
        };
        let r = self.local(bounds);
        let s = layout.parameter_scale;
        let dark = self.dark;
        self.panel(r, BAR_RADIUS * s);
        // SAFETY: all drawing objects are owned by Canvas on the UI thread.
        unsafe {
            for (i, rgb) in ANNOTATION_COLORS.iter().enumerate() {
                let x = r.left + (COLOR_START + (i as f32 + 0.5) * COLOR_STEP) * s;
                let y = (r.top + r.bottom) / 2.;
                self.brush.SetColor(&color(rgb[0], rgb[1], rgb[2], 1.));
                let circle = D2D1_ELLIPSE {
                    point: Vector2 { X: x, Y: y },
                    radiusX: 8. * s,
                    radiusY: 8. * s,
                };
                self.target.FillEllipse(&circle, &self.brush);
                if focus == 11 + i {
                    let rect = D2D_RECT_F {
                        left: x - 13. * s,
                        top: y - 13. * s,
                        right: x + 13. * s,
                        bottom: y + 13. * s,
                    };
                    self.outline(rect, 5. * s, 1.5 * s, tone(palette::accent(dark), 1.));
                }
                self.brush.SetColor(&if *rgb == editor.style().color {
                    tone(palette::accent(self.dark), 1.)
                } else {
                    tone(palette::muted(dark), 0.68)
                });
                self.target.DrawEllipse(
                    &D2D1_ELLIPSE {
                        radiusX: 10. * s,
                        radiusY: 10. * s,
                        ..circle
                    },
                    &self.brush,
                    if *rgb == editor.style().color { 2. } else { 1. },
                    None,
                );
            }
        }
    }
    fn paint_parameter_slider(
        &self,
        layout: &Layout,
        (parameter, value): (CaptureParameter, u16),
        focus: usize,
        editing: bool,
    ) {
        let Some((slider, edit)) = layout.parameter_slider().zip(layout.parameter_edit()) else {
            return;
        };
        let slider = self.local(slider);
        let edit = self.local(edit);
        let s = layout.parameter_scale;
        let text = if (s - layout.scale).abs() > 0.001 {
            self.parameter_text
                .as_ref()
                .map_or(&self.text, |(_, text)| text)
        } else {
            &self.text
        };
        let dark = self.dark;
        let accent = tone(palette::accent(dark), 1.);
        let divider = tone(palette::divider(dark), 1.);
        let foreground = tone(palette::foreground(dark), 1.);
        let y = (slider.top + slider.bottom) / 2.;
        let left = slider.left + 6. * s;
        let right = slider.right - 6. * s;
        let thumb = left + parameter_control::fraction(parameter, value) * (right - left);
        let track = D2D_RECT_F {
            left,
            top: y - 1.5 * s,
            right,
            bottom: y + 1.5 * s,
        };
        self.round(track, 1.5 * s, divider);
        if thumb > left {
            self.round(
                D2D_RECT_F {
                    right: thumb,
                    ..track
                },
                1.5 * s,
                accent,
            );
        }
        self.round(
            D2D_RECT_F {
                left: thumb - 5.5 * s,
                top: y - 5.5 * s,
                right: thumb + 5.5 * s,
                bottom: y + 5.5 * s,
            },
            5.5 * s,
            accent,
        );
        if focus == 19 {
            self.outline(slider, 5. * s, 1.5 * s, accent);
        }
        self.fill(
            D2D_RECT_F {
                left: slider.left - 53. * s,
                right: slider.left - 52. * s,
                top: slider.top + 1. * s,
                bottom: slider.bottom - 1. * s,
            },
            divider,
        );
        self.label_with_format(
            match parameter {
                CaptureParameter::Stroke(_) => "线宽",
                CaptureParameter::TextSize => "字号",
                CaptureParameter::CornerRadius => "圆角",
            },
            D2D_RECT_F {
                left: slider.left - 47. * s,
                right: slider.left - 8. * s,
                ..slider
            },
            foreground,
            text,
        );
        self.label_with_format(
            "px",
            D2D_RECT_F {
                left: edit.right + 2. * s,
                right: edit.right + 26. * s,
                ..edit
            },
            tone(palette::muted(dark), 1.),
            text,
        );
        self.round(edit, 6. * s, tone(palette::background(dark), 1.));
        self.outline(
            edit,
            6. * s,
            s,
            if editing || focus == 20 {
                accent
            } else {
                divider
            },
        );
        if !editing {
            self.label_with_format(&value.to_string(), edit, foreground, text);
        }
    }
    fn paint_annotations(
        &self,
        editor: &Editor,
        clip: Option<PixelRect>,
        rounded: Option<&ID2D1Geometry>,
        editing: Option<usize>,
        prepared: (
            &[Option<IDWriteTextLayout>],
            &super::annotation_render::Geometry,
        ),
    ) {
        let Some(clip) = clip.filter(|_| !editor.objects().is_empty() || editor.active()) else {
            return;
        };
        let point = |p: PixelPoint| Vector2 {
            X: (i64::from(p.x) - i64::from(self.source.bounds().left)) as f32,
            Y: (i64::from(p.y) - i64::from(self.source.bounds().top)) as f32,
        };
        // SAFETY: retained UI-thread resources; clip is balanced before returning.
        unsafe {
            let bounds = self.local(clip);
            if let Some(mask) = rounded {
                let params = D2D1_LAYER_PARAMETERS1 {
                    contentBounds: bounds,
                    geometricMask: std::mem::ManuallyDrop::new(Some(mask.clone())),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: 1.,
                    ..Default::default()
                };
                self.context.PushLayer(&params, None::<&ID2D1Layer>);
                drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
            } else {
                self.target
                    .PushAxisAlignedClip(&bounds, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            }
            let selected = editor
                .selected
                .filter(|i| !editor.active() && editing != Some(*i));
            if let Some(i) = selected {
                let object = &editor.objects()[i];
                if let Shape::Pen(points) = &object.shape {
                    self.brush.SetColor(&tone(palette::accent(self.dark), 0.45));
                    prepared.1.stroke(
                        &self.target,
                        &self.brush,
                        self.bounds(),
                        i,
                        points,
                        object.style.width + 3. * self.scale,
                    );
                }
            }
            super::annotation_render::draw(
                &self.target,
                &self.brush,
                self.source.bounds(),
                editor,
                prepared,
                editing,
            );
            if let Some(object) = selected.and_then(|i| editor.objects().get(i)) {
                for p in object.handles() {
                    let p = point(p);
                    let r = D2D_RECT_F {
                        left: p.X - 3. * self.scale,
                        top: p.Y - 3. * self.scale,
                        right: p.X + 3. * self.scale,
                        bottom: p.Y + 3. * self.scale,
                    };
                    if matches!(object.shape, Shape::Arrow(..)) {
                        let circle = D2D1_ELLIPSE {
                            point: p,
                            radiusX: 3. * self.scale,
                            radiusY: 3. * self.scale,
                        };
                        self.brush.SetColor(&tone(palette::accent(self.dark), 1.));
                        self.target.FillEllipse(&circle, &self.brush);
                        self.brush
                            .SetColor(&tone(palette::on_accent(self.dark), 1.));
                        self.target.DrawEllipse(&circle, &self.brush, 1., None);
                    } else {
                        self.fill(r, tone(palette::accent(self.dark), 1.));
                        self.brush
                            .SetColor(&tone(palette::on_accent(self.dark), 1.));
                        self.target.DrawRectangle(&r, &self.brush, 1., None);
                    }
                }
            }
            if rounded.is_some() {
                self.target.PopLayer();
            } else {
                self.target.PopAxisAlignedClip();
            }
            if let Some(object) = editor
                .selected
                .filter(|i| editing != Some(*i))
                .and_then(|i| editor.objects().get(i))
            {
                if let Shape::Text { bounds, angle, .. } = object.shape {
                    self.paint_text_frame(bounds, angle);
                }
            }
        }
    }
    fn paint_text_frame(&self, bounds: PixelRect, angle: f64) {
        let origin = self.bounds();
        let frame = text_frame(bounds, self.scale);
        let (cx, _) = text_center(bounds);
        let (x, y) = rotate_point(bounds, angle, cx, f64::from(frame.top));
        let local = |x: f64, y: f64| Vector2 {
            X: (x - f64::from(origin.left)) as f32,
            Y: (y - f64::from(origin.top)) as f32,
        };
        let handle = text_rotation_handle(bounds, angle, self.scale);
        let handle = local(f64::from(handle.x), f64::from(handle.y));
        let circle = D2D1_ELLIPSE {
            point: handle,
            radiusX: 5. * self.scale,
            radiusY: 5. * self.scale,
        };
        let accent = tone(palette::accent(self.dark), 1.);
        // SAFETY: called during Canvas drawing on the UI thread. Restore the transform before
        // drawing the handle in canvas coordinates.
        unsafe {
            self.brush.SetColor(&accent);
            let mut prior = Matrix3x2::default();
            self.target.GetTransform(&mut prior);
            self.target.SetTransform(
                &(super::annotation_render::text_transform(bounds, origin, angle) * prior),
            );
            self.target
                .DrawRectangle(&self.local(frame), &self.brush, 1., None);
            self.target.SetTransform(&prior);
            self.target
                .DrawLine(local(x, y), handle, &self.brush, 1., None);
            self.brush
                .SetColor(&tone(palette::background(self.dark), 1.));
            self.target.FillEllipse(&circle, &self.brush);
            self.brush.SetColor(&accent);
            self.target.DrawEllipse(&circle, &self.brush, 1., None);
        }
    }
    fn local(&self, r: PixelRect) -> D2D_RECT_F {
        D2D_RECT_F {
            left: (i64::from(r.left) - i64::from(self.source.bounds().left)) as f32,
            top: (i64::from(r.top) - i64::from(self.source.bounds().top)) as f32,
            right: (i64::from(r.right) - i64::from(self.source.bounds().left)) as f32,
            bottom: (i64::from(r.bottom) - i64::from(self.source.bounds().top)) as f32,
        }
    }
    fn fill(&self, r: D2D_RECT_F, c: D2D1_COLOR_F) {
        // SAFETY: Canvas owns UI-thread-only target and brush.
        unsafe {
            self.brush.SetColor(&c);
            self.target.FillRectangle(&r, &self.brush);
        }
    }
    fn round(&self, r: D2D_RECT_F, radius: f32, c: D2D1_COLOR_F) {
        // SAFETY: Canvas owns UI-thread-only target and brush.
        unsafe {
            self.brush.SetColor(&c);
            self.target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: r,
                    radiusX: radius,
                    radiusY: radius,
                },
                &self.brush,
            );
        }
    }
    fn panel(&self, r: D2D_RECT_F, radius: f32) {
        self.round(r, radius, tone(palette::background(self.dark), 1.));
        self.outline(r, radius, 1., tone(palette::divider(self.dark), 1.));
    }
    fn outline(&self, r: D2D_RECT_F, radius: f32, width: f32, c: D2D1_COLOR_F) {
        // SAFETY: the target and brush are retained by the canvas during this draw.
        unsafe {
            self.brush.SetColor(&c);
            if radius == 0. {
                self.target.DrawRectangle(&r, &self.brush, width, None);
            } else {
                self.target.DrawRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: r,
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &self.brush,
                    width,
                    None,
                );
            }
        }
    }
    fn label(&self, text: &str, r: D2D_RECT_F, c: D2D1_COLOR_F) {
        self.label_with_format(text, r, c, &self.text);
    }
    fn label_with_format(
        &self,
        text: &str,
        r: D2D_RECT_F,
        c: D2D1_COLOR_F,
        format: &IDWriteTextFormat,
    ) {
        let wide: Vec<_> = text.encode_utf16().collect(); // SAFETY: text slice and resources live through synchronous DrawText.
        unsafe {
            self.brush.SetColor(&c);
            self.target.DrawText(
                &wide,
                format,
                &r,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}
pub(super) fn format(factory: &IDWriteFactory, scale: f32) -> Result<IDWriteTextFormat> {
    // SAFETY: formats are created and retained on this UI thread; static system font names are NUL terminated.
    unsafe {
        let text = factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            14. * scale,
            w!("zh-CN"),
        )?;
        text.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        text.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        Ok(text)
    }
}
fn tone(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    color((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha)
}

fn color(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.,
        g: g as f32 / 255.,
        b: b as f32 / 255.,
        a,
    }
}

pub const ANNOTATION_COLORS: [[u8; 3]; 8] = [
    [255, 59, 64],
    [255, 149, 0],
    [255, 204, 0],
    [52, 199, 89],
    [0, 122, 255],
    [175, 82, 222],
    [255, 255, 255],
    [20, 20, 20],
];

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: UI-thread surface, outside BeginDraw; detach its back buffer before releasing the swap chain.
        unsafe {
            self.context.SetTarget(None::<&ID2D1Image>);
            let _ = self.context.Flush(None, None);
        }
    }
}
