//! Event-driven image surface. Display scaling never alters the source pixels.
use crate::core::{image::Image, pinned::View};
use windows::{
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

thread_local! {static FACTORY:std::cell::RefCell<Option<ID2D1Factory>>=const {std::cell::RefCell::new(None)};}
fn factory() -> Result<ID2D1Factory> {
    FACTORY.with_borrow_mut(|slot| {
        if let Some(factory) = slot {
            return Ok(factory.clone());
        }
        // SAFETY: one UI-thread factory shares its device/resource cache among every pinned surface.
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        *slot = Some(factory.clone());
        Ok(factory)
    })
}
pub fn shutdown() {
    FACTORY.with_borrow_mut(|slot| {
        *slot = None;
    });
}
pub struct Canvas {
    target: ID2D1HwndRenderTarget,
    bitmap: ID2D1Bitmap,
    brush: ID2D1SolidColorBrush,
    text: IDWriteTextFormat,
    icons: IDWriteTextFormat,
    pub dark: bool,
}
pub fn button(width: i32, p: (i32, i32), scale: f32) -> Option<usize> {
    let left = width as f32 - 124. * scale;
    let x = p.0 as f32;
    let y = p.1 as f32;
    if y >= 8. * scale && y < 44. * scale && x >= left && x < left + 108. * scale {
        Some(((x - left) / (36. * scale)) as usize)
    } else {
        None
    }
}
impl Canvas {
    pub fn new(hwnd: HWND, image: &Image, width: i32, height: i32, dpi: u32) -> Result<Self> {
        // SAFETY: live UI-thread HWND, thread-local D2D/DWrite objects and call-scoped pixel storage copied by CreateBitmap.
        unsafe {
            let (w, h) = image
                .bounds()
                .size()
                .ok_or_else(windows::core::Error::from_win32)?;
            let factory = factory()?;
            let target = factory.CreateHwndRenderTarget(
                &D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    dpiX: 96.,
                    dpiY: 96.,
                    ..Default::default()
                },
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U {
                        width: width.max(1) as u32,
                        height: height.max(1) as u32,
                    },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                },
            )?;
            let mut pixels = Vec::new();
            pixels
                .try_reserve_exact(image.pixels().len())
                .map_err(|_| {
                    windows::core::Error::new(
                        windows::Win32::Foundation::E_OUTOFMEMORY,
                        "没有足够内存显示贴图",
                    )
                })?;
            pixels.extend_from_slice(image.pixels());
            for p in pixels.chunks_exact_mut(4) {
                for channel in 0..3 {
                    p[channel] = ((u16::from(p[channel]) * u16::from(p[3]) + 127) / 255) as u8;
                }
            }
            let bitmap = target.CreateBitmap(
                D2D_SIZE_U {
                    width: w,
                    height: h,
                },
                Some(pixels.as_ptr().cast()),
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
            let brush = target.CreateSolidColorBrush(&color(0., 1.), None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let scale = dpi as f32 / 96.;
            let format = |icons| -> Result<IDWriteTextFormat> {
                let f = dw.CreateTextFormat(
                    if icons {
                        w!("Segoe Fluent Icons")
                    } else {
                        w!("Segoe UI")
                    },
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    if icons { 18. * scale } else { 13. * scale },
                    w!("zh-CN"),
                )?;
                f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                Ok(f)
            };
            Ok(Self {
                target,
                bitmap,
                brush,
                text: format(false)?,
                icons: format(true)?,
                dark: super::selection_render::system_dark(),
            })
        }
    }
    pub fn resize(&self, width: i32, height: i32) -> Result<()> {
        // SAFETY: the target belongs to this UI thread; dimensions are positive client pixels.
        unsafe {
            self.target.Resize(&D2D_SIZE_U {
                width: width.max(1) as u32,
                height: height.max(1) as u32,
            })
        }
    }
    pub fn paint(
        &self,
        view: View,
        minimum: (i32, i32),
        dpi: u32,
        hover: bool,
        notice: Option<&str>,
        busy: bool,
    ) -> Result<()> {
        let (width, height) = view.dimensions(minimum);
        let (x, y) = view.image_origin(minimum);
        let scale = dpi as f32 / 96.;
        // SAFETY: every resource is UI-thread owned; drawing references are call-scoped. EndDraw pairs BeginDraw on all paths.
        unsafe {
            self.target.BeginDraw();
            self.target
                .Clear(Some(&color(if self.dark { 0.12 } else { 0.96 }, 1.)));
            self.target.DrawBitmap(
                &self.bitmap,
                Some(&D2D_RECT_F {
                    left: x as f32,
                    top: y as f32,
                    right: (x + f64::from(view.width) * view.scale) as f32,
                    bottom: (y + f64::from(view.height) * view.scale) as f32,
                }),
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
            self.brush
                .SetColor(&color(if self.dark { 0.45 } else { 0.65 }, 0.8));
            self.target.DrawRectangle(
                &D2D_RECT_F {
                    left: 0.5,
                    top: 0.5,
                    right: width as f32 - 0.5,
                    bottom: height as f32 - 0.5,
                },
                &self.brush,
                1.,
                None,
            );
            if hover {
                let left = width as f32 - 128. * scale;
                self.panel(
                    D2D_RECT_F {
                        left,
                        top: 4. * scale,
                        right: width as f32 - 8. * scale,
                        bottom: 48. * scale,
                    },
                    scale,
                );
                self.brush
                    .SetColor(&color(if self.dark { 0.94 } else { 0.12 }, 1.));
                for (i, glyph) in [0xe8c8u16, 0xe74e, 0xe8bb].iter().enumerate() {
                    self.brush.SetColor(&color(
                        if self.dark { 0.94 } else { 0.12 },
                        if busy && i < 2 { 0.4 } else { 1. },
                    ));
                    let left = left + (4. + i as f32 * 36.) * scale;
                    self.target.DrawText(
                        &[*glyph],
                        &self.icons,
                        &D2D_RECT_F {
                            left,
                            top: 8. * scale,
                            right: left + 36. * scale,
                            bottom: 44. * scale,
                        },
                        &self.brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }
            if let Some(text) = notice {
                let right = width as f32 - 10. * scale;
                let bottom = height as f32 - 10. * scale;
                let rect = D2D_RECT_F {
                    left: (right - 112. * scale).max(2.),
                    top: (bottom - 30. * scale).max(2.),
                    right,
                    bottom,
                };
                self.panel(rect, scale);
                self.brush
                    .SetColor(&color(if self.dark { 0.94 } else { 0.12 }, 1.));
                let text: Vec<u16> = text.encode_utf16().collect();
                self.target.DrawText(
                    &text,
                    &self.text,
                    &rect,
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            self.target.EndDraw(None, None)
        }
    }
    fn panel(&self, rect: D2D_RECT_F, scale: f32) {
        // SAFETY: called only during the owning target's active draw on the UI thread.
        unsafe {
            self.brush
                .SetColor(&color(if self.dark { 0.16 } else { 0.99 }, 0.97));
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 8. * scale,
                radiusY: 8. * scale,
            };
            self.target.FillRoundedRectangle(&rounded, &self.brush);
            self.brush
                .SetColor(&color(if self.dark { 0.35 } else { 0.78 }, 1.));
            self.target
                .DrawRoundedRectangle(&rounded, &self.brush, 1., None);
        }
    }
}
fn color(v: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: v,
        g: v,
        b: v,
        a,
    }
}
