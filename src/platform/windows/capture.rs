use super::display;
use crate::core::{geometry::PixelRect, image::Image};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
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
        Graphics::{Direct3D::*, Direct3D11::*, Dxgi::*},
        System::{
            Com::*,
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
            },
        },
    },
    core::{Interface, Result, factory},
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
    #[cfg(feature = "diagnostics")]
    pub elapsed: Duration,
    pub screens: Vec<crate::core::display::DisplayInfo>,
    pub windows: Vec<PixelRect>,
}

#[cfg(feature = "diagnostics")]
pub fn desktop() -> Result<CaptureResult> {
    desktop_with_cancel(&AtomicBool::new(false))
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(windows::core::Error::new(E_ABORT, "截图已取消"));
    }
    Ok(())
}

pub fn desktop_with_cancel(cancel: &AtomicBool) -> Result<CaptureResult> {
    desktop_inner(cancel, || {})
}

#[cfg(feature = "diagnostics")]
pub fn cancel_after_start() -> Result<()> {
    let cancel = AtomicBool::new(false);
    match desktop_inner(&cancel, || cancel.store(true, Ordering::Release)) {
        Err(error) if error.code() == E_ABORT => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(fail("取消后仍返回了捕获结果")),
    }
}

fn desktop_inner(cancel: &AtomicBool, on_started: impl FnOnce()) -> Result<CaptureResult> {
    #[cfg(feature = "diagnostics")]
    let start = Instant::now();
    let mut on_started = Some(on_started);
    check_cancel(cancel)?;
    // SAFETY: capture is called only on a fresh dedicated worker thread.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
    }
    let _apartment = Apartment;
    let screens = display::enumerate()?;
    let windows = super::window_targets::enumerate()?;
    for screen in &screens {
        if screen.info.color != crate::core::display::ColorMode::Sdr {
            return Err(fail(
                "当前显示器使用 HDR 或高级色彩模式；SDR 色彩路径尚未验证，已停止截图以避免错误颜色。",
            ));
        }
    }
    if !GraphicsCaptureSession::IsSupported()? {
        return Err(fail("系统不支持 Windows Graphics Capture"));
    }
    let first = screens
        .first()
        .ok_or_else(|| fail("没有可捕获的显示器"))?
        .info
        .bounds;
    let bounds = screens.iter().fold(first, |r, screen| {
        let s = screen.info.bounds;
        PixelRect {
            left: r.left.min(s.left),
            top: r.top.min(s.top),
            right: r.right.max(s.right),
            bottom: r.bottom.max(s.bottom),
        }
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
    for screen in &screens {
        check_cancel(cancel)?;
        let monitor = screen.monitor;
        let rect = &screen.info.bounds;
        // SAFETY: monitor handle came from current synchronous monitor enumeration.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
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
        if let Some(notify) = on_started.take() {
            notify();
        }
        let wait = (|| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                check_cancel(cancel)?;
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) => return Ok(()),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(fail("捕获通知已断开"));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                        return Err(fail("等待屏幕首帧超时，请重试"));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        })();
        guard.pool.RemoveFrameArrived(token)?;
        wait?;
        check_cancel(cancel)?;
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
                    // Copy the mapped monitor directly into its physical desktop region.
                    // No additional full-monitor CPU image is needed alongside the desktop buffer.
                    let row_bytes = width as usize * 4;
                    let mapped_size = u64::from(mapped.RowPitch) * u64::from(height);
                    if mapped.pData.is_null()
                        || (mapped.RowPitch as usize) < row_bytes
                        || mapped_size > isize::MAX as u64
                    {
                        return Err(fail("无效的捕获纹理步长"));
                    }
                    let dst_x = usize::try_from(i64::from(rect.left) - i64::from(bounds.left))
                        .map_err(|_| fail("显示器横向范围无效"))?
                        * 4;
                    let dst_y = usize::try_from(i64::from(rect.top) - i64::from(bounds.top))
                        .map_err(|_| fail("显示器纵向范围无效"))?;
                    let dst_stride = image.stride() as usize;
                    for y in 0..height as usize {
                        if y % 64 == 0 {
                            check_cancel(cancel)?;
                        }
                        let src = std::slice::from_raw_parts(
                            mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                            row_bytes,
                        );
                        let offset = (dst_y + y) * dst_stride + dst_x;
                        let row = image
                            .pixels_mut()
                            .get_mut(offset..offset + row_bytes)
                            .ok_or_else(|| fail("显示器范围超出桌面图像"))?;
                        row.copy_from_slice(src);
                        for pixel in row.chunks_exact_mut(4) {
                            pixel[3] = 255;
                        }
                    }
                    Ok(())
                })();
                context.Unmap(&staging, 0);
                result
            }
        })();
        frame.Close()?;
        result?;
    }
    check_cancel(cancel)?;
    if display::enumerate()? != screens {
        return Err(fail("截图期间显示配置已变化，请重新截图"));
    }
    Ok(CaptureResult {
        image,
        #[cfg(feature = "diagnostics")]
        elapsed: start.elapsed(),
        screens: screens.into_iter().map(|screen| screen.info).collect(),
        windows,
    })
}
