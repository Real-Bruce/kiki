//! WIC PNG and JPEG encoding. JPEG scanlines are flattened onto white in bounded batches.
use crate::core::{
    config::OutputFormat,
    image::{Image, blend_over_white},
};
use std::path::Path;
use windows::{
    Win32::{
        Foundation::E_FAIL,
        Graphics::Imaging::*,
        System::{
            Com::{StructuredStorage::PROPBAG2, *},
            Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_R4},
        },
    },
    core::{Error, PCWSTR, PWSTR, Result},
};

const JPEG_QUALITY: f32 = 0.90;

pub fn save(image: &Image, path: &Path, format: OutputFormat) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let _apartment = super::composition::Apartment::enter()?;
    // SAFETY: WIC objects stay on this COM thread and the path is NUL-terminated.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromFilename(PCWSTR(path.as_ptr()), 0x40000000)?;
        write_frame(&factory, &stream, image, format)
    }
}

fn write_frame(
    factory: &IWICImagingFactory,
    stream: &IStream,
    image: &Image,
    format: OutputFormat,
) -> Result<()> {
    let (width, height) = image
        .bounds()
        .size()
        .ok_or_else(|| Error::new(E_FAIL, "无效图像"))?;
    let (container, pixel_format) = match format {
        OutputFormat::Png => (GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA),
        OutputFormat::Jpeg => {
            if width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
                return Err(Error::new(E_FAIL, "JPEG 的宽和高最多为 65535 像素"));
            }
            (GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR)
        }
    };
    // SAFETY: COM interfaces are local to the encoding thread. Frame properties and
    // pixel slices remain valid for each synchronous WIC call.
    unsafe {
        let encoder = factory.CreateEncoder(&container, std::ptr::null())?;
        encoder.Initialize(stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        let mut options = None;
        encoder.CreateNewFrame(&mut frame, &mut options)?;
        let frame = frame.ok_or_else(|| Error::new(E_FAIL, "无法创建图像编码帧"))?;
        if format == OutputFormat::Jpeg {
            let options = options
                .as_ref()
                .ok_or_else(|| Error::new(E_FAIL, "无法设置 JPEG 品质"))?;
            let mut name: Vec<_> = "ImageQuality".encode_utf16().chain(Some(0)).collect();
            let property = PROPBAG2 {
                pstrName: PWSTR(name.as_mut_ptr()),
                ..Default::default()
            };
            let quality = VARIANT {
                Anonymous: VARIANT_0 {
                    Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                        vt: VT_R4,
                        Anonymous: VARIANT_0_0_0 {
                            fltVal: JPEG_QUALITY,
                        },
                        ..Default::default()
                    }),
                },
            };
            options.Write(1, &property, &quality)?;
        }
        frame.Initialize(options.as_ref())?;
        frame.SetSize(width, height)?;
        let mut actual = pixel_format;
        frame.SetPixelFormat(&mut actual)?;
        if actual != pixel_format {
            return Err(Error::new(
                E_FAIL,
                format!("{} 编码像素格式不匹配", format.label()),
            ));
        }
        match format {
            OutputFormat::Png => frame.WritePixels(height, image.stride(), image.pixels())?,
            OutputFormat::Jpeg => write_jpeg_pixels(&frame, image, width, height)?,
        }
        frame.Commit()?;
        encoder.Commit()
    }
}

fn write_jpeg_pixels(
    frame: &IWICBitmapFrameEncode,
    image: &Image,
    width: u32,
    height: u32,
) -> Result<()> {
    // The caller checked the 16-bit JPEG dimensions, and Image owns complete BGRA rows.
    let stride = width as usize * 3;
    let rows = (1024 * 1024 / stride).max(1).min(height as usize);
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(rows * stride)
        .map_err(|_| Error::new(E_FAIL, "没有足够内存编码 JPEG"))?;
    pixels.resize(rows * stride, 0);
    let source_stride = image.stride() as usize;
    for input in image.pixels().chunks(rows * source_stride) {
        let lines = input.len() / source_stride;
        let output = &mut pixels[..lines * stride];
        for (bgra, bgr) in input.chunks_exact(4).zip(output.chunks_exact_mut(3)) {
            for (channel, value) in bgr.iter_mut().zip(&bgra[..3]) {
                *channel = blend_over_white(*value, bgra[3]);
            }
        }
        // SAFETY: output contains exactly lines complete BGR scanlines and WIC reads them synchronously.
        unsafe { frame.WritePixels(lines as u32, stride as u32, output)? };
    }
    Ok(())
}

pub fn encode_png(image: &Image) -> Result<Vec<u8>> {
    let _apartment = super::composition::Apartment::enter()?;
    // SAFETY: in-memory COM stream and WIC objects belong to this worker; stream owns and releases its global allocation.
    unsafe {
        let stream = StructuredStorage::CreateStreamOnHGlobal(
            windows::Win32::Foundation::HGLOBAL::default(),
            true,
        )?;
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        write_frame(&factory, &stream, image, OutputFormat::Png)?;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        let length = usize::try_from(stat.cbSize).map_err(|_| Error::new(E_FAIL, "PNG 过大"))?;
        if length > 600 * 1024 * 1024 {
            return Err(Error::new(E_FAIL, "PNG 超过内存限制"));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::new(E_FAIL, "没有足够内存编码 PNG"))?;
        bytes.resize(length, 0);
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut read = 0;
        stream
            .Read(bytes.as_mut_ptr().cast(), length as u32, Some(&mut read))
            .ok()?;
        if read as usize != length {
            return Err(Error::new(E_FAIL, "PNG 读取不完整"));
        }
        Ok(bytes)
    }
}
#[cfg(test)]
pub(super) use tests::decode_file;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::PixelRect;

    #[test]
    fn memory_png_preserves_straight_alpha_and_pixels() {
        let mut image = Image::new(PixelRect {
            left: 0,
            top: 0,
            right: 3,
            bottom: 1,
        })
        .unwrap();
        image
            .pixels_mut()
            .copy_from_slice(&[0, 0, 255, 255, 50, 100, 200, 128, 0, 0, 0, 0]);
        let bytes = encode_png(&image).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let _apartment = super::super::composition::Apartment::enter().unwrap();
        // SAFETY: this test retains encoded storage and all COM resources through synchronous decoding on the same thread.
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap();
            let stream = factory.CreateStream().unwrap();
            stream.InitializeFromMemory(&bytes).unwrap();
            let decoder = factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)
                .unwrap();
            let converter = factory.CreateFormatConverter().unwrap();
            converter
                .Initialize(
                    &decoder.GetFrame(0).unwrap(),
                    &GUID_WICPixelFormat32bppBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.,
                    WICBitmapPaletteTypeCustom,
                )
                .unwrap();
            let mut decoded = [0; 12];
            converter
                .CopyPixels(std::ptr::null(), 12, &mut decoded)
                .unwrap();
            assert_eq!(decoded, image.pixels());
        }
    }
    #[test]
    fn wic_png_roundtrip_preserves_size_bgra_and_transparent_gap() {
        let directory = std::env::temp_dir().join(format!("kiki-wic-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("roundtrip.png");
        let mut image = Image::new(PixelRect {
            left: -1,
            top: 0,
            right: 1,
            bottom: 2,
        })
        .unwrap();
        image
            .pixels_mut()
            .copy_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255]);
        save(&image, &file, OutputFormat::Png).unwrap();
        let (size, pixels) = decode_file(&file);
        assert_eq!(size, (2, 2));
        assert_eq!(pixels, image.pixels());
        assert!(save(&image, &directory, OutputFormat::Png).is_err());
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    pub(in crate::platform::windows) fn decode_file(file: &Path) -> ((u32, u32), Vec<u8>) {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
        let _apartment = super::super::composition::Apartment::enter().unwrap();
        // SAFETY: input path and the destination buffer remain valid through the synchronous decoder calls.
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap();
            let decoder = factory
                .CreateDecoderFromFilename(
                    PCWSTR(wide.as_ptr()),
                    None,
                    windows::Win32::Foundation::GENERIC_READ,
                    WICDecodeMetadataCacheOnLoad,
                )
                .unwrap();
            let frame = decoder.GetFrame(0).unwrap();
            let (mut width, mut height) = (0, 0);
            frame.GetSize(&mut width, &mut height).unwrap();
            let converter = factory.CreateFormatConverter().unwrap();
            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.,
                    WICBitmapPaletteTypeCustom,
                )
                .unwrap();
            let mut pixels = vec![0; (width * height * 4) as usize];
            converter
                .CopyPixels(std::ptr::null(), width * 4, &mut pixels)
                .unwrap();
            ((width, height), pixels)
        }
    }

    #[test]
    fn jpeg_white_matte_preserves_color_and_dimensions_across_batches() {
        let directory = std::env::temp_dir().join(format!("kiki-jpeg-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("白底 圆角.jpeg");
        let mut image = Image::new(PixelRect {
            left: -160,
            top: -90,
            right: 481,
            bottom: 550,
        })
        .unwrap();
        let stride = image.stride() as usize;
        for (y, row) in image.pixels_mut().chunks_exact_mut(stride).enumerate() {
            for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                let value = if (264..360).contains(&x) && (200..280).contains(&y) {
                    [12, 44, 70, 0]
                } else if (264..360).contains(&x) && (432..512).contains(&y) {
                    [40, 70, 100, 128]
                } else {
                    [60, 120, 200, 255]
                };
                pixel.copy_from_slice(&value);
            }
        }
        image.round_corners(256);
        let original = image.pixels().to_vec();
        save(&image, &file, OutputFormat::Jpeg).unwrap();
        assert_eq!(image.pixels(), original);
        assert!(std::fs::read(&file).unwrap().starts_with(b"\xff\xd8\xff"));
        let (size, pixels) = decode_file(&file);
        assert_eq!(size, (641, 640));
        for (x, y, expected) in [
            (0, 0, [255, 255, 255, 255]),
            (640, 639, [255, 255, 255, 255]),
            (320, 240, [255, 255, 255, 255]),
            (320, 472, [147, 162, 177, 255]),
            (320, 587, [60, 120, 200, 255]),
        ] {
            let at = y * stride + x * 4;
            for (actual, expected) in pixels[at..at + 4].iter().zip(expected) {
                assert!(actual.abs_diff(expected) <= 4, "pixel ({x}, {y})");
            }
        }
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
