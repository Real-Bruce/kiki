use crate::core::{
    display::DisplayInfo,
    geometry::PixelRect,
    image::Image,
    selection::{PixelPoint, Selection, contains, toolbar},
};
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
        },
    },
    core::{Result, w},
};
use windows_numerics::Vector2;

pub struct Layout {
    pub bounds: PixelRect,
    pub buttons: [PixelRect; 11],
    pub scale: f32,
}
pub struct Canvas {
    target: ID2D1HwndRenderTarget,
    bitmap: ID2D1Bitmap,
    brush: ID2D1SolidColorBrush,
    dw: IDWriteFactory,
    text: IDWriteTextFormat,
    icons: IDWriteTextFormat,
    scale: f32,
    pub dark: bool,
    source: Image,
}
pub fn system_dark() -> bool {
    if std::env::args_os().any(|a| a == "--theme=dark") {
        return true;
    }
    if std::env::args_os().any(|a| a == "--theme=light") {
        return false;
    }
    UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Background))
        .is_ok_and(|c| u32::from(c.R) + u32::from(c.G) + u32::from(c.B) < 384)
}
impl Canvas {
    pub fn new(hwnd: HWND, image: Image) -> Result<Self> {
        // SAFETY: UI-thread COM objects, live HWND and call-scoped pixel slice; CreateBitmap copies the source.
        unsafe {
            let (width, height) = image
                .bounds()
                .size()
                .ok_or_else(windows::core::Error::from_win32)?;
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateHwndRenderTarget(
                &D2D1_RENDER_TARGET_PROPERTIES {
                    dpiX: 96.,
                    dpiY: 96.,
                    ..Default::default()
                },
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U { width, height },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                },
            )?;
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
            let text = format(&dw, false, 1.)?;
            let icons = format(&dw, true, 1.)?;
            Ok(Self {
                target,
                bitmap,
                brush,
                dw,
                text,
                icons,
                scale: 1.,
                dark: system_dark(),
                source: image,
            })
        }
    }
    pub fn layout(&self, rect: PixelRect, screens: &[DisplayInfo]) -> Layout {
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
        let (screen, preferred) = screen.map_or((self.source.bounds(), 1.), |s| {
            (s.bounds, s.scale_percent as f32 / 100.)
        });
        let scale = preferred
            .min((i64::from(screen.right) - i64::from(screen.left)) as f32 / 520.)
            .max(0.1);
        let bounds = toolbar(
            rect,
            screen,
            (520. * scale).round() as i32,
            (48. * scale).round() as i32,
            (10. * scale).round() as i32,
        );
        let mut x = bounds.left as f32 + 8. * scale;
        let buttons = std::array::from_fn(|i| {
            if [1, 5, 7].contains(&i) {
                x += 8. * scale;
            }
            let width = if i == 9 { 80. } else { 40. };
            let r = PixelRect {
                left: x.round() as i32,
                top: (bounds.top as f32 + 6. * scale).round() as i32,
                right: (x + (width - 4.) * scale).round() as i32,
                bottom: (bounds.top as f32 + 42. * scale).round() as i32,
            };
            x += width * scale;
            r
        });
        Layout {
            bounds,
            buttons,
            scale,
        }
    }
    pub fn paint(
        &mut self,
        selection: &Selection,
        hover: Option<PixelRect>,
        screens: &[DisplayInfo],
        pointer: PixelPoint,
        focus: usize,
    ) -> Result<()> {
        let layout = selection
            .rect
            .filter(|_| !selection.active())
            .map(|r| self.layout(r, screens));
        let scale = layout.as_ref().map_or_else(
            || {
                screens
                    .first()
                    .map_or(1., |s| s.scale_percent as f32 / 100.)
            },
            |l| l.scale,
        );
        // SAFETY: all drawing resources belong to this UI thread; BeginDraw is always paired with EndDraw.
        unsafe {
            if (scale - self.scale).abs() > 0.001 {
                self.text = format(&self.dw, false, scale)?;
                self.icons = format(&self.dw, true, scale)?;
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
                    let label = D2D_RECT_F {
                        left: r.left.max(4.),
                        top: (r.top - 29. * scale).max(4.),
                        right: (r.left + 132. * scale).min(w - 4.),
                        bottom: (r.top - 3. * scale).max(30. * scale),
                    };
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
            if let Some(layout) = layout {
                self.paint_toolbar(&layout, pointer, focus);
            }
            let hint = D2D_RECT_F {
                left: 16. * scale,
                top: h - 44. * scale,
                right: 350. * scale,
                bottom: h - 12. * scale,
            };
            self.round(hint, 6. * scale, color(30, 36, 46, 0.88));
            self.label(
                "拖动框选　F 当前屏幕　Esc 取消",
                hint,
                color(255, 255, 255, 1.),
            );
            self.target.EndDraw(None, None)
        }
    }
    fn paint_toolbar(&self, layout: &Layout, pointer: PixelPoint, focus: usize) {
        // SAFETY: synchronous UI-thread drawing; all brushes and formats are retained by Canvas.
        unsafe {
            let r = self.local(layout.bounds);
            let s = layout.scale;
            self.round(
                D2D_RECT_F {
                    left: r.left - 2.,
                    top: r.top + 3.,
                    right: r.right + 2.,
                    bottom: r.bottom + 5.,
                },
                12. * s,
                color(0, 0, 0, 0.18),
            );
            let panel = if self.dark {
                color(44, 47, 53, 0.97)
            } else {
                color(249, 250, 253, 0.97)
            };
            self.round(r, 12. * s, panel);
            self.brush.SetColor(&if self.dark {
                color(98, 103, 114, 1.)
            } else {
                color(205, 211, 223, 1.)
            });
            self.target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: r,
                    radiusX: 12. * s,
                    radiusY: 12. * s,
                },
                &self.brush,
                1.,
                None,
            );
            for i in [1, 5, 7] {
                let b = self.local(layout.buttons[i]);
                self.fill(
                    D2D_RECT_F {
                        left: b.left - 6. * s,
                        top: r.top + 12. * s,
                        right: b.left - 5. * s,
                        bottom: r.bottom - 12. * s,
                    },
                    color(135, 143, 157, 0.35),
                );
            }
            let names = [
                "选择",
                "矩形",
                "箭头",
                "画笔",
                "文字",
                "撤销",
                "重做",
                "贴图",
                "保存",
                "复制",
                "取消 · Esc",
            ];
            let mut hovered = None;
            for (i, button) in layout.buttons.iter().enumerate() {
                let b = self.local(*button);
                let active = i == 0 || i == 10;
                let over = contains(*button, pointer);
                if over {
                    hovered = Some(i);
                }
                if i == 0 {
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
                if over && active {
                    self.round(
                        b,
                        6. * s,
                        if self.dark {
                            color(72, 82, 95, 1.)
                        } else {
                            color(227, 232, 242, 1.)
                        },
                    );
                }
                if i == 9 {
                    self.round(b, 6. * s, color(0, 120, 212, 0.65));
                }
                let foreground = if i == 0 {
                    if self.dark {
                        color(235, 246, 255, 1.)
                    } else {
                        color(0, 120, 212, 1.)
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
                    color(34, 40, 49, 1.)
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
            if let Some(i) = hovered {
                let b = self.local(layout.buttons[i]);
                let t = D2D_RECT_F {
                    left: b.left - 14. * s,
                    top: r.top - 34. * s,
                    right: b.right + 14. * s,
                    bottom: r.top - 6. * s,
                };
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
    fn icon(&self, index: usize, r: D2D_RECT_F, c: D2D1_COLOR_F, s: f32) {
        let x = (r.left + r.right) / 2.;
        let y = (r.top + r.bottom) / 2.;
        // SAFETY: retained UI-thread rendering resources and bounded icon coordinates.
        unsafe {
            let line = |a: (f32, f32), b: (f32, f32)| {
                self.brush.SetColor(&c);
                self.target.DrawLine(
                    Vector2 {
                        X: x + a.0 * s,
                        Y: y + a.1 * s,
                    },
                    Vector2 {
                        X: x + b.0 * s,
                        Y: y + b.1 * s,
                    },
                    &self.brush,
                    1.5 * s,
                    None,
                );
            };
            match index {
                0 => {
                    for (a, b) in [
                        ((-7., -10.), (-6., 9.)),
                        ((-6., 9.), (-1., 4.)),
                        ((-1., 4.), (3., 11.)),
                        ((3., 11.), (6., 9.)),
                        ((6., 9.), (2., 2.)),
                        ((2., 2.), (9., 1.)),
                        ((9., 1.), (-7., -10.)),
                    ] {
                        line(a, b);
                    }
                }
                1 => {
                    self.brush.SetColor(&c);
                    self.target.DrawRectangle(
                        &D2D_RECT_F {
                            left: x - 9. * s,
                            top: y - 7. * s,
                            right: x + 9. * s,
                            bottom: y + 7. * s,
                        },
                        &self.brush,
                        1.5 * s,
                        None,
                    );
                }
                2 => {
                    line((-9., 9.), (9., -9.));
                    line((0., -9.), (9., -9.));
                    line((9., -9.), (9., 0.));
                }
                4 => {
                    line((-8., -9.), (8., -9.));
                    line((0., -9.), (0., 9.));
                    line((-4., 9.), (4., 9.));
                }
                9 => {
                    self.brush.SetColor(&color(255, 255, 255, 0.7));
                    self.target.DrawRectangle(
                        &D2D_RECT_F {
                            left: r.left + 10. * s,
                            top: y - 5. * s,
                            right: r.left + 21. * s,
                            bottom: y + 8. * s,
                        },
                        &self.brush,
                        1.5 * s,
                        None,
                    );
                    self.target.DrawRectangle(
                        &D2D_RECT_F {
                            left: r.left + 13. * s,
                            top: y - 8. * s,
                            right: r.left + 24. * s,
                            bottom: y + 5. * s,
                        },
                        &self.brush,
                        1.5 * s,
                        None,
                    );
                    self.label(
                        "复制",
                        D2D_RECT_F {
                            left: r.left + 29. * s,
                            ..r
                        },
                        color(255, 255, 255, 0.75),
                    );
                }
                10 => {
                    line((-7., -7.), (7., 7.));
                    line((7., -7.), (-7., 7.));
                }
                _ => {
                    let glyph = match index {
                        3 => 0xE70F,
                        5 => 0xE7A7,
                        6 => 0xE7A6,
                        7 => 0xE718,
                        8 => 0xE74E,
                        _ => 0xE711,
                    };
                    self.brush.SetColor(&c);
                    self.target.DrawText(
                        &[glyph],
                        &self.icons,
                        &r,
                        &self.brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
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
fn format(factory: &IDWriteFactory, icon: bool, scale: f32) -> Result<IDWriteTextFormat> {
    // SAFETY: formats are created and retained on this UI thread; static system font names are NUL terminated.
    unsafe {
        let text = factory.CreateTextFormat(
            if icon {
                w!("Segoe Fluent Icons")
            } else {
                w!("Segoe UI")
            },
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            if icon { 20. * scale } else { 14. * scale },
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
