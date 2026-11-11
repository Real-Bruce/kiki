//! Clean BGRA output using the same annotation painter as the HWND preview.
use super::annotation_render;
use crate::core::{annotation::Editor, geometry::PixelRect, image::Image};
use windows::{
    Win32::{
        Foundation::{E_FAIL, RPC_E_CHANGED_MODE},
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
            Imaging::*,
        },
        System::Com::*,
    },
    core::{Error, Result},
};
pub(super) struct Apartment(bool);
impl Apartment {
    pub(super) fn enter() -> Result<Self> {
        // SAFETY: initialize COM on the calling worker, preserving an existing STA.
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if status.is_err() && status != RPC_E_CHANGED_MODE {
            status.ok()?;
        }
        Ok(Self(status.is_ok()))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: paired with this thread's successful initialization, after local COM resources have dropped.
            unsafe {
                CoUninitialize();
            }
        }
    }
}
fn fail(message: &str) -> Error {
    Error::new(E_FAIL, message)
}
pub fn render(
    source: &Image,
    selection: PixelRect,
    editor: &Editor,
    corner_radius: u16,
) -> Result<Image> {
    let _apartment = Apartment::enter()?;
    let mut output = source.region(selection).map_err(|e| fail(&e))?;
    let (width, height) = selection.size().ok_or_else(|| fail("无效输出范围"))?;
    let lock_width = i32::try_from(width).map_err(|_| fail("输出范围过宽"))?;
    let lock_height = i32::try_from(height).map_err(|_| fail("输出范围过高"))?;
    // SAFETY: all factories, textures and locks are confined to this thread and drop before COM. Image allocations are checked and bounded by Image::new.
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let backing = wic.CreateBitmap(
            width,
            height,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnLoad,
        )?;
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let target = factory.CreateWicBitmapRenderTarget(
            &backing,
            &D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.,
                dpiY: 96.,
                ..Default::default()
            },
        )?;
        // D2D expects premultiplied pixels, while Image and PNG expose straight BGRA.
        for pixel in output.pixels_mut().chunks_exact_mut(4) {
            for channel in 0..3 {
                pixel[channel] =
                    ((u16::from(pixel[channel]) * u16::from(pixel[3]) + 127) / 255) as u8;
            }
        }
        let bitmap = target.CreateBitmap(
            D2D_SIZE_U { width, height },
            Some(output.pixels().as_ptr().cast()),
            output.stride(),
            &D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.,
                dpiY: 96.,
            },
        )?;
        let brush = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 1.,
            },
            None,
        )?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let layouts = annotation_render::layouts(&dw, editor)?;
        let geometry = annotation_render::geometry(&factory, selection, editor)?;
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F::default()));
        target.DrawBitmap(
            &bitmap,
            None,
            1.,
            D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            None,
        );
        annotation_render::draw(
            &target,
            &brush,
            selection,
            editor,
            (&layouts, &geometry),
            None,
        );
        target.EndDraw(None, None)?;
        drop(target);
        let lock = backing.Lock(
            &WICRect {
                X: 0,
                Y: 0,
                Width: lock_width,
                Height: lock_height,
            },
            WICBitmapLockRead.0 as u32,
        )?;
        let stride = lock.GetStride()?;
        let mut length = 0;
        let mut data = std::ptr::null_mut();
        lock.GetDataPointer(&mut length, &mut data)?;
        let required = u64::from(stride)
            .checked_mul(u64::from(height - 1))
            .and_then(|v| v.checked_add(u64::from(output.stride())))
            .ok_or_else(|| fail("成图步长溢出"))?;
        if data.is_null()
            || stride < output.stride()
            || required > u64::from(length)
            || required > isize::MAX as u64
        {
            return Err(fail("无效成图缓冲区"));
        }
        for y in 0..height as usize {
            let row =
                std::slice::from_raw_parts(data.add(y * stride as usize), output.stride() as usize);
            let offset = y * output.stride() as usize;
            output.pixels_mut()[offset..offset + row.len()].copy_from_slice(row);
        }
        for pixel in output.pixels_mut().chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            if alpha == 0 {
                pixel.fill(0);
            } else if alpha < 255 {
                for channel in pixel.iter_mut().take(3) {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
    }
    output.round_corners(corner_radius);
    Ok(output)
}
#[cfg(any(feature = "diagnostics", test))]
pub(super) fn fixture() -> Result<(Image, Editor)> {
    fixture_at(-160, -90)
}
#[cfg(any(feature = "diagnostics", test))]
fn fixture_at(left: i32, top: i32) -> Result<(Image, Editor)> {
    use crate::core::{
        annotation::{Object, Shape, Style, Tool},
        selection::PixelPoint,
    };
    let bounds = PixelRect {
        left,
        top,
        right: left.checked_add(640).ok_or_else(|| fail("诊断范围溢出"))?,
        bottom: top.checked_add(360).ok_or_else(|| fail("诊断范围溢出"))?,
    };
    let mut image = Image::new(bounds).map_err(|e| fail(&e))?;
    for pixel in image.pixels_mut().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[248, 248, 248, 255]);
    }
    for y in 0..360usize {
        for x in 440..480usize {
            let i = y * image.stride() as usize + x * 4;
            image.pixels_mut()[i..i + 4].fill(0);
        }
    }
    let mut editor = Editor::default();
    for (tool, a, b) in [
        (Tool::Rectangle, (-120, -50), (80, 40)),
        (Tool::Arrow, (-100, 160), (350, 70)),
        (Tool::Pen, (-80, 100), (200, 120)),
    ] {
        editor.tool = tool;
        editor
            .begin(
                PixelPoint {
                    x: left + a.0 + 160,
                    y: top + a.1 + 90,
                },
                6,
                1.25,
            )
            .map_err(fail)?;
        editor.finish(PixelPoint {
            x: left + b.0 + 160,
            y: top + b.1 + 90,
        });
    }
    editor
        .commit_text(
            None,
            Object {
                shape: Shape::Text {
                    bounds: PixelRect {
                        left: left + 260,
                        top: top + 50,
                        right: left + 580,
                        bottom: top + 140,
                    },
                    content: "中文标注\n第二行 ABC 123".into(),
                    size: 30.,
                    wrap_width: 320.,
                    angle: 0.41,
                },
                style: Style {
                    color: [0, 90, 180],
                    ..Default::default()
                },
            },
        )
        .map_err(fail)?;
    Ok((image, editor))
}
#[cfg(feature = "diagnostics")]
pub fn probe(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| fail(&e.to_string()))?;
    let (source, editor) = fixture()?;
    super::png::save(
        &render(&source, source.bounds(), &editor, 0)?,
        &path.join("fixture-annotated.png"),
    )?;
    let capture = std::thread::Builder::new()
        .name("annotation-probe".into())
        .spawn(super::capture::desktop)
        .map_err(|e| fail(&e.to_string()))?
        .join()
        .map_err(|_| fail("诊断捕获线程退出"))??;
    let bounds = capture.image.bounds();
    let crop = PixelRect {
        left: bounds.left,
        top: bounds.top,
        right: bounds.left + (bounds.right - bounds.left).min(640),
        bottom: bounds.top + (bounds.bottom - bounds.top).min(360),
    };
    let (_, desktop_editor) = fixture_at(crop.left, crop.top)?;
    super::png::save(
        &render(&capture.image, crop, &desktop_editor, 0)?,
        &path.join("desktop-annotated.png"),
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clean_render_crop_invariance_base_ownership_and_transparent_gap() {
        let (source, mut editor) = fixture().unwrap();
        let original = source.pixels().to_vec();
        let all = render(&source, source.bounds(), &editor, 0).unwrap();
        editor.selected = None;
        assert_eq!(
            all.pixels(),
            render(&source, source.bounds(), &editor, 0)
                .unwrap()
                .pixels()
        );
        let crop = PixelRect {
            left: -60,
            top: -20,
            right: 340,
            bottom: 190,
        };
        let small = render(&source, crop, &editor, 0).unwrap();
        assert_eq!(small.pixels(), all.region(crop).unwrap().pixels());
        assert_eq!(source.pixels(), original);
        assert_eq!(&all.pixels()[0..4], &original[0..4]);
        let gap = 320 * all.stride() as usize + 450 * 4;
        assert_eq!(&all.pixels()[gap..gap + 4], &[0; 4]);
        assert!(all.pixels().chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        let empty = render(&source, source.bounds(), &Editor::default(), 0).unwrap();
        assert_eq!(empty.pixels(), source.pixels());
    }
}
