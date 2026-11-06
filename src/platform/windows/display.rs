use crate::core::{
    display::{ColorMode, DisplayInfo},
    geometry::PixelRect,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dxgi::*, Gdi::*},
        UI::Shell::GetScaleFactorForMonitor,
    },
    core::{BOOL, Error, Interface, Result},
};

#[derive(Debug, PartialEq, Eq)]
pub struct Screen {
    pub monitor: HMONITOR,
    pub info: DisplayInfo,
}

pub fn enumerate() -> Result<Vec<Screen>> {
    let mut monitors = Vec::<HMONITOR>::new();
    // SAFETY: synchronous callback receives an exclusively borrowed vector; handles are used only in this snapshot.
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM((&mut monitors as *mut Vec<HMONITOR>) as isize),
        )
        .ok()?;
    }
    let mut screens = Vec::new();
    // SAFETY: initialized Win32 descriptors and live COM objects; no OS-owned pointers escape.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        for monitor in monitors {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast()).ok()?;
            let scale_percent = GetScaleFactorForMonitor(monitor)?.0;
            let mut mode = DEVMODEW {
                dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            EnumDisplaySettingsW(
                windows::core::PCWSTR(info.szDevice.as_ptr()),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
            .ok()?;
            let mut description = None;
            let mut adapter_index = 0;
            loop {
                let adapter = match factory.EnumAdapters1(adapter_index) {
                    Ok(adapter) => adapter,
                    Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                    Err(e) => return Err(e),
                };
                let mut output_index = 0;
                loop {
                    let output = match adapter.EnumOutputs(output_index) {
                        Ok(output) => output,
                        Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => break,
                        Err(e) => return Err(e),
                    };
                    let desc = output.cast::<IDXGIOutput6>()?.GetDesc1()?;
                    if desc.Monitor == monitor && desc.AttachedToDesktop.as_bool() {
                        description = Some(desc);
                        break;
                    }
                    output_index += 1;
                }
                if description.is_some() {
                    break;
                }
                adapter_index += 1;
            }
            let desc = description
                .ok_or_else(|| Error::new(E_FAIL, "无法确认显示器色彩模式，已停止截图"))?;
            let end = info
                .szDevice
                .iter()
                .position(|v| *v == 0)
                .unwrap_or(info.szDevice.len());
            let r = info.monitorInfo.rcMonitor;
            screens.push(Screen {
                monitor,
                info: DisplayInfo {
                    bounds: PixelRect {
                        left: r.left,
                        top: r.top,
                        right: r.right,
                        bottom: r.bottom,
                    },
                    name: String::from_utf16_lossy(&info.szDevice[..end]),
                    scale_percent,
                    rotation_degrees: match desc.Rotation.0 {
                        1 => 0,
                        2 => 90,
                        3 => 180,
                        4 => 270,
                        _ => return Err(Error::new(E_FAIL, "无法确定显示器方向")),
                    },
                    refresh_hz: mode.dmDisplayFrequency,
                    color: match desc.ColorSpace {
                        Common::DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709 => ColorMode::Sdr,
                        Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 => ColorMode::Hdr,
                        _ => ColorMode::Unsupported,
                    },
                },
            });
        }
    }
    if screens.is_empty() {
        return Err(Error::new(E_FAIL, "没有可捕获的显示器"));
    }
    Ok(screens)
}

unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: EnumDisplayMonitors invokes synchronously with the vector pointer supplied by enumerate.
    unsafe {
        let monitors = &mut *(data.0 as *mut Vec<HMONITOR>);
        if monitors.try_reserve(1).is_err() {
            return FALSE;
        }
        monitors.push(monitor);
    }
    TRUE
}
