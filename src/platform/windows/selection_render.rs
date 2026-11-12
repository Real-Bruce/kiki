use super::{
    capture_toolbar::{self, Layout},
    color_picker::Readout,
    long_capture::{self, Capture, Document, Panel},
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
    pub capture_tools: [bool; Tool::TOOLBAR.len()],
    pub pointer: PixelPoint,
    pub focus: usize,
    pub pressed: Option<usize>,
    pub editing_parameter: bool,
    pub parameter: Option<(CaptureParameter, u16)>,
    pub message: Option<&'static str>,
    pub picker: Option<Readout>,
}
pub struct Canvas {
    target: ID2D1RenderTarget,
    factory: ID2D1Factory,
    gdi: ID2D1GdiInteropRenderTarget,
    context: ID2D1DeviceContext,
    swap: windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    bitmap: Option<ID2D1Bitmap>,
    window_bounds: PixelRect,
    band_bitmap: Option<(PixelRect, ID2D1Bitmap)>,
    band_mosaics: Option<PixelRect>,
    mosaics: Vec<(PixelRect, u16)>,
    mosaic_bitmap: Option<(PixelRect, ID2D1Bitmap)>,
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
    pub fn window_bounds(&self) -> PixelRect {
        self.window_bounds
    }
    pub fn set_document(&mut self, image: Image) {
        self.source = std::sync::Arc::new(image);
        self.bitmap = None;
        self.band_bitmap = None;
        self.mosaic_bitmap = None;
        self.mosaics.clear();
        self.band_mosaics = None;
    }
    pub fn rgb_at(&self, point: PixelPoint) -> Option<[u8; 3]> {
        self.source.rgb_at(point)
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
                bitmap: Some(bitmap),
                window_bounds: image.bounds(),
                band_bitmap: None,
                band_mosaics: None,
                mosaics: Vec::new(),
                mosaic_bitmap: None,
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
        capture_tools: [bool; Tool::TOOLBAR.len()],
    ) -> Layout {
        let (screen, preferred) = self.display(rect, screens);
        Layout::new(rect, screen, preferred, tool, capture_tools)
    }

    fn prepare_mosaics(&mut self, editor: &Editor, band: Option<PixelRect>) -> Result<()> {
        if self.band_mosaics == band && editor.mosaics().eq(self.mosaics.iter().copied()) {
            return Ok(());
        }
        self.mosaic_bitmap = None;
        self.mosaics.clear();
        let coverage = editor
            .mosaics()
            .filter_map(|(bounds, _)| bounds.intersection(band.unwrap_or_else(|| self.bounds())))
            .reduce(|a, b| PixelRect {
                left: a.left.min(b.left),
                top: a.top.min(b.top),
                right: a.right.max(b.right),
                bottom: a.bottom.max(b.bottom),
            });
        if let Some(bounds) = coverage {
            let fail =
                |message| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, message);
            let (width, height) = bounds.size().ok_or_else(|| fail("无效马赛克范围"))?;
            let mut overlay = Image::new(bounds).map_err(|message| {
                windows::core::Error::new(windows::Win32::Foundation::E_FAIL, message)
            })?;
            for (area, size) in editor.mosaics() {
                overlay.pixelate(&self.source, area, size);
            }
            overlay.premultiply();
            // SAFETY: CreateBitmap copies the checked, tightly packed pixels before returning.
            let bitmap = unsafe {
                self.target.CreateBitmap(
                    D2D_SIZE_U { width, height },
                    Some(overlay.pixels().as_ptr().cast()),
                    overlay.stride(),
                    &D2D1_BITMAP_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.,
                        dpiY: 96.,
                    },
                )?
            };
            self.mosaic_bitmap = Some((bounds, bitmap));
        }
        self.mosaics.extend(editor.mosaics());
        self.band_mosaics = band;
        Ok(())
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
            capture_tools,
            pointer,
            focus,
            pressed,
            editing_parameter,
            parameter,
            message,
            picker,
        } = input;
        let editing = text_input.and_then(|input| input.index);
        let text_active = text_input.is_some();
        let layout = selection
            .rect
            .filter(|_| !selection.active())
            .map(|r| self.layout(r, screens, editor.tool, capture_tools));
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
        self.prepare_mosaics(editor, None)?;
        let Some(bitmap) = self.bitmap.as_ref() else {
            return Ok(());
        };
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
                bitmap,
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
            if let Some(picker) = picker {
                self.paint_picker(picker, pointer, screens, layout.as_ref());
            }
            if let Some(layout) = layout {
                self.paint_toolbar(&layout, editor, pointer, focus, pressed, busy);
                self.paint_parameters(&layout, editor, focus);
                if let Some(parameter) = parameter {
                    self.paint_parameter_slider(&layout, parameter, focus, editing_parameter);
                }
            }
            if busy || message.is_some() || super::preferences::hints() {
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
                    } else if let Some(message) = message {
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
    fn paint_picker(
        &self,
        readout: Readout,
        pointer: PixelPoint,
        screens: &[DisplayInfo],
        layout: Option<&Layout>,
    ) {
        let distance = |value: i32, min: i32, max: i32| {
            i64::from(value.abs_diff(value.max(min).min(max.saturating_sub(1))))
        };
        let screen = screens
            .iter()
            .min_by_key(|screen| {
                let r = screen.bounds;
                distance(pointer.x, r.left, r.right) + distance(pointer.y, r.top, r.bottom)
            })
            .map_or(self.bounds(), |screen| screen.bounds);
        let s = self.scale;
        let width = (222. * s).round() as i32;
        let height = (65. * s).round() as i32;
        let gap = (14. * s).round() as i32;
        let left = pointer.x.saturating_sub(width).saturating_sub(gap);
        let right = pointer.x.saturating_add(gap);
        let top = pointer.y.saturating_sub(height).saturating_sub(gap);
        let bottom = pointer.y.saturating_add(gap);
        let positions = [(right, bottom), (left, bottom), (right, top), (left, top)]
            .map(|(x, y)| fit_panel(PixelPoint { x, y }, screen, width, height));
        let area = positions
            .into_iter()
            .find(|r| {
                !contains(*r, pointer)
                    && layout.is_none_or(|layout| layout.bounds.intersection(*r).is_none())
            })
            .unwrap_or(positions[0]);
        let panel = self.screen_local(area);
        let dark = self.dark;
        self.panel(panel, 9. * s);
        let foreground = tone(palette::foreground(dark), 1.);
        let mut label = D2D_RECT_F {
            left: panel.left + 9. * s,
            right: panel.right - 9. * s,
            top: panel.top + 5. * s,
            bottom: panel.top + 37. * s,
        };
        if let Some([r, g, b]) = readout.rgb {
            let swatch = D2D_RECT_F {
                left: panel.left + 13. * s,
                right: panel.left + 36. * s,
                top: panel.top + 10. * s,
                bottom: panel.top + 33. * s,
            };
            self.round(swatch, 4. * s, color(r, g, b, 1.));
            self.outline(swatch, 4. * s, s, tone(palette::divider(dark), 1.));
            label.left = swatch.right + 4. * s;
            self.label(&readout.format.encode([r, g, b]), label, foreground);
        } else {
            self.label("无像素", label, foreground);
        }
        self.label(
            readout.notice.map_or_else(
                || {
                    if readout.rgb.is_some() {
                        readout.format.hint()
                    } else {
                        "移动到有像素的区域"
                    }
                },
                |notice| notice.label(),
            ),
            D2D_RECT_F {
                top: panel.top + 36. * s,
                bottom: panel.bottom - 4. * s,
                left: panel.left + 8. * s,
                right: panel.right - 8. * s,
            },
            tone(palette::muted(dark), 1.),
        );
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
        let desktop = self.window_bounds;
        let hover = layout.button_at(pointer);
        BarPaint {
            target: &self.target,
            brush: &self.brush,
            icons: &self.icons,
            dark,
        }
        .draw(
            layout,
            PixelPoint {
                x: desktop.left,
                y: desktop.top,
            },
            BarState {
                selected: capture_toolbar::tool_button(editor.tool),
                hover,
                pressed,
                focus,
                can_undo: editor.can_undo(),
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
            let t = self.screen_local(fit_panel(
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
        if layout.color_parameters() && (COLOR_START..COLOR_START + 8. * COLOR_STEP).contains(&x) {
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
        let r = self.screen_local(bounds);
        let s = layout.parameter_scale;
        let dark = self.dark;
        self.panel(r, BAR_RADIUS * s);
        if editor.tool == Tool::Sequence {
            let text = self
                .parameter_text
                .as_ref()
                .map_or(&self.text, |(_, text)| text);
            self.label_with_format(
                &format!("下一编号 {}", editor.next_sequence()),
                D2D_RECT_F {
                    left: r.left + 8. * s,
                    right: r.left + 108. * s,
                    ..r
                },
                tone(palette::foreground(dark), 1.),
                text,
            );
        }
        if !editor.tool.has_color() {
            return;
        }
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
        let slider = self.screen_local(slider);
        let edit = self.screen_local(edit);
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
        if layout.color_parameters() {
            self.fill(
                D2D_RECT_F {
                    left: slider.left - 53. * s,
                    right: slider.left - 52. * s,
                    top: slider.top + 1. * s,
                    bottom: slider.bottom - 1. * s,
                },
                divider,
            );
        }
        self.label_with_format(
            match parameter {
                CaptureParameter::Stroke(_) => "线宽",
                CaptureParameter::TextSize => "字号",
                CaptureParameter::CornerRadius => "圆角",
                CaptureParameter::MosaicSize => "颗粒大小",
                CaptureParameter::SequenceSize => "直径",
            },
            D2D_RECT_F {
                left: slider.left
                    - if matches!(parameter, CaptureParameter::MosaicSize) {
                        80.
                    } else {
                        47.
                    } * s,
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
            if let Some((area, bitmap)) = &self.mosaic_bitmap {
                self.target.DrawBitmap(
                    bitmap,
                    Some(&self.local(*area)),
                    1.,
                    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                    None,
                );
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
                if let Shape::Mosaic { bounds, .. } = object.shape {
                    self.brush.SetColor(&tone(palette::accent(self.dark), 1.));
                    self.target
                        .DrawRectangle(&self.local(bounds), &self.brush, 1., None);
                }
                if let Shape::Sequence { center, diameter } = object.shape {
                    let radius = f32::from(diameter) / 2. + 2. * self.scale;
                    self.brush.SetColor(&tone(palette::accent(self.dark), 1.));
                    self.target.DrawEllipse(
                        &D2D1_ELLIPSE {
                            point: point(center),
                            radiusX: radius,
                            radiusY: radius,
                        },
                        &self.brush,
                        self.scale,
                        None,
                    );
                }
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
        self.paint_text_frame_in(bounds, angle, self.bounds());
    }
    fn paint_text_frame_in(&self, bounds: PixelRect, angle: f64, origin: PixelRect) {
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
            self.target.DrawRectangle(
                &D2D_RECT_F {
                    left: (frame.left - origin.left) as f32,
                    top: (frame.top - origin.top) as f32,
                    right: (frame.right - origin.left) as f32,
                    bottom: (frame.bottom - origin.top) as f32,
                },
                &self.brush,
                1.,
                None,
            );
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
    fn screen_local(&self, r: PixelRect) -> D2D_RECT_F {
        let origin = self.window_bounds;
        D2D_RECT_F {
            left: (r.left - origin.left) as f32,
            top: (r.top - origin.top) as f32,
            right: (r.right - origin.left) as f32,
            bottom: (r.bottom - origin.top) as f32,
        }
    }

    fn image_bitmap(&self, image: &Image) -> Result<ID2D1Bitmap> {
        let (width, height) = image
            .bounds()
            .size()
            .ok_or_else(windows::core::Error::from_win32)?;
        // SAFETY: the UI-thread target copies the validated, tightly packed pixels during this call.
        unsafe {
            self.target.CreateBitmap(
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
            )
        }
    }

    pub fn paint_capture(&mut self, capture: &Capture) -> Result<()> {
        let scale = capture.screen.scale_percent as f32 / 100.;
        self.update_font(scale)?;
        let preview = capture
            .update
            .as_ref()
            .map(|u| self.image_bitmap(&u.thumbnail))
            .transpose()?;
        // SAFETY: the native window region exposes the shaded surround and preview panel.
        // The opening shows the real scrolling application. All drawing stays on the UI thread.
        unsafe {
            self.target.BeginDraw();
            self.target
                .Clear(Some(&tone(palette::background(self.dark), 1.)));
            if let Some(bitmap) = self.bitmap.as_ref() {
                self.target.DrawBitmap(
                    bitmap,
                    None,
                    1.,
                    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                    None,
                );
            }
            self.fill(
                self.screen_local(capture.screen.bounds),
                tone(palette::foreground(false), 0.50),
            );
            let panel = &capture.panel;
            self.outline(
                self.screen_local(capture.area),
                0.,
                4.,
                tone(palette::accent(self.dark), 1.),
            );
            self.long_panel(panel, "长截图预览");
            let foreground = tone(palette::foreground(self.dark), 1.);
            let status = PixelRect {
                top: panel.track.bottom,
                bottom: panel.finish.top - (22. * scale) as i32,
                left: panel.track.left,
                right: panel.track.right,
            };
            self.label(&capture.message, self.screen_local(status), foreground);
            if let Some((update, bitmap)) = capture.update.as_ref().zip(preview.as_ref()) {
                let area = self.screen_local(long_capture::thumbnail_bounds(
                    panel.track,
                    &update.thumbnail,
                ));
                let track = self.screen_local(panel.track);
                let (w, h) = update.thumbnail.bounds().size().unwrap_or((1, 1));
                let visible =
                    (track.bottom - track.top) * w as f32 / (track.right - track.left).max(1.);
                if let Some(offset) = capture.browse.filter(|_| visible < h as f32) {
                    let top = offset * (h as f32 - visible);
                    self.target.DrawBitmap(
                        bitmap,
                        Some(&track),
                        1.,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        Some(&D2D_RECT_F {
                            left: 0.,
                            top,
                            right: w as f32,
                            bottom: top + visible,
                        }),
                    );
                } else {
                    self.paint_thumbnail(bitmap, area, update.height, update.viewport);
                }
                let mut dimensions = self.screen_local(panel.finish);
                dimensions.right = self.screen_local(panel.cancel).right;
                dimensions.bottom = dimensions.top;
                dimensions.top -= 24. * scale;
                self.label(
                    &format!("{} × {} px", update.width, update.height),
                    dimensions,
                    foreground,
                );
            }
            let footer = self.screen_local(panel.finish);
            self.round(
                footer,
                5. * scale,
                tone(
                    palette::accent(self.dark),
                    if preview.is_some() { 1. } else { 0.35 },
                ),
            );
            self.label(
                if capture.requested {
                    "处理中…"
                } else {
                    "完成"
                },
                footer,
                tone(palette::on_accent(self.dark), 1.),
            );
            let cancel = self.screen_local(panel.cancel);
            self.outline(
                cancel,
                5. * scale,
                1.,
                tone(palette::divider(self.dark), 1.),
            );
            self.label("取消", cancel, foreground);
            self.target.EndDraw(None, None)?;
            self.swap
                .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0))
                .ok()
        }
    }

    fn update_font(&mut self, scale: f32) -> Result<()> {
        if (self.scale - scale).abs() > 0.001 {
            self.text = format(&self.dw, scale)?;
            self.scale = scale;
        }
        Ok(())
    }

    fn long_panel(&self, panel: &Panel, title: &str) {
        self.panel(self.screen_local(panel.bounds), 9. * self.scale);
        let label = PixelRect {
            bottom: panel.track.top,
            ..panel.bounds
        };
        self.label(
            title,
            self.screen_local(label),
            tone(palette::foreground(self.dark), 1.),
        );
    }

    fn paint_thumbnail(
        &self,
        bitmap: &ID2D1Bitmap,
        area: D2D_RECT_F,
        height: u32,
        viewport: (usize, usize),
    ) {
        let extent = area.bottom - area.top;
        let top = area.top + extent * viewport.0 as f32 / height as f32;
        let bottom = area.top + extent * (viewport.0 + viewport.1) as f32 / height as f32;
        // SAFETY: the thumbnail bitmap belongs to this drawing target.
        unsafe {
            self.target.DrawBitmap(
                bitmap,
                Some(&area),
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
        self.outline(
            D2D_RECT_F {
                top,
                bottom: bottom.min(area.bottom).max(top + 1.),
                ..area
            },
            0.,
            1.5,
            tone(palette::accent(self.dark), 1.),
        );
    }

    pub fn paint_document(
        &mut self,
        input: PaintInput<'_>,
        document: &Document,
        layout: &Layout,
    ) -> Result<()> {
        let view = document.view;
        let band = view.band();
        let scale = layout.scale;
        self.update_font(scale)?;
        if layout.parameters.is_some() {
            self.parameter_text = Some((
                layout.parameter_scale,
                format(&self.dw, layout.parameter_scale)?,
            ));
        }
        if self
            .band_bitmap
            .as_ref()
            .is_none_or(|(area, _)| *area != band)
        {
            let image = self
                .source
                .region(band)
                .map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, e))?;
            self.band_bitmap = Some((band, self.image_bitmap(&image)?));
        }
        let thumbnail = self.image_bitmap(&document.thumbnail)?;
        self.prepare_mosaics(input.editor, Some(band))?;
        let layouts = super::annotation_render::layouts(&self.dw, input.editor)?;
        let geometry =
            super::annotation_render::geometry(&self.factory, self.bounds(), input.editor)?;
        let radius = view.document.corner_radius(input.corner_radius);
        let rounded = (radius > 0.)
            .then(|| self.rounded_selection(view.document, radius))
            .transpose()?;
        let viewport = self.screen_local(view.canvas);
        let (_, height) = view.document.size().unwrap_or((1, 1));
        let editing = input.text_input.and_then(|draft| draft.index);
        // SAFETY: all resources belong to this drawing target and thread. Transforms and clips
        // are restored before drawing any controls in physical screen coordinates.
        unsafe {
            self.target.BeginDraw();
            self.target
                .Clear(Some(&tone(palette::background(self.dark), 1.)));
            self.target
                .PushAxisAlignedClip(&viewport, D2D1_ANTIALIAS_MODE_ALIASED);
            self.target.SetTransform(&Matrix3x2 {
                M11: view.scale,
                M22: view.scale,
                M12: 0.,
                M21: 0.,
                M31: viewport.left,
                M32: viewport.top - view.offset as f32 * view.scale,
            });
            if let Some((_, bitmap)) = self.band_bitmap.as_ref() {
                if let Some((mask, _)) = &rounded {
                    let params = D2D1_LAYER_PARAMETERS1 {
                        contentBounds: self.local(band),
                        geometricMask: std::mem::ManuallyDrop::new(Some(mask.clone())),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: Matrix3x2::identity(),
                        opacity: 1.,
                        ..Default::default()
                    };
                    self.context.PushLayer(&params, None::<&ID2D1Layer>);
                    drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
                }
                self.target.DrawBitmap(
                    bitmap,
                    Some(&self.local(band)),
                    1.,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
                self.paint_annotations(
                    input.editor,
                    Some(band),
                    None,
                    editing,
                    (&layouts, &geometry),
                );
                if rounded.is_some() {
                    self.target.PopLayer();
                }
            }
            self.target.SetTransform(&Matrix3x2::identity());
            self.target.PopAxisAlignedClip();
            let draft = input.text_input.map_or(Ok(()), |draft| {
                let clip = RECT {
                    left: viewport.left as i32,
                    top: viewport.top as i32,
                    right: viewport.right as i32,
                    bottom: viewport.bottom as i32,
                };
                let result = draft.draw(&self.gdi, clip, 0);
                self.paint_text_frame_in(draft.content_bounds(), 0., self.window_bounds);
                result
            });
            let top = document.panel.bounds.top;
            let caption = PixelRect {
                bottom: view.canvas.top,
                top,
                ..view.canvas
            };
            let (width, height) = view.document.size().unwrap_or((1, height));
            self.label(
                &format!("长图编辑　{width} × {height} px"),
                self.screen_local(caption),
                tone(palette::foreground(self.dark), 1.),
            );
            self.long_panel(&document.panel, "长图导航");
            self.paint_thumbnail(
                &thumbnail,
                self.screen_local(document.preview_bounds()),
                height,
                (
                    view.offset as usize,
                    view.visible_rows().min(height as i32) as usize,
                ),
            );
            let track = self.screen_local(document.scrollbar());
            let extent = track.bottom - track.top;
            self.round(track, 4. * scale, tone(palette::divider(self.dark), 1.));
            let top = track.top + extent * view.offset as f32 / height as f32;
            let bottom =
                (top + extent * view.visible_rows() as f32 / height as f32).min(track.bottom);
            self.round(
                D2D_RECT_F {
                    top,
                    bottom,
                    ..track
                },
                4. * scale,
                tone(palette::accent(self.dark), 1.),
            );
            if let Some(picker) = input.picker {
                self.paint_picker(picker, input.pointer, input.screens, Some(layout));
            }
            self.paint_toolbar(
                layout,
                input.editor,
                input.pointer,
                input.focus,
                input.pressed,
                input.busy,
            );
            self.paint_parameters(layout, input.editor, input.focus);
            if let Some(parameter) = input.parameter {
                self.paint_parameter_slider(
                    layout,
                    parameter,
                    input.focus,
                    input.editing_parameter,
                );
            }
            if input.busy || input.message.is_some() {
                let panel = PixelRect {
                    top: document.panel.track.bottom,
                    ..document.panel.bounds
                };
                let message = if input.busy {
                    "正在输出…"
                } else {
                    input.message.unwrap_or_default()
                };
                self.label(
                    message,
                    self.screen_local(panel),
                    tone(palette::foreground(self.dark), 1.),
                );
            }
            self.target.EndDraw(None, None)?;
            draft?;
            self.swap
                .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0))
                .ok()
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
