//! Explicit developer probe. Never runs during ordinary tray operation.
use super::capture;
use std::{io::Write, path::PathBuf};
use windows::{
    Win32::{
        Foundation::E_FAIL,
        System::{ProcessStatus::*, Threading::*},
    },
    core::{Error, Result},
};

pub fn run_if_requested() -> Result<bool> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_none_or(|arg| arg != "--stress") {
        return Ok(false);
    }
    let path = args
        .get(2)
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(E_FAIL, "--stress 需要输出目录"))?;
    std::fs::create_dir_all(&path).map_err(io_error)?;
    std::thread::Builder::new()
        .name("stress-probe".into())
        .spawn(move || stress(path))
        .map_err(io_error)?
        .join()
        .map_err(|_| Error::new(E_FAIL, "诊断线程意外退出"))??;
    Ok(true)
}

fn stress(path: PathBuf) -> Result<()> {
    let mut csv = std::fs::File::create(path.join("stress.csv")).map_err(io_error)?;
    writeln!(
        csv,
        "iteration,capture_ms,private_bytes,total_working_set,handles,gdi,user"
    )
    .map_err(io_error)?;
    // Include baseline and every result; no working set trimming or sample filtering.
    row(&mut csv, 0, 0.)?;
    for index in 1..=100 {
        let image = capture::desktop()?;
        let elapsed = image.elapsed.as_secs_f64() * 1000.;
        drop(image);
        row(&mut csv, index, elapsed)?;
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    row(&mut csv, 101, 0.)?;
    csv.flush().map_err(io_error)?;
    Ok(())
}

fn row(csv: &mut std::fs::File, index: u32, elapsed: f64) -> Result<()> {
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
