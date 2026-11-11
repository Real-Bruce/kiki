//! Retained Direct2D surface and palette for the settings window.
pub use super::ui_palette::*;
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::HDC,
        },
    },
    core::{Interface, Result, w},
};

thread_local! {
    static CONTROL_SURFACE: std::cell::Cell<Option<ControlSurface>> = const { std::cell::Cell::new(None) };
}

struct ControlSurface {
    target: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
}

impl ControlSurface {
    fn new() -> Result<Self> {
        // SAFETY: the factory, target and brush are created and used on the settings UI thread.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                // Win32 supplies physical pixels; the caller already scales DIP dimensions.
                dpiX: 96.,
                dpiY: 96.,
                ..Default::default()
            })?;
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let brush = target.CreateSolidColorBrush(&color(0), None)?;
            Ok(Self { target, brush })
        }
    }
}

/// Paint into the DC borrowed from a native child. The caller can draw GDI text after this
/// returns. A failed bind or draw discards the target so the next paint can recreate it.
pub fn paint_control<T>(
    dc: HDC,
    bounds: RECT,
    background: u32,
    draw: impl FnOnce(&ControlShapes<'_>) -> T,
) -> Option<T> {
    if bounds.left >= bounds.right || bounds.top >= bounds.bottom {
        return None;
    }
    let surface = CONTROL_SURFACE
        .take()
        .or_else(|| ControlSurface::new().ok())?;
    // SAFETY: dc remains live through EndDraw. No GDI calls or window dispatch occur during
    // the closure, which can only issue Direct2D drawing commands on this thread.
    unsafe {
        surface.target.BindDC(dc, &bounds).ok()?;
        surface.target.BeginDraw();
        surface.target.Clear(Some(&color(background)));
        let output = draw(&ControlShapes {
            surface: &surface,
            origin: [bounds.left, bounds.top],
        });
        surface.target.EndDraw(None, None).ok()?;
        CONTROL_SURFACE.set(Some(surface));
        Some(output)
    }
}

pub fn release_control_surface() {
    drop(CONTROL_SURFACE.take());
}

pub struct ControlShapes<'a> {
    surface: &'a ControlSurface,
    origin: [i32; 2],
}

impl ControlShapes<'_> {
    fn rect(&self, bounds: RECT) -> D2D_RECT_F {
        let [x, y] = self.origin;
        D2D_RECT_F {
            left: (bounds.left - x) as f32,
            top: (bounds.top - y) as f32,
            right: (bounds.right - x) as f32,
            bottom: (bounds.bottom - y) as f32,
        }
    }

    pub fn rounded(&self, bounds: RECT, diameter: i32, rgb: u32) {
        let rect = self.rect(bounds);
        let radius =
            (diameter as f32 / 2.).min((rect.right - rect.left).min(rect.bottom - rect.top) / 2.);
        if radius <= 0. {
            return;
        }
        // SAFETY: the target and brush are borrowed for the active draw, with no GDI calls.
        unsafe {
            self.surface.brush.SetColor(&color(rgb));
            self.surface.target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: radius,
                    radiusY: radius,
                },
                &self.surface.brush,
            );
        }
    }

    pub fn focus_ring(&self, bounds: RECT, radius: f32, width: f32, rgb: u32) {
        let mut rect = self.rect(bounds);
        let half = width / 2.;
        rect.left += half;
        rect.top += half;
        rect.right -= half;
        rect.bottom -= half;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return;
        }
        let radius = (radius - half)
            .max(0.)
            .min((rect.right - rect.left).min(rect.bottom - rect.top) / 2.);
        // SAFETY: the brush belongs to this active target. The stroke stays within the bounds.
        unsafe {
            self.surface.brush.SetColor(&color(rgb));
            self.surface.target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: radius,
                    radiusY: radius,
                },
                &self.surface.brush,
                width,
                None,
            );
        }
    }

    pub fn clipped(&self, bounds: RECT, draw: impl FnOnce()) {
        // SAFETY: this clip belongs to the active draw. The closure only draws within this pass.
        unsafe {
            self.surface
                .target
                .PushAxisAlignedClip(&self.rect(bounds), D2D1_ANTIALIAS_MODE_ALIASED);
            draw();
            self.surface.target.PopAxisAlignedClip();
        }
    }
}

pub struct Canvas {
    target: ID2D1RenderTarget,
    context: ID2D1DeviceContext,
    swap: windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    brush: ID2D1SolidColorBrush,
    factory: IDWriteFactory,
    _device: std::rc::Rc<super::pinned_device::Device>,
}
impl Canvas {
    pub fn new(hwnd: HWND, width: u32, height: u32, dpi: u32) -> Result<Self> {
        // SAFETY: all COM resources belong to the window's UI thread; the target retains no borrowed pointers.
        unsafe {
            let device = super::pinned_device::get()?;
            let (context, swap) = device.surface(hwnd, width, height, false)?;
            context.SetDpi(dpi as f32, dpi as f32);
            let target: ID2D1RenderTarget = context.cast()?;
            let brush = target.CreateSolidColorBrush(&color(0), None)?;
            Ok(Self {
                target,
                context,
                swap,
                _device: device,
                brush,
                factory: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
            })
        }
    }
    pub fn resize(&self, width: u32, height: u32, dpi: u32) -> Result<()> {
        // SAFETY: called between draws; release the context's only back-buffer reference before resize.
        unsafe {
            self.context.SetTarget(None::<&ID2D1Image>);
            self.swap.ResizeBuffers(
                2,
                width.max(1),
                height.max(1),
                windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_FLAG(0),
            )?;
            super::pinned_device::bind(&self.context, &self.swap)?;
            self.context.SetDpi(dpi as f32, dpi as f32);
            Ok(())
        }
    }
    pub fn begin(&self, dark: bool) {
        // SAFETY: this canvas owns the target on the window thread between BeginDraw and EndDraw.
        unsafe {
            self.target.BeginDraw();
            self.target.Clear(Some(&color(background(dark))));
        }
    }
    pub fn end(&self) -> Result<()> {
        // SAFETY: owning UI-thread target and swap chain, after all card/text drawing.
        unsafe {
            self.target.EndDraw(None, None)?;
            self.swap
                .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0))
                .ok()
        }
    }
    pub fn fill(&self, x: f32, y: f32, w: f32, h: f32, rgb: u32) {
        // SAFETY: brush and target are owned by this active render pass.
        unsafe {
            self.brush.SetColor(&color(rgb));
            self.target.FillRectangle(
                &D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                &self.brush,
            );
        }
    }
    pub fn input_frame(&self, x: f32, y: f32, w: f32, h: f32, rgb: u32) {
        // SAFETY: the call-scoped geometry and brush belong to the current settings paint.
        unsafe {
            self.brush.SetColor(&color(rgb));
            self.target.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    radiusX: 5.,
                    radiusY: 5.,
                },
                &self.brush,
                1.,
                None,
            );
        }
    }
    pub fn heading(&self, text: &str, x: f32, y: f32, width: f32, size: f32, rgb: u32) {
        self.write(
            text,
            [x, y, width, size],
            rgb,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            text_font(text),
        );
    }
    pub fn value(&self, text: &str, x: f32, y: f32, width: f32, rgb: u32) {
        self.write(
            text,
            [x, y, width, 14.],
            rgb,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
            text_font(text),
        );
    }
    pub fn glyph(&self, text: &str, x: f32, y: f32, rgb: u32) {
        self.write(
            text,
            [x, y, 22., 17.],
            rgb,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            w!("Segoe Fluent Icons"),
        );
    }
    pub fn text(&self, text: &str, x: f32, y: f32, width: f32, size: f32, rgb: u32) {
        self.write(
            text,
            [x, y, width, size],
            rgb,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            text_font(text),
        );
    }
    fn write(
        &self,
        text: &str,
        [x, y, width, size]: [f32; 4],
        rgb: u32,
        weight: DWRITE_FONT_WEIGHT,
        alignment: DWRITE_TEXT_ALIGNMENT,
        font: windows::core::PCWSTR,
    ) {
        // SAFETY: UTF-16 slice and layout rectangle live through this synchronous drawing call.
        unsafe {
            if let Ok(format) = self.factory.CreateTextFormat(
                font,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("zh-CN"),
            ) {
                let _ = format.SetTextAlignment(alignment);
                let wide: Vec<_> = text.encode_utf16().collect();
                self.brush.SetColor(&color(rgb));
                self.target.DrawText(
                    &wide,
                    &format,
                    &D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + width,
                        bottom: y + size * 2.5,
                    },
                    &self.brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }
}
fn text_font(text: &str) -> windows::core::PCWSTR {
    if text.chars().any(|c| c > '\u{2e7f}') {
        w!("Microsoft YaHei UI")
    } else {
        w!("Segoe UI")
    }
}
fn color(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 255) as f32 / 255.,
        g: ((rgb >> 8) & 255) as f32 / 255.,
        b: (rgb & 255) as f32 / 255.,
        a: 1.,
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: UI-thread surface, outside BeginDraw, before the window's state releases its device.
        unsafe {
            self.context.SetTarget(None::<&ID2D1Image>);
            let _ = self.context.Flush(None, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::*;

    #[test]
    fn native_control_edges_have_blended_pixels_at_multiple_scales() {
        // SAFETY: the test owns the memory DC and bitmap for the entire render and readback.
        unsafe {
            let dc = CreateCompatibleDC(None);
            assert!(!dc.is_invalid());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 144,
                    biHeight: -100,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, bitmap.into());
            let bounds = RECT {
                left: 7,
                top: 9,
                right: 135,
                bottom: 92,
            };
            let mut sampled = Vec::new();
            for percent in [100, 125, 150, 200] {
                let scale = |dip: i32| (dip * percent + 50) / 100;
                let circle = RECT {
                    left: 13,
                    top: 15,
                    right: 13 + scale(22),
                    bottom: 15 + scale(22),
                };
                let frame = RECT {
                    left: 65,
                    top: 16,
                    right: 65 + scale(29),
                    bottom: 16 + scale(24),
                };
                let ok = paint_control(dc, bounds, 0xffffff, |shapes| {
                    shapes.rounded(circle, scale(22), 0x000000);
                    shapes.focus_ring(frame, scale(6) as f32, percent as f32 / 100., 0x000000);
                })
                .is_some();
                // Sample the corners rather than text or filled centers. Binary GDI masks
                // would yield only black and white for the same shapes.
                let corner_has_blend = |r: RECT| {
                    (r.top..r.top + scale(6)).any(|y| {
                        (r.left..r.left + scale(6)).any(|x| {
                            let pixel = GetPixel(dc, x, y).0;
                            pixel > 0 && pixel < 0x00ffffff
                        })
                    })
                };
                sampled.push((
                    percent,
                    ok,
                    GetPixel(dc, circle.left, circle.top).0,
                    GetPixel(dc, circle.left + scale(11), circle.top + scale(11)).0,
                    corner_has_blend(circle),
                    corner_has_blend(frame),
                ));
            }
            release_control_surface();
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            for (percent, drawn, corner, center, circle_blend, frame_blend) in sampled {
                assert!(
                    drawn && corner == 0xffffff && center == 0,
                    "{percent}%: circle placement"
                );
                assert!(
                    circle_blend && frame_blend,
                    "{percent}%: antialiased circle and outline"
                );
            }
        }
    }
}
