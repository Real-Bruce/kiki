use crate::core::image::Image;
use std::path::Path;
use windows::{
    Win32::{Foundation::E_FAIL, Graphics::Imaging::*, System::Com::*},
    core::{Error, PCWSTR, Result},
};

pub fn save(image: &Image, path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: synchronous WIC calls use live slices and COM objects on this thread.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialized.is_err() && initialized != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
            initialized.ok()?;
        }
        let result = (|| {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = factory.CreateStream()?;
            stream.InitializeFromFilename(PCWSTR(path.as_ptr()), 0x40000000)?;
            let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.ok_or_else(|| Error::new(E_FAIL, "无法创建 PNG 帧"))?;
            frame.Initialize(None)?;
            let (width, height) = image
                .bounds()
                .size()
                .ok_or_else(|| Error::new(E_FAIL, "无效图像"))?;
            frame.SetSize(width, height)?;
            let mut format = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut format)?;
            if format != GUID_WICPixelFormat32bppBGRA {
                return Err(Error::new(E_FAIL, "PNG 编码格式不匹配"));
            }
            frame.WritePixels(height, image.stride(), image.pixels())?;
            frame.Commit()?;
            encoder.Commit()
        })();
        if initialized.is_ok() {
            CoUninitialize();
        }
        result
    }
}

pub fn encode(image: &Image) -> Result<Vec<u8>> {
    let _apartment = super::composition::Apartment::enter()?;
    // SAFETY: in-memory COM stream and WIC objects belong to this worker; stream owns and releases its global allocation.
    unsafe {
        let stream = StructuredStorage::CreateStreamOnHGlobal(
            windows::Win32::Foundation::HGLOBAL::default(),
            true,
        )?;
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or_else(|| Error::new(E_FAIL, "无法创建 PNG 帧"))?;
        frame.Initialize(None)?;
        let (width, height) = image
            .bounds()
            .size()
            .ok_or_else(|| Error::new(E_FAIL, "图像尺寸无效"))?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        if format != GUID_WICPixelFormat32bppBGRA {
            return Err(Error::new(E_FAIL, "PNG 格式不匹配"));
        }
        frame.WritePixels(height, image.stride(), image.pixels())?;
        frame.Commit()?;
        encoder.Commit()?;
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
        let bytes = encode(&image).unwrap();
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
        save(&image, &file).unwrap();
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: test initializes COM on its own thread and all COM objects drop before uninitialization.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            {
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
                assert_eq!((width, height), (2, 2));
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
                let mut pixels = vec![0; 16];
                converter
                    .CopyPixels(std::ptr::null(), 8, &mut pixels)
                    .unwrap();
                assert_eq!(pixels, image.pixels());
            }
            CoUninitialize();
        }
        assert!(save(&image, &directory).is_err());
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
