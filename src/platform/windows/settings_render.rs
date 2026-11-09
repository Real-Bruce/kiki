//! Small retained Direct2D surface for settings cards and text.
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
        },
    },
    core::{Interface, Result, w},
};
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
        unsafe {
            self.target.BeginDraw();
            self.target
                .Clear(Some(&color(if dark { 0x202020 } else { 0xf3f3f3 })));
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
    pub fn card(&self, x: f32, y: f32, w: f32, h: f32, dark: bool) {
        unsafe {
            self.brush
                .SetColor(&color(if dark { 0x2b2b2b } else { 0xffffff }));
            self.target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    radiusX: 8.,
                    radiusY: 8.,
                },
                &self.brush,
            );
        }
    }
    pub fn text(&self, text: &str, x: f32, y: f32, width: f32, size: f32, rgb: u32) {
        // SAFETY: UTF-16 slice and layout rectangle live through this synchronous drawing call.
        unsafe {
            if let Ok(format) = self.factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("zh-CN"),
            ) {
                self.brush.SetColor(&color(rgb));
                let wide: Vec<_> = text.encode_utf16().collect();
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
