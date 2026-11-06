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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::PixelRect;

    #[test]
    fn wic_png_roundtrip_preserves_size_bgra_and_transparent_gap() {
        let directory =
            std::env::temp_dir().join(format!("icapture-wic-test-{}", std::process::id()));
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
