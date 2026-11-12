//! Explicit developer probe. Never runs during ordinary tray operation.
use super::capture;
use std::{io::Write, path::PathBuf};
use windows::{
    Win32::{
        Foundation::*,
        System::{
            ProcessStatus::*, SystemInformation::GetSystemTimePreciseAsFileTime, Threading::*,
        },
        UI::{HiDpi::GetDpiForSystem, WindowsAndMessaging::*},
    },
    core::{Error, Result},
};

pub fn run_if_requested() -> Result<bool> {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == "--long-performance-probe")
    {
        let path = args
            .get(2)
            .map(PathBuf::from)
            .ok_or_else(|| Error::new(E_FAIL, "需要长截图性能输出目录"))?;
        super::performance::long_images(&path)?;
        return Ok(true);
    }
    if args.get(1).is_some_and(|arg| arg == "--isolation-probe") {
        let path = args
            .get(2)
            .map(PathBuf::from)
            .ok_or_else(|| Error::new(E_FAIL, "需要输出目录"))?;
        super::performance::isolate(&path)?;
        return Ok(true);
    }
    if args
        .get(1)
        .is_some_and(|arg| arg == "--inspect-output-clipboard")
    {
        let path = args
            .get(2)
            .map(PathBuf::from)
            .ok_or_else(|| Error::new(E_FAIL, "需要输出目录"))?;
        super::output::inspect_clipboard(&path)?;
        return Ok(true);
    }
    if args.get(1).is_some_and(|arg| arg == "--pin-stress") {
        let path = args
            .get(2)
            .map(PathBuf::from)
            .ok_or_else(|| Error::new(E_FAIL, "需要输出目录"))?;
        super::pinned::stress(&path)?;
        return Ok(true);
    }
    if args.get(1).is_some_and(|arg| arg == "--annotation-probe") {
        let path = args
            .get(2)
            .map(PathBuf::from)
            .ok_or_else(|| Error::new(E_FAIL, "--annotation-probe 需要输出目录"))?;
        super::composition::probe(&path)?;
        return Ok(true);
    }
    if args
        .get(1)
        .is_none_or(|arg| arg != "--stress" && arg != "--cancel-stress")
    {
        return Ok(false);
    }
    let path = args
        .get(2)
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(E_FAIL, "--stress 需要输出目录"))?;
    std::fs::create_dir_all(&path).map_err(io_error)?;
    let cancel = args.get(1).is_some_and(|arg| arg == "--cancel-stress");
    std::thread::Builder::new()
        .name("stress-probe".into())
        .spawn(move || stress(path, cancel))
        .map_err(io_error)?
        .join()
        .map_err(|_| Error::new(E_FAIL, "诊断线程意外退出"))??;
    Ok(true)
}

fn stress(path: PathBuf, cancel: bool) -> Result<()> {
    let mut csv = std::fs::File::create(path.join("stress.csv")).map_err(io_error)?;
    writeln!(
        csv,
        "iteration,capture_ms,private_bytes,total_working_set,handles,gdi,user"
    )
    .map_err(io_error)?;
    // Include baseline and every result; no working set trimming or sample filtering.
    row(&mut csv, 0, 0.)?;
    for index in 1..=100 {
        let elapsed = if cancel {
            let started = std::time::Instant::now();
            capture::cancel_after_start()?;
            started.elapsed().as_secs_f64() * 1000.
        } else {
            let image = capture::desktop()?;
            image.elapsed.as_secs_f64() * 1000.
        };
        row(&mut csv, index, elapsed)?;
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    row(&mut csv, 101, 0.)?;
    csv.flush().map_err(io_error)?;
    if cancel {
        // Recovery must work in the same worker/process after all canceled sessions release.
        let recovered = capture::desktop()?;
        super::image_encode::save(
            &recovered.image,
            &path.join("recovered.png"),
            crate::core::config::OutputFormat::Png,
        )?;
    }
    Ok(())
}

pub(super) fn row(csv: &mut std::fs::File, index: u32, elapsed: f64) -> Result<()> {
    // SAFETY: initialized output counters and current-process pseudo handle; no handle ownership transfer.
    unsafe {
        let process = GetCurrentProcess();
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        GetProcessMemoryInfo(
            process,
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )?;
        let mut handles = 0;
        GetProcessHandleCount(process, &mut handles)?;
        writeln!(
            csv,
            "{index},{elapsed:.3},{},{},{handles},{},{}",
            counters.PrivateUsage,
            counters.WorkingSetSize,
            GetGuiResources(process, GR_GDIOBJECTS),
            GetGuiResources(process, GR_USEROBJECTS)
        )
        .map_err(io_error)?;
    }
    Ok(())
}
fn io_error(e: std::io::Error) -> Error {
    Error::new(E_FAIL, e.to_string())
}

pub fn start_readiness_probe(
    hwnd: HWND,
    registered: bool,
) -> Result<Option<std::thread::JoinHandle<Result<()>>>> {
    let mut args = std::env::args_os();
    if args.nth(1).is_none_or(|arg| arg != "--startup-probe") {
        return Ok(None);
    }
    let folder = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(E_FAIL, "--startup-probe 需要输出目录"))?;
    // SAFETY: output FILETIMEs are initialized; this is a read-only current-process snapshot.
    let (elapsed_ms, dpi) = unsafe {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )?;
        let now = GetSystemTimePreciseAsFileTime();
        let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        let delta = ticks(now)
            .checked_sub(ticks(creation))
            .ok_or_else(|| Error::new(E_FAIL, "采样期间系统时钟发生回拨"))?;
        (delta as f64 / 10_000., GetDpiForSystem())
    };
    let window_bits = hwnd.0 as usize;
    let worker = std::thread::Builder::new().name("startup-probe".into()).spawn(move || {
        let result = (|| {
            std::fs::create_dir_all(&folder).map_err(io_error)?;
            std::fs::write(folder.join("startup.csv"), format!("ready_ms,hotkey_registered,system_dpi\n{elapsed_ms:.3},{registered},{dpi}\n")).map_err(io_error)
        })();
        // SAFETY: hidden window belongs to this diagnostic process; UI waits for this worker on exit.
        unsafe { PostMessageW(Some(HWND(window_bits as *mut _)),WM_CLOSE,WPARAM(0),LPARAM(0))?; }
        result
    }).map_err(io_error)?;
    Ok(Some(worker))
}

thread_local! {static PIN_OBSERVER:std::cell::RefCell<Option<(std::fs::File,u32,bool)>>=const{std::cell::RefCell::new(None)};}
pub fn start_pin_observer(hwnd: HWND) -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let Some(index) = args.iter().position(|arg| arg == "--pin-sample") else {
        return Ok(());
    };
    let path = args
        .get(index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(E_FAIL, "需要贴图采样目录"))?;
    std::fs::create_dir_all(&path).map_err(io_error)?;
    let mut file = std::fs::File::create(path.join("visible.csv")).map_err(io_error)?;
    writeln!(file,"count,gpu_local,gpu_nonlocal,iteration,unused_capture_ms,private_bytes,total_working_set,handles,gdi,user").map_err(io_error)?;
    PIN_OBSERVER.with_borrow_mut(|slot| *slot = Some((file, 0, false)));
    pin_observer_tick(hwnd);
    // SAFETY: explicit diagnostics only; bounded one-second sampling on the owning UI thread, stopped after closing all pins or 600 samples.
    unsafe {
        if SetTimer(Some(hwnd), 77, 1000, None) == 0 {
            return Err(Error::from_win32());
        }
    }
    Ok(())
}
pub fn pin_observer_tick(hwnd: HWND) {
    let state = PIN_OBSERVER.with_borrow_mut(Option::take);
    let Some((mut file, index, seen)) = state else {
        return;
    };
    let count = super::pinned::count();
    let result = (|| -> Result<()> {
        match super::pinned_device::memory() {
            Some((local, nonlocal)) => write!(file, "{count},{local},{nonlocal},"),
            None => write!(file, "{count},NA,NA,"),
        }
        .map_err(io_error)?;
        row(&mut file, index, 0.)?;
        file.flush().map_err(io_error)
    })();
    if result.is_ok() && index < 600 && !(seen && count == 0) {
        PIN_OBSERVER.with_borrow_mut(|slot| *slot = Some((file, index + 1, seen || count > 0)));
    } else {
        // SAFETY: balances this diagnostic observer's timer; closing the file releases all observer resources.
        unsafe {
            let _ = KillTimer(Some(hwnd), 77);
        }
    }
}
