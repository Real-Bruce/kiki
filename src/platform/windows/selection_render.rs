use super::{
    parameter_control, text_input::TextInput, toolbar_icons::ToolbarIcons, ui_palette as palette,
};
use crate::core::{
    annotation::{
        Editor, Shape, Tool, rotate_point, text_center, text_frame, text_rotation_handle,
    },
    config::CaptureParameter,
    display::DisplayInfo,
    geometry::PixelRect,
    image::Image,
    selection::{PixelPoint, Selection, contains, fit_panel, selection_label, toolbar},
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

const BAR_HEIGHT: f32 = 48.;
const PARAM_GAP: f32 = 8.;
const PARAM_HEIGHT: f32 = 42.;
// 01-capture.png: main panel x=353..1233, y=723..790. Coordinates are
// offsets from the panel's left edge, normalized to the 48 DIP panel height.
const REFERENCE_SCALE: f32 = BAR_HEIGHT / 68.;
const BAR_WIDTH: f32 = 880. * REFERENCE_SCALE;
const BUTTON_CENTERS: [f32; 11] = [
    46., 111., 177., 243., 309., 408., 472., 571., 636., 737., 842.,
];
const DIVIDER_CENTERS: [f32; 3] = [358., 522., 809.];
const BUTTON_WIDTH: f32 = 42.;
const COPY_WIDTH: f32 = 118. * REFERENCE_SCALE;
const BUTTON_HEIGHT: f32 = 36.;
const BUTTON_FACE_HEIGHT: f32 = 34.;
const BUTTON_RADIUS: f32 = 7.;
const BAR_RADIUS: f32 = 10.;
const PARAM_WIDTH: f32 = 506.;
const COLOR_START: f32 = 10.;
const COLOR_STEP: f32 = 30.;

pub struct Layout {
    pub bounds: PixelRect,
    pub toolbar: PixelRect,
    pub parameters: Option<PixelRect>,
    pub screen: PixelRect,
    pub buttons: [PixelRect; 11],
    pub scale: f32,
}
impl Layout {
    fn parameter_box(&self, left: f32, right: f32, inset_y: f32) -> Option<PixelRect> {
        let r = self.parameters?;
        let px = |value: f32| (value * self.scale).round() as i32;
        Some(PixelRect {
            left: r.left + px(left),
            right: r.left + px(right),
            top: r.top + px(inset_y),
            bottom: r.bottom - px(inset_y),
        })
    }
    pub fn parameter_slider(&self) -> Option<PixelRect> {
        self.parameter_box(305., 418., 8.)
    }
    pub fn parameter_edit(&self) -> Option<PixelRect> {
        self.parameter_box(430., 477., 7.)
    }
}
pub struct PaintInput<'a> {
    pub selection: &'a Selection,
    pub editor: &'a Editor,
    pub text_input: Option<&'a TextInput>,
    pub busy: bool,
    pub hover: Option<PixelRect>,
    pub screens: &'a [DisplayInfo],
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
                icons,
                scale: 1.,
                dark: system_dark(),
                source: std::sync::Arc::new(image),
            })
        }
    }
    pub fn layout(&self, rect: PixelRect, screens: &[DisplayInfo], tool: Tool) -> Layout {
        let (screen, preferred) = self.display(rect, screens);
        let parameters = tool != Tool::Select;
        let height = BAR_HEIGHT
            + if parameters {
                PARAM_GAP + PARAM_HEIGHT
            } else {
                0.
            };
        let scale = preferred
            .min((i64::from(screen.right) - i64::from(screen.left)) as f32 / BAR_WIDTH)
            .min((i64::from(screen.bottom) - i64::from(screen.top)) as f32 / height);
        let bounds = toolbar(
            rect,
            screen,
            (BAR_WIDTH * scale).round() as i32,
            (height * scale).round() as i32,
            (10. * scale).round() as i32,
        );
        let x = |offset: f32| bounds.left.saturating_add((offset * scale).round() as i32);
        let y = |offset: f32| bounds.top.saturating_add((offset * scale).round() as i32);
        let buttons = std::array::from_fn(|i| {
            let center = BUTTON_CENTERS[i] * REFERENCE_SCALE;
            let half = if i == 9 { COPY_WIDTH } else { BUTTON_WIDTH } / 2.;
            PixelRect {
                left: x(center - half),
                top: y((BAR_HEIGHT - BUTTON_HEIGHT) / 2.),
                right: x(center + half),
                bottom: y((BAR_HEIGHT + BUTTON_HEIGHT) / 2.),
            }
        });
        Layout {
            toolbar: PixelRect {
                bottom: y(BAR_HEIGHT),
                ..bounds
            },
            parameters: parameters.then(|| PixelRect {
                left: x((BAR_WIDTH - PARAM_WIDTH) / 2.),
                top: y(BAR_HEIGHT + PARAM_GAP),
                right: x((BAR_WIDTH + PARAM_WIDTH) / 2.),
                bottom: bounds.bottom,
            }),
            bounds,
            screen,
            buttons,
            scale,
        }
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
            text_input,
            busy,
            hover,
            screens,
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
            .map(|r| self.layout(r, screens, editor.tool));
        let anchor = selection.rect.or(hover).unwrap_or(PixelRect {
            left: pointer.x,
            top: pointer.y,
            right: pointer.x,
            bottom: pointer.y,
        });
        let (screen, preferred) = self.display(anchor, screens);
        let scale = layout.as_ref().map_or(preferred, |l| l.scale);
        let text_layouts = super::annotation_render::layouts(&self.dw, editor)?;
        let geometry = super::annotation_render::geometry(&self.factory, self.bounds(), editor)?;
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
            let selected = selection.rect.or(hover);
            if let Some(rect) = selected {
                let r = self.local(rect);
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
                    self.fill(mask, tone(palette::foreground(false), 0.50));
                }
                self.outline(r, 0., 2.5, tone(palette::on_accent(self.dark), 1.));
                self.outline(r, 0., 1.5, tone(palette::accent(self.dark), 1.));
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
            self.paint_annotations(editor, selection.rect, editing, (&text_layouts, &geometry));
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
    fn paint_toolbar(
        &self,
        layout: &Layout,
        editor: &Editor,
        pointer: PixelPoint,
        focus: usize,
        pressed: Option<usize>,
        busy: bool,
    ) {
        let r = self.local(layout.toolbar);
        let s = layout.scale;
        let dark = self.dark;
        self.panel(r, BAR_RADIUS * s);
        for center in DIVIDER_CENTERS {
            let x = r.left + center * REFERENCE_SCALE * s;
            self.fill(
                D2D_RECT_F {
                    left: x - 0.45 * s,
                    top: r.top + 14. * s,
                    right: x + 0.45 * s,
                    bottom: r.bottom - 14. * s,
                },
                tone(palette::divider(dark), 1.),
            );
        }
        let names = [
            "选择",
            "矩形",
            "箭头",
            "画笔",
            "文字",
            "撤销（Ctrl+Z）",
            "重做（Ctrl+Y）",
            "贴图",
            "保存（Ctrl+S）",
            "复制",
            "取消（Esc）",
        ];
        let mut hovered = None;
        for (i, button) in layout.buttons.iter().enumerate() {
            let b = self.local(*button);
            let inset = (BUTTON_HEIGHT - BUTTON_FACE_HEIGHT) * s / 2.;
            let face = D2D_RECT_F {
                top: b.top + inset,
                bottom: b.bottom - inset,
                ..b
            };
            let active = !busy && editor.enabled(i);
            let over = contains(*button, pointer);
            if over {
                hovered = Some(i);
            }
            let selected = i == editor.tool.button();
            let down = over && active && pressed == Some(i);
            let background = match (i == 9, active) {
                (true, false) => Some(palette::selected(dark)),
                (true, true) if down => Some(palette::accent_pressed(dark)),
                (true, true) if over => Some(palette::accent_hover(dark)),
                (true, true) => Some(palette::accent(dark)),
                _ if down => Some(palette::pressed(dark)),
                _ if over && active => Some(palette::hover(dark)),
                _ if selected => Some(palette::selected(dark)),
                _ => None,
            };
            if let Some(background) = background {
                self.round(face, BUTTON_RADIUS * s, tone(background, 1.));
            }
            let foreground = if !active {
                tone(palette::muted(dark), 0.58)
            } else if i == 9 {
                tone(palette::on_accent(dark), 1.)
            } else if selected {
                tone(palette::accent(dark), 1.)
            } else {
                tone(palette::foreground(dark), 1.)
            };
            self.icon(i, b, foreground, s);
            if focus == i {
                self.outline(b, 6. * s, 1.5 * s, tone(palette::accent(dark), 1.));
            }
        }
        if let Some(i) = hovered.filter(|_| super::preferences::hints()) {
            let b = layout.buttons[i];
            let width = ((i64::from(b.right) - i64::from(b.left)) as f32 + 48. * s).round() as i32;
            let t = self.local(fit_panel(
                PixelPoint {
                    x: b.left.saturating_sub((24. * s).round() as i32),
                    y: layout.bounds.top.saturating_sub((34. * s).round() as i32),
                },
                layout.screen,
                width,
                (28. * s).round() as i32,
            ));
            self.panel(t, 6. * s);
            self.label(names[i], t, tone(palette::foreground(dark), 1.));
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
        let x = (p.x - r.left) as f32 / layout.scale;
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
        let s = layout.scale;
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
        let s = layout.scale;
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
        self.label(
            match parameter {
                CaptureParameter::Stroke(_) => "线宽",
                CaptureParameter::TextSize => "字号",
            },
            D2D_RECT_F {
                left: slider.left - 47. * s,
                right: slider.left - 8. * s,
                ..slider
            },
            foreground,
        );
        self.label(
            "px",
            D2D_RECT_F {
                left: edit.right + 2. * s,
                right: edit.right + 26. * s,
                ..edit
            },
            tone(palette::muted(dark), 1.),
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
            self.label(&value.to_string(), edit, foreground);
        }
    }
    fn paint_annotations(
        &self,
        editor: &Editor,
        clip: Option<PixelRect>,
        editing: Option<usize>,
        prepared: (
            &[Option<IDWriteTextLayout>],
            &super::annotation_render::Geometry,
        ),
    ) {
        let Some(clip) = clip else {
            return;
        };
        let point = |p: PixelPoint| Vector2 {
            X: (i64::from(p.x) - i64::from(self.source.bounds().left)) as f32,
            Y: (i64::from(p.y) - i64::from(self.source.bounds().top)) as f32,
        };
        // SAFETY: retained UI-thread resources; clip is balanced before returning.
        unsafe {
            self.target
                .PushAxisAlignedClip(&self.local(clip), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
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
            self.target.PopAxisAlignedClip();
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
    fn icon(&self, index: usize, r: D2D_RECT_F, c: D2D1_COLOR_F, scale: f32) {
        let center = Vector2 {
            X: (r.left + r.right) / 2.,
            Y: (r.top + r.bottom) / 2.,
        };
        // SAFETY: the brush is owned by the active canvas on the render thread.
        unsafe {
            self.brush.SetColor(&c);
        }
        let icon_center = if index == 9 {
            Vector2 {
                X: center.X - 18. * scale,
                ..center
            }
        } else {
            center
        };
        self.icons
            .draw(&self.target, &self.brush, index, icon_center, scale);
        if index == 9 {
            self.label(
                "复制",
                D2D_RECT_F {
                    left: center.X - 14. * scale,
                    ..r
                },
                c,
            );
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
        let wide: Vec<_> = text.encode_utf16().collect(); // SAFETY: text slice and resources live through synchronous DrawText.
        unsafe {
            self.brush.SetColor(&c);
            self.target.DrawText(
                &wide,
                &self.text,
                &r,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}
fn format(factory: &IDWriteFactory, scale: f32) -> Result<IDWriteTextFormat> {
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
