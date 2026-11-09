//! Retained Direct2D surface and palette for the settings window.
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
pub const fn background(dark: bool) -> u32 {
    if dark { 0x242527 } else { 0xfcfcfd }
}
pub const fn sidebar(dark: bool) -> u32 {
    if dark { 0x2d2e31 } else { 0xf1f3f6 }
}
pub const fn foreground(dark: bool) -> u32 {
    if dark { 0xf2f3f5 } else { 0x202226 }
}
pub const fn muted(dark: bool) -> u32 {
    if dark { 0xb7bbc2 } else { 0x585d66 }
}
pub const fn divider(dark: bool) -> u32 {
    if dark { 0x47494e } else { 0xdce0e6 }
}
pub const fn accent(dark: bool) -> u32 {
    if dark { 0x82c1ff } else { 0x006ac0 }
}
pub const fn warning(dark: bool) -> u32 {
    if dark { 0xffbd85 } else { 0x974311 }
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
