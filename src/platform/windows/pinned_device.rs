//! One explicit D3D/D2D resource domain shared by preview, settings and pinned windows.
use std::{cell::RefCell, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::*,
            Direct3D11::*,
            Dxgi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::{Error, Interface, Result, w},
};
thread_local! {static DEVICE:RefCell<Option<Rc<Device>>>=const{RefCell::new(None)};}
pub struct Device {
    pub d3d: ID3D11Device,
    pub d2d: ID2D1Device,
    pub factory: IDXGIFactory2,
    dxgi: IDXGIDevice3,
    immediate: ID3D11DeviceContext,
    cleanup: HWND,
}
pub fn get() -> Result<Rc<Device>> {
    DEVICE.with_borrow_mut(|slot| {
        if let Some(device) = slot {
            // SAFETY: read-only status check on this thread's device.
            if unsafe { device.d3d.GetDeviceRemovedReason().is_ok() } {
                return Ok(device.clone());
            }
        }
        // SAFETY: WARP drawing resources stay on this UI thread and are separate from the hardware capture device.
        let value = unsafe {
            let create = |driver| -> Result<(ID3D11Device, ID3D11DeviceContext)> {
                let mut device = None;
                let mut context = None;
                D3D11CreateDevice(
                    None,
                    driver,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )?;
                Ok((
                    device.ok_or_else(|| Error::new(E_FAIL, "贴图设备创建失败"))?,
                    context.ok_or_else(|| Error::new(E_FAIL, "贴图设备上下文创建失败"))?,
                ))
            };
            let (d3d, immediate) = create(D3D_DRIVER_TYPE_WARP)?;
            let dxgi: IDXGIDevice3 = d3d.cast()?;
            dxgi.SetMaximumFrameLatency(1)?;
            let factory = dxgi.GetAdapter()?.GetParent::<IDXGIFactory2>()?;
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d = d2d_factory.CreateDevice(&dxgi)?;
            d2d.SetMaximumTextureMemory(16 * 1024 * 1024);
            Rc::new(Device {
                d3d,
                d2d,
                factory,
                dxgi,
                immediate,
                cleanup: cleanup_window()?,
            })
        };
        *slot = Some(value.clone());
        Ok(value)
    })
}
pub fn idle() {
    DEVICE.with_borrow(|slot| {
        if let Some(device) = slot {
            if Rc::strong_count(device) != 1 {
                return;
            }
            device.release_caches();
            // SAFETY: owning message-only HWND schedules one cache cleanup after pending GPU work completes.
            unsafe {
                let _ = SetTimer(Some(device.cleanup), 1, 200, None);
            }
        }
    });
}
#[cfg(feature = "diagnostics")]
pub fn memory() -> Option<(u64, u64)> {
    DEVICE.with_borrow(|slot| {
        let device = slot.as_ref()?;
        // SAFETY: read-only process GPU usage query on node zero of the owning adapter.
        unsafe {
            let adapter: IDXGIAdapter3 = device.dxgi.GetAdapter().ok()?.cast().ok()?;
            let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            let mut nonlocal = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local)
                .ok()?;
            adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut nonlocal)
                .ok()?;
            Some((local.CurrentUsage, nonlocal.CurrentUsage))
        }
    })
}
impl Device {
    fn release_caches(&self) {
        // SAFETY: UI thread calls only after the final pinned surface drops or while destroying this device. These APIs discard unused caches and unbind submitted resources.
        unsafe {
            self.immediate.ClearState();
            self.immediate.Flush();
            self.d2d.ClearResources(0);
            self.dxgi.Trim();
        }
    }
}
pub fn shutdown() {
    let device = DEVICE.with_borrow_mut(Option::take);
    drop(device);
}
impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: exclusively owned message-only window; destruction cancels its timer before device interfaces release.
        unsafe {
            let _ = DestroyWindow(self.cleanup);
        }
        self.release_caches();
    }
}

fn cleanup_window() -> Result<HWND> {
    // SAFETY: static callback, process module and UI-thread message-only window. Class reuse across test threads is intentional.
    unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW {
            hInstance: instance,
            lpfnWndProc: Some(cleanup_proc),
            lpszClassName: w!("kiki.PinnedCleanup"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(Error::from_win32());
        }
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class.lpszClassName,
            w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
    }
}
unsafe extern "system" fn cleanup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: no window state pointer is retained; the slot clone is released before returning. No panic crosses the callback.
    unsafe {
        if msg == WM_TIMER && wp.0 == 1 {
            let _ = KillTimer(Some(hwnd), 1);
            let _ = std::panic::catch_unwind(|| {
                let retired = DEVICE.with_borrow_mut(|slot| {
                    if slot
                        .as_ref()
                        .is_some_and(|device| Rc::strong_count(device) == 1)
                    {
                        slot.take()
                    } else {
                        None
                    }
                });
                drop(retired);
            });
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

pub(super) fn bind(
    context: &ID2D1DeviceContext,
    swap: &windows::Win32::Graphics::Dxgi::IDXGISwapChain1,
) -> Result<()> {
    // SAFETY: buffer zero belongs to this live swap chain; context retains its bitmap until resize or drop.
    unsafe {
        let surface: windows::Win32::Graphics::Dxgi::IDXGISurface = swap.GetBuffer(0)?;
        let desc = swap.GetDesc1()?;
        let mut options = D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW;
        if desc.Flags & DXGI_SWAP_CHAIN_FLAG_GDI_COMPATIBLE.0 as u32 != 0 {
            options |= D2D1_BITMAP_OPTIONS_GDI_COMPATIBLE;
        }
        let bitmap = context.CreateBitmapFromDxgiSurface(
            &surface,
            Some(&D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.,
                dpiY: 96.,
                bitmapOptions: options,
                ..Default::default()
            }),
        )?;
        context.SetTarget(&bitmap);
        Ok(())
    }
}

impl Device {
    /// Create an independent HWND surface while sharing the bounded device caches.
    pub fn surface(
        &self,
        hwnd: HWND,
        width: u32,
        height: u32,
        gdi: bool,
    ) -> Result<(ID2D1DeviceContext, IDXGISwapChain1)> {
        // SAFETY: this device and live HWND belong to the calling UI thread; the context retains the back buffer until detach/drop.
        unsafe {
            use windows::Win32::Graphics::Dxgi::Common::*;
            let context = self
                .d2d
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            context.SetDpi(96., 96.);
            let swap = self.factory.CreateSwapChainForHwnd(
                &self.d3d,
                hwnd,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: width.max(1),
                    Height: height.max(1),
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    Flags: if gdi {
                        DXGI_SWAP_CHAIN_FLAG_GDI_COMPATIBLE.0 as u32
                    } else {
                        0
                    },
                    ..Default::default()
                },
                None,
                None,
            )?;
            self.factory
                .MakeWindowAssociation(hwnd, DXGI_MWA_NO_WINDOW_CHANGES | DXGI_MWA_NO_ALT_ENTER)?;
            bind(&context, &swap)?;
            Ok((context, swap))
        }
    }
}
