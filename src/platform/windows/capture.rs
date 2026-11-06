use crate::core::{geometry::PixelRect, image::Image};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::*,
        Graphics::{Direct3D::*, Direct3D11::*, Dxgi::*, Gdi::*},
        System::{
            Com::*,
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
            },
        },
    },
    core::{BOOL, Interface, Result, factory},
};

fn fail(message: &str) -> windows::core::Error {
    windows::core::Error::new(E_FAIL, message)
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances this worker thread's successful CoInitializeEx.
        unsafe { CoUninitialize() };
    }
}

struct CaptureGuard {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

pub struct CaptureResult {
    pub image: Image,
    pub elapsed: Duration,
    pub monitors: usize,
}

pub fn desktop() -> Result<CaptureResult> {
    let start = Instant::now();
    // SAFETY: capture is called only on a fresh dedicated worker thread.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let _apartment = Apartment;
    ensure_sdr()?;
    if !GraphicsCaptureSession::IsSupported()? {
        return Err(fail("系统不支持 Windows Graphics Capture"));
    }
    let mut screens: Vec<(HMONITOR, PixelRect)> = Vec::new();
    // SAFETY: callback receives a live exclusive Vec for this synchronous enumeration.
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enumerate),
            LPARAM((&mut screens as *mut Vec<(HMONITOR, PixelRect)>) as isize),
        )
        .ok()?;
    }
    let first = screens.first().ok_or_else(|| fail("没有可捕获的显示器"))?.1;
    let bounds = screens.iter().fold(first, |r, (_, s)| PixelRect {
        left: r.left.min(s.left),
        top: r.top.min(s.top),
        right: r.right.max(s.right),
        bottom: r.bottom.max(s.bottom),
    });
    let mut image = Image::new(bounds).map_err(|e| fail(&e))?;
    let mut device = None;
    let mut context = None;
    // SAFETY: output COM pointers are initialized by D3D11; no borrowed GPU data escapes.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.ok_or_else(|| fail("无法创建设备"))?;
    let context = context.ok_or_else(|| fail("无法创建设备上下文"))?;
    let dxgi: IDXGIDevice = device.cast()?;
    // SAFETY: DXGI interface is backed by this worker's live D3D11 device.
    let rt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()? };
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    for (monitor, rect) in &screens {
        // SAFETY: monitor handle came from current synchronous monitor enumeration.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(*monitor)? };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &rt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            item.Size()?,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let guard = CaptureGuard { pool, session };
        guard.session.SetIsCursorCaptureEnabled(false)?;
        let (tx, rx) = mpsc::sync_channel(1);
        let token = guard
            .pool
            .FrameArrived(&TypedEventHandler::new(move |_, _| {
                let _ = tx.try_send(());
                Ok(())
            }))?;
        guard.session.StartCapture()?;
        let wait = rx.recv_timeout(Duration::from_secs(3));
        guard.pool.RemoveFrameArrived(token)?;
        wait.map_err(|_| fail("等待屏幕首帧超时，请重试"))?;
        let frame = guard.pool.TryGetNextFrame()?;
        let result = (|| {
            let size = frame.ContentSize()?;
            let (width, height) = rect.size().ok_or_else(|| fail("显示器尺寸无效"))?;
            if size.Width != width as i32 || size.Height != height as i32 {
                return Err(fail("显示器配置已变化，请重新截图"));
            }
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            // SAFETY: frame is retained through the entire copy/readback operation.
            unsafe {
                let texture: ID3D11Texture2D = access.GetInterface()?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                texture.GetDesc(&mut desc);
                if desc.Width < width
                    || desc.Height < height
                    || desc.Format != Common::DXGI_FORMAT_B8G8R8A8_UNORM
                {
                    return Err(fail("捕获纹理格式或尺寸不匹配，请重新截图"));
                }
                desc.Usage = D3D11_USAGE_STAGING;
                desc.BindFlags = 0;
                desc.MiscFlags = 0;
                desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                let mut staging = None;
                device.CreateTexture2D(&desc, None, Some(&mut staging))?;
                let staging = staging.ok_or_else(|| fail("无法创建读回纹理"))?;
                context.CopyResource(&staging, &texture);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                let result = (|| {
                    let mut screen = Image::new(*rect).map_err(|e| fail(&e))?;
                    let mapped_size = u64::from(mapped.RowPitch) * u64::from(height);
                    if mapped.pData.is_null()
                        || mapped.RowPitch < screen.stride
                        || mapped_size > isize::MAX as u64
                    {
                        return Err(fail("无效的捕获纹理步长"));
                    }
                    for y in 0..height as usize {
                        let src = std::slice::from_raw_parts(
                            (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                            screen.stride as usize,
                        );
                        let offset = y * screen.stride as usize;
                        screen.pixels[offset..offset + screen.stride as usize].copy_from_slice(src);
                        for pixel in screen.pixels[offset..offset + screen.stride as usize]
                            .chunks_exact_mut(4)
                        {
                            pixel[3] = 255;
                        }
                    }
                    image.blit(&screen);
                    Ok(())
                })();
                context.Unmap(&staging, 0);
                result
            }
        })();
        frame.Close()?;
        result?;
    }
    Ok(CaptureResult {
        image,
        elapsed: start.elapsed(),
        monitors: screens.len(),
    })
}

fn ensure_sdr() -> Result<()> {
    // SAFETY: local COM objects and output descriptors are used only on this capture thread.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut adapter_index = 0;
        loop {
            let adapter = match factory.EnumAdapters1(adapter_index) {
                Ok(adapter) => adapter,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => return Err(error),
            };
            let mut output_index = 0;
            loop {
                let output = match adapter.EnumOutputs(output_index) {
                    Ok(output) => output,
                    Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(error) => return Err(error),
                };
                let output: IDXGIOutput6 = output.cast()?;
                let desc = output.GetDesc1()?;
                if desc.AttachedToDesktop.as_bool()
                    && desc.ColorSpace != Common::DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709
                {
                    return Err(fail(
                        "当前显示器使用 HDR 或高级色彩模式；SDR 色彩兼容路径尚未验证，已停止截图以避免错误颜色。",
                    ));
                }
                output_index += 1;
            }
            adapter_index += 1;
        }
    }
    Ok(())
}

unsafe extern "system" fn enumerate(
    monitor: HMONITOR,
    _: HDC,
    rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: EnumDisplayMonitors supplies a valid RECT and the caller's exclusive Vec pointer.
    unsafe {
        let r = *rect;
        let screens = &mut *(data.0 as *mut Vec<(HMONITOR, PixelRect)>);
        // Avoid allocation panic crossing the Win32 callback boundary.
        if screens.try_reserve(1).is_err() {
            return FALSE;
        }
        screens.push((
            monitor,
            PixelRect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            },
        ));
    }
    TRUE
}
