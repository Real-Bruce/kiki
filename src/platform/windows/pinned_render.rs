//! Event-driven image surface. Display scaling never alters the source pixels.
use super::{
    capture_toolbar::{CLOSE, COPY, SAVE},
    toolbar_icons::ToolbarIcons,
    ui_palette as palette,
};
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
use windows_numerics::Vector2;

pub fn shutdown() {
    super::pinned_device::shutdown();
}
pub struct Canvas {
    target: ID2D1RenderTarget,
    context: ID2D1DeviceContext,
    swap: windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
    bitmap: ID2D1Bitmap,
    brush: ID2D1SolidColorBrush,
    text: IDWriteTextFormat,
    icons: ToolbarIcons,
    pub dark: bool,
    _device: std::rc::Rc<super::pinned_device::Device>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Button {
    Copy,
    Save,
    Close,
}

impl Button {
    const ALL: [Self; 3] = [Self::Copy, Self::Save, Self::Close];

    const fn icon(self) -> usize {
        match self {
            Self::Copy => COPY,
            Self::Save => SAVE,
            Self::Close => CLOSE,
        }
    }

    pub const fn command(self) -> u32 {
        match self {
            Self::Copy => 1,
            Self::Save => 2,
            Self::Close => 4,
        }
    }

    pub const fn enabled(self, busy: bool) -> bool {
        !busy || matches!(self, Self::Close)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Toolbar {
    panel: D2D_RECT_F,
    scale: f32,
}

pub(super) struct Controls {
    pub hot: Option<Button>,
    pub down: Option<Button>,
    pub busy: bool,
}

impl Toolbar {
    const EDGE: f32 = 6.;
    const PADDING: f32 = 4.;
    const BUTTON: f32 = 36.;

    pub fn new(width: i32, dpi: u32) -> Self {
        let scale = dpi as f32 / 96.;
        let right = width as f32 - Self::EDGE * scale;
        let top = Self::EDGE * scale;
        Self {
            panel: D2D_RECT_F {
                left: right
                    - (Self::BUTTON * Button::ALL.len() as f32 + 2. * Self::PADDING) * scale,
                top,
                right,
                bottom: top + (Self::BUTTON + 2. * Self::PADDING) * scale,
            },
            scale,
        }
    }

    fn buttons(self) -> impl Iterator<Item = (Button, D2D_RECT_F)> {
        Button::ALL.into_iter().enumerate().map(move |(i, button)| {
            let left = self.panel.left + (Self::PADDING + Self::BUTTON * i as f32) * self.scale;
            let top = self.panel.top + Self::PADDING * self.scale;
            (
                button,
                D2D_RECT_F {
                    left,
                    top,
                    right: left + Self::BUTTON * self.scale,
                    bottom: top + Self::BUTTON * self.scale,
                },
            )
        })
    }

    pub fn hit(self, (x, y): (i32, i32)) -> Option<Button> {
        let (x, y) = (x as f32, y as f32);
        self.buttons().find_map(|(button, r)| {
            (x >= r.left && x < r.right && y >= r.top && y < r.bottom).then_some(button)
        })
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
            use windows::core::Interface;
            let device = super::pinned_device::get()?;
            let (context, swap) =
                device.surface(hwnd, width.max(1) as u32, height.max(1) as u32, false)?;
            let target: ID2D1RenderTarget = context.cast()?;
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
            drop(pixels);
            let brush = target.CreateSolidColorBrush(&color(0., 1.), None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let scale = dpi as f32 / 96.;
            let text = dw.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13. * scale,
                w!("zh-CN"),
            )?;
            text.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            text.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            let icons = ToolbarIcons::new(&target.GetFactory()?)?;
            Ok(Self {
                target,
                context,
                swap,
                _device: device,
                bitmap,
                brush,
                text,
                icons,
                dark: super::selection_render::system_dark(),
            })
        }
    }
    pub fn resize(&self, width: i32, height: i32) -> Result<()> {
        use windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_FLAG;
        // SAFETY: draw has ended, the context releases its only back-buffer reference before ResizeBuffers.
        unsafe {
            self.context.SetTarget(None::<&ID2D1Image>);
            self.swap.ResizeBuffers(
                2,
                width.max(1) as u32,
                height.max(1) as u32,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
            super::pinned_device::bind(&self.context, &self.swap)
        }
    }
    pub fn paint(
        &self,
        view: View,
        minimum: (i32, i32),
        dpi: u32,
        controls: Option<Controls>,
        notice: Option<&str>,
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
            if let Some(controls) = controls {
                self.draw_toolbar(Toolbar::new(width, dpi), controls);
            }
            self.target.EndDraw(None, None)?;
            self.swap
                .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT(0))
                .ok()
        }
    }
    fn draw_toolbar(&self, toolbar: Toolbar, controls: Controls) {
        let scale = toolbar.scale;
        let dark = self.dark;
        self.panel(toolbar.panel, scale);
        // SAFETY: drawing on the owning UI thread between BeginDraw and EndDraw.
        unsafe {
            for (button, rect) in toolbar.buttons() {
                let enabled = button.enabled(controls.busy);
                let hot = enabled && controls.hot == Some(button);
                if hot {
                    let bg = if controls.down == Some(button) {
                        palette::pressed(dark)
                    } else {
                        palette::selected(dark)
                    };
                    self.brush.SetColor(&rgb(bg, 1.));
                    let inset = 2. * scale;
                    self.target.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: rect.left + inset,
                                right: rect.right - inset,
                                top: rect.top + inset,
                                bottom: rect.bottom - inset,
                            },
                            radiusX: 6. * scale,
                            radiusY: 6. * scale,
                        },
                        &self.brush,
                    );
                }
                self.brush.SetColor(&rgb(
                    if hot {
                        palette::accent(dark)
                    } else {
                        palette::foreground(dark)
                    },
                    if enabled { 1. } else { 0.4 },
                ));
                self.icons.draw(
                    &self.target,
                    &self.brush,
                    button.icon(),
                    Vector2 {
                        X: (rect.left + rect.right) * 0.5,
                        Y: (rect.top + rect.bottom) * 0.5,
                    },
                    scale,
                );
            }
        }
    }
    fn panel(&self, rect: D2D_RECT_F, scale: f32) {
        // SAFETY: called only during the owning target's active draw on the UI thread.
        unsafe {
            self.brush
                .SetColor(&rgb(palette::background(self.dark), 0.97));
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: 8. * scale,
                radiusY: 8. * scale,
            };
            self.target.FillRoundedRectangle(&rounded, &self.brush);
            self.brush.SetColor(&rgb(palette::divider(self.dark), 1.));
            self.target
                .DrawRoundedRectangle(&rounded, &self.brush, 1., None);
        }
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        // SAFETY: owning UI thread, outside BeginDraw. Detach the swap-chain back buffer before resource destruction.
        unsafe {
            self.context.SetTarget(None::<&ID2D1Image>);
            let _ = self.context.Flush(None, None);
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

fn rgb(value: u32, a: f32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((value >> shift) & 255) as f32 / 255.;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a,
    }
}
