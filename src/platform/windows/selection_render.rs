use super::{text_input::TextInput, toolbar_icons::ToolbarIcons};
use crate::core::{
    annotation::{Editor, Shape, Tool, rotate_point, text_center},
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
const PARAM_WIDTH: f32 = 418.;
const COLOR_START: f32 = 10.;
const COLOR_STEP: f32 = 30.;
const VALUE_START: f32 = 258.;
const VALUE_STEP: f32 = 52.;

pub struct Layout {
    pub bounds: PixelRect,
    pub toolbar: PixelRect,
    pub parameters: Option<PixelRect>,
    pub screen: PixelRect,
    pub buttons: [PixelRect; 11],
    pub scale: f32,
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
    pub fn layout(&self, rect: PixelRect, screens: &[DisplayInfo], parameters: bool) -> Layout {
        let (screen, preferred) = self.display(rect, screens);
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
        } = input;
        let editing = text_input.and_then(|input| input.index);
        let text_active = text_input.is_some();
        let layout = selection
            .rect
            .filter(|_| !selection.active())
            .map(|r| self.layout(r, screens, editor.tool != Tool::Select));
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
                    self.fill(mask, color(24, 28, 36, 0.50));
                }
                self.brush.SetColor(&color(0, 120, 212, 1.));
                self.target.DrawRectangle(&r, &self.brush, 1.5, None);
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
                        self.fill(handle, color(255, 255, 255, 1.));
                        self.brush.SetColor(&color(0, 120, 212, 1.));
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
                    self.round(label, 5. * scale, color(0, 94, 176, 1.));
                    self.label(&format!("{rw} × {rh}"), label, color(255, 255, 255, 1.));
                }
            } else {
                self.fill(
                    D2D_RECT_F {
                        left: 0.,
                        top: 0.,
                        right: w,
                        bottom: h,
                    },
                    color(24, 28, 36, 0.50),
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
                let bounds = input.bounds();
                self.brush.SetColor(&color(0, 120, 212, 1.));
                self.target.DrawRectangle(
                    &D2D_RECT_F {
                        left: bounds.left as f32,
                        top: bounds.top as f32,
                        right: bounds.right as f32,
                        bottom: bounds.bottom as f32,
                    },
                    &self.brush,
                    1.,
                    None,
                );
                result
            } else {
                Ok(())
            };
            if let Some(layout) = layout {
                self.paint_toolbar(&layout, editor, pointer, focus, pressed, busy);
                self.paint_parameters(&layout, editor, focus);
            }
            if busy || super::preferences::hints() {
                let hint = D2D_RECT_F {
                    left: 16. * scale,
                    top: h - 44. * scale,
                    right: (if text_active { 480. } else { 350. }) * scale,
                    bottom: h - 12. * scale,
                };
                self.round(hint, 6. * scale, color(30, 36, 46, 0.88));
                self.label(
                    if busy {
                        "正在输出，请稍候…"
                    } else if text_active {
                        "Enter 换行　Ctrl+Enter 完成　Esc 取消文字"
                    } else {
                        "拖动框选　F 当前屏幕　Esc 取消"
                    },
                    hint,
                    color(255, 255, 255, 1.),
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
        // SAFETY: synchronous UI-thread drawing; all brushes and formats are retained by Canvas.
        unsafe {
            let r = self.local(layout.toolbar);
            let s = layout.scale;
            let panel = if self.dark {
                color(44, 47, 53, 0.97)
            } else {
                color(235, 240, 249, 0.97)
            };
            self.round(r, BAR_RADIUS * s, panel);
            self.brush.SetColor(&if self.dark {
                color(98, 103, 114, 1.)
            } else {
                color(255, 255, 255, 0.78)
            });
            self.target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: r,
                    radiusX: BAR_RADIUS * s,
                    radiusY: BAR_RADIUS * s,
                },
                &self.brush,
                1.,
                None,
            );
            for center in DIVIDER_CENTERS {
                let x = r.left + center * REFERENCE_SCALE * s;
                self.fill(
                    D2D_RECT_F {
                        left: x - 0.45 * s,
                        top: r.top + 14. * s,
                        right: x + 0.45 * s,
                        bottom: r.bottom - 14. * s,
                    },
                    if self.dark {
                        color(170, 180, 195, 0.55)
                    } else {
                        color(118, 132, 154, 0.55)
                    },
                );
            }
            let names = [
                "选择",
                "矩形",
                "箭头",
                "画笔",
                "文字",
                "撤销 · Ctrl+Z",
                "重做 · Ctrl+Y",
                "贴图",
                "保存 · Ctrl+S",
                "复制",
                "取消 · Esc",
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
                if i == editor.tool.button() {
                    self.round(
                        face,
                        BUTTON_RADIUS * s,
                        if self.dark {
                            color(41, 87, 129, 1.)
                        } else {
                            color(190, 216, 253, 1.)
                        },
                    );
                }
                if over && active {
                    self.round(
                        face,
                        BUTTON_RADIUS * s,
                        if self.dark {
                            color(72, 82, 95, 1.)
                        } else {
                            color(227, 232, 242, 1.)
                        },
                    );
                }
                if over && active && pressed == Some(i) {
                    self.round(
                        face,
                        BUTTON_RADIUS * s,
                        if self.dark {
                            color(35, 66, 96, 1.)
                        } else {
                            color(188, 217, 245, 1.)
                        },
                    );
                }
                if i == 9 {
                    let blue = if over && pressed == Some(i) {
                        (0, 85, 154)
                    } else if over {
                        (0, 105, 193)
                    } else {
                        (0, 110, 235)
                    };
                    self.round(
                        face,
                        BUTTON_RADIUS * s,
                        color(blue.0, blue.1, blue.2, if busy { 0.65 } else { 1. }),
                    );
                }
                let foreground = if i == editor.tool.button() {
                    if self.dark {
                        color(235, 246, 255, 1.)
                    } else {
                        color(18, 22, 28, 1.)
                    }
                } else if !active {
                    if self.dark {
                        color(153, 159, 170, 0.65)
                    } else {
                        color(126, 133, 145, 0.60)
                    }
                } else if self.dark {
                    color(244, 246, 250, 1.)
                } else {
                    color(18, 22, 28, 1.)
                };
                self.icon(i, b, foreground, s);
                if focus == i {
                    self.brush.SetColor(&color(0, 120, 212, 1.));
                    self.target.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: b,
                            radiusX: 6. * s,
                            radiusY: 6. * s,
                        },
                        &self.brush,
                        2.,
                        None,
                    );
                }
            }
            if let Some(i) = hovered.filter(|_| super::preferences::hints()) {
                let b = layout.buttons[i];
                let width =
                    ((i64::from(b.right) - i64::from(b.left)) as f32 + 48. * s).round() as i32;
                let t = self.local(fit_panel(
                    PixelPoint {
                        x: b.left.saturating_sub((24. * s).round() as i32),
                        y: layout.bounds.top.saturating_sub((34. * s).round() as i32),
                    },
                    layout.screen,
                    width,
                    (28. * s).round() as i32,
                ));
                self.round(t, 6. * s, panel);
                self.label(
                    names[i],
                    t,
                    if self.dark {
                        color(245, 246, 248, 1.)
                    } else {
                        color(36, 42, 50, 1.)
                    },
                );
            }
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
        let metrics = super::annotation_render::text_metrics(&self.dw, &content, size, width)?;
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
        Ok(crate::core::annotation::Object {
            shape: Shape::Text {
                bounds: PixelRect {
                    left: origin.x,
                    top: origin.y,
                    right,
                    bottom,
                },
                content,
                size,
                wrap_width: width,
                angle,
            },
            style,
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
        } else if (VALUE_START..VALUE_START + 3. * VALUE_STEP).contains(&x) {
            let offset = x - VALUE_START;
            (offset % VALUE_STEP < 46.).then_some(8 + (offset / VALUE_STEP) as usize)
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
        self.round(
            r,
            10. * s,
            if self.dark {
                color(44, 47, 53, 0.98)
            } else {
                color(249, 250, 253, 0.98)
            },
        );
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
                    self.brush.SetColor(&color(0, 120, 212, 1.));
                    self.target.DrawRectangle(&rect, &self.brush, 1.5, None);
                }
                self.brush.SetColor(&if *rgb == editor.style().color {
                    color(0, 120, 212, 1.)
                } else {
                    color(136, 143, 155, 1.)
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
            for (i, width) in (if editor.tool == Tool::Text {
                [18., 24., 36.]
            } else {
                [1., 3., 6.]
            })
            .iter()
            .enumerate()
            {
                let b = D2D_RECT_F {
                    left: r.left + (VALUE_START + i as f32 * VALUE_STEP) * s,
                    top: r.top + 6. * s,
                    right: r.left + (VALUE_START + 46. + i as f32 * VALUE_STEP) * s,
                    bottom: r.bottom - 6. * s,
                };
                if focus == 19 + i {
                    self.brush.SetColor(&color(0, 120, 212, 1.));
                    self.target.DrawRectangle(&b, &self.brush, 1.5, None);
                }
                if *width
                    == if editor.tool == Tool::Text {
                        editor.text_size
                    } else {
                        editor.style().width
                    }
                {
                    self.round(
                        b,
                        6. * s,
                        if self.dark {
                            color(41, 87, 129, 1.)
                        } else {
                            color(211, 233, 255, 1.)
                        },
                    );
                }
                self.brush.SetColor(&if self.dark {
                    color(245, 246, 250, 1.)
                } else {
                    color(34, 40, 49, 1.)
                });
                if editor.tool == Tool::Text {
                    self.label(
                        &format!("{}", *width as i32),
                        b,
                        if self.dark {
                            color(245, 246, 250, 1.)
                        } else {
                            color(34, 40, 49, 1.)
                        },
                    );
                    continue;
                }
                self.target.DrawLine(
                    Vector2 {
                        X: b.left + 10. * s,
                        Y: (b.top + b.bottom) / 2.,
                    },
                    Vector2 {
                        X: b.right - 10. * s,
                        Y: (b.top + b.bottom) / 2.,
                    },
                    &self.brush,
                    width * s,
                    None,
                );
            }
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
                    self.brush.SetColor(&color(0, 120, 212, 0.45));
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
                self.brush.SetColor(&color(239, 153, 44, 1.));
                for p in object.handles() {
                    let p = point(p);
                    let r = D2D_RECT_F {
                        left: p.X - 3. * self.scale,
                        top: p.Y - 3. * self.scale,
                        right: p.X + 3. * self.scale,
                        bottom: p.Y + 3. * self.scale,
                    };
                    self.fill(r, color(255, 255, 255, 1.));
                    self.brush.SetColor(&color(239, 153, 44, 1.));
                    self.target.DrawRectangle(&r, &self.brush, 1., None);
                }
            }
            self.target.PopAxisAlignedClip();
            if let Some(object) = editor
                .selected
                .filter(|i| editing != Some(*i))
                .and_then(|i| editor.objects().get(i))
            {
                if let Shape::Text { bounds, angle, .. } = object.shape {
                    let c = color(0, 120, 212, 1.);
                    self.brush.SetColor(&c);
                    let mut prior = Matrix3x2::default();
                    self.target.GetTransform(&mut prior);
                    self.target.SetTransform(
                        &(super::annotation_render::text_transform(bounds, self.bounds(), angle)
                            * prior),
                    );
                    self.target
                        .DrawRectangle(&self.local(bounds), &self.brush, 1., None);
                    self.target.SetTransform(&prior);
                    if editor.tool == Tool::Select {
                        if let Some(handle) = object.rotation_handle(self.scale) {
                            let (cx, _) = text_center(bounds);
                            let (x, y) = rotate_point(bounds, angle, cx, f64::from(bounds.top));
                            let origin = self.bounds();
                            let tip = Vector2 {
                                X: (x - f64::from(origin.left)) as f32,
                                Y: (y - f64::from(origin.top)) as f32,
                            };
                            let handle = point(handle);
                            self.target.DrawLine(tip, handle, &self.brush, 1., None);
                            let circle = D2D1_ELLIPSE {
                                point: handle,
                                radiusX: 4. * self.scale,
                                radiusY: 4. * self.scale,
                            };
                            self.brush.SetColor(&color(255, 255, 255, 1.));
                            self.target.FillEllipse(&circle, &self.brush);
                            self.brush.SetColor(&c);
                            self.target.DrawEllipse(&circle, &self.brush, 1., None);
                        }
                    }
                }
            }
        }
    }
    fn icon(&self, index: usize, r: D2D_RECT_F, c: D2D1_COLOR_F, scale: f32) {
        let center = Vector2 {
            X: (r.left + r.right) / 2.,
            Y: (r.top + r.bottom) / 2.,
        };
        // SAFETY: the brush is owned by the active canvas on the render thread.
        unsafe {
            self.brush.SetColor(&if index == 9 {
                color(255, 255, 255, 1.)
            } else {
                c
            });
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
                color(255, 255, 255, 1.),
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
