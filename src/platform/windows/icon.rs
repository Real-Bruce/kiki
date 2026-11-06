//! Shared executable resources: Windows retains these handles for the module lifetime.
use windows::{
    Win32::{Foundation::HINSTANCE, UI::WindowsAndMessaging::*},
    core::{PCWSTR, Result},
};

pub fn application(instance: HINSTANCE, small: bool) -> Result<HICON> {
    // SAFETY: resource ID 1 is embedded by app.rc in the live application module.
    // LR_SHARED gives Windows ownership; callers must not call DestroyIcon.
    unsafe {
        let (width, height) = if small {
            (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON))
        } else {
            (GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON))
        };
        LoadImageW(
            Some(instance),
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            width,
            height,
            LR_SHARED,
        )
        .map(|handle| HICON(handle.0))
    }
}
