use windows::{
    Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1},
    Win32::System::{Com::*, WinRT::*},
    core::Result,
};

/// UI STA plus a process-lifetime MTA keepalive for windows-rs agile factory caches.
pub struct Runtime {
    mta: CO_MTA_USAGE_COOKIE,
    factory: Option<IDXGIFactory1>,
}
impl Runtime {
    pub fn initialize() -> Result<Self> {
        // SAFETY: called once at main-thread entry, before COM objects or workers exist.
        unsafe {
            RoInitialize(RO_INIT_SINGLETHREADED)?;
            match CoIncrementMTAUsage() {
                Ok(mta) => {
                    // Keep the DXGI adapter/driver lifetime stable across capture sessions.
                    match CreateDXGIFactory1() {
                        Ok(factory) => Ok(Self {
                            mta,
                            factory: Some(factory),
                        }),
                        Err(error) => {
                            let _ = CoDecrementMTAUsage(mta);
                            RoUninitialize();
                            Err(error)
                        }
                    }
                }
                Err(error) => {
                    RoUninitialize();
                    Err(error)
                }
            }
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.factory = None;
        // SAFETY: all UI objects and joined capture workers are released before this guard.
        unsafe {
            let _ = CoDecrementMTAUsage(self.mta);
            RoUninitialize();
        }
    }
}
