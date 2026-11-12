//! Opt-in Release lifecycle sampler; absent from normal builds. Never trims working sets.
use super::{pinned, preferences, preview};
use std::{
    cell::RefCell,
    fs::File,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::DwmFlush, Dxgi::*},
        System::{Diagnostics::ToolHelp::*, ProcessStatus::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
    core::{Error, Interface, Result},
};
thread_local! {static PROBE: RefCell<Option<Probe>> = const {RefCell::new(None)};}
enum Stage {
    Begin,
    Waiting(Instant),
    Ready(HWND),
    Output(Instant),
    Settling(Instant),
    Idle {
        started: Instant,
        cpu: u64,
        ticks: u32,
    },
}
struct Probe {
    folder: PathBuf,
    latency: File,
    resources: File,
    stage: Stage,
    iteration: u32,
    first_only: bool,
    error: Option<String>,
}
fn fail(message: impl AsRef<str>) -> Error {
    Error::new(E_FAIL, message.as_ref())
}
fn io(e: std::io::Error) -> Error {
    fail(e.to_string())
}
pub fn start(owner: HWND) -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let Some(index) = args.iter().position(|a| a == "--performance-probe") else {
        return Ok(());
    };
    let folder = args
        .get(index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| fail("性能采样需要目录"))?;
    if !preferences::active() {
        return Err(fail("性能采样要求热键注册成功"));
    }
    std::fs::create_dir_all(&folder).map_err(io)?;
    let mut latency = File::create(folder.join("latency.csv")).map_err(io)?;
    writeln!(latency, "iteration,kind,action,ready_ms").map_err(io)?;
    let mut resources = File::create(folder.join("resources.csv")).map_err(io)?;
    writeln!(resources,"iteration,phase,private_working_set,private_bytes,total_working_set,handles,gdi,user,threads,gpu_local,gpu_nonlocal").map_err(io)?;
    sample(&mut resources, 0, "fresh_idle")?;
    PROBE.with_borrow_mut(|p| {
        *p = Some(Probe {
            folder,
            latency,
            resources,
            stage: Stage::Begin,
            iteration: 1,
            first_only: args.iter().any(|a| a == "--first-only"),
            error: None,
        })
    });
    // SAFETY: diagnostics-only timer on this UI thread's hidden host. Removed at completion/error.
    unsafe {
        if SetTimer(Some(owner), 78, 30, None) == 0 {
            return Err(Error::from_win32());
        }
    }
    Ok(())
}
/// Timed endpoint: ordinary visible Preview, first native paint and DWM completion.
pub fn ready(hwnd: HWND) {
    let waiting = PROBE.with_borrow(|p| {
        p.as_ref()
            .is_some_and(|p| matches!(p.stage, Stage::Waiting(_)))
    });
    if !waiting {
        return;
    }
    // SAFETY: UI-thread live preview; only the diagnostic run synchronizes with the compositor.
    let result = unsafe {
        let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
        DwmFlush()
    };
    let Some(mut probe) = PROBE.with_borrow_mut(Option::take) else {
        return;
    };
    let result = result.and_then(|_| {
        if let Stage::Waiting(start) = probe.stage {
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            let kind = if probe.iteration == 1 {
                "first"
            } else {
                "subsequent"
            };
            let action = if probe.iteration <= 100 {
                "cancel"
            } else {
                "pin"
            };
            writeln!(
                probe.latency,
                "{},{kind},{action},{elapsed:.3}",
                probe.iteration
            )
            .map_err(io)?;
            probe.latency.flush().map_err(io)?;
            if probe.iteration == 1 {
                std::fs::write(
                    probe.folder.join("display.txt"),
                    preview::performance_display(hwnd)?,
                )
                .map_err(io)?;
            }
            sample(&mut probe.resources, probe.iteration, "editing")?;
            probe.stage = Stage::Ready(hwnd);
        }
        Ok(())
    });
    if let Err(e) = result {
        probe.error = Some(e.to_string());
    }
    PROBE.with_borrow_mut(|p| *p = Some(probe));
}
pub fn tick(owner: HWND) {
    let Some(mut probe) = PROBE.with_borrow_mut(Option::take) else {
        return;
    };
    let result = step(&mut probe, owner);
    match result {
        Ok(false) => PROBE.with_borrow_mut(|p| *p = Some(probe)),
        Ok(true) => {
            if let Err(e) = std::fs::write(
                probe.folder.join("complete.txt"),
                "All requested samples completed.\n",
            ) {
                abort(&probe, &io(e));
            }
            // SAFETY: owned diagnostic host; normal guard teardown joins workers and flushes config.
            unsafe {
                let _ = KillTimer(Some(owner), 78);
                let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        Err(e) => {
            abort(&probe, &e);
            unsafe {
                // SAFETY: stop only this explicitly requested diagnostic process's timer/message loop.
                let _ = KillTimer(Some(owner), 78);
                let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}
fn step(p: &mut Probe, owner: HWND) -> Result<bool> {
    if let Some(error) = &p.error {
        return Err(fail(error));
    }
    match p.stage {
        Stage::Begin => {
            let id = (1..=2)
                .find(|id| preferences::matches(*id))
                .ok_or_else(|| fail("已注册热键 ID 丢失"))?;
            p.stage = Stage::Waiting(Instant::now());
            // SAFETY: post the registered-hotkey dispatch path, including message-queue delay. Not a physical key delivery measurement.
            unsafe {
                PostMessageW(Some(owner), WM_HOTKEY, WPARAM(id), LPARAM(0))?;
            }
        }
        Stage::Waiting(start) if start.elapsed() > Duration::from_secs(8) => {
            return Err(fail("截图首帧超时"));
        }
        Stage::Ready(hwnd) => {
            preview::performance_finish(hwnd, p.iteration > 100)?;
            p.stage = if p.iteration > 100 {
                Stage::Output(Instant::now())
            } else {
                Stage::Settling(Instant::now())
            };
        }
        Stage::Output(start) => {
            if pinned::count() == 1 {
                sample(&mut p.resources, p.iteration, "pinned")?;
                pinned::close_all();
                p.stage = Stage::Settling(Instant::now());
            } else if start.elapsed() > Duration::from_secs(8) {
                return Err(fail("贴图输出超时"));
            }
        }
        Stage::Settling(start) if start.elapsed() >= Duration::from_millis(300) => {
            if pinned::count() != 0 {
                return Err(fail("贴图未释放"));
            }
            // The ordinary App retains only a closed Preview lifetime cell, never its pixels/COM objects.
            sample(&mut p.resources, p.iteration, "closed")?;
            if p.iteration % 10 == 0 {
                window_inventory(&p.folder)?;
            }
            if p.first_only {
                return Ok(true);
            }
            if p.iteration < 200 {
                p.iteration += 1;
                p.stage = Stage::Begin;
            } else {
                p.resources.flush().map_err(io)?;
                window_inventory(&p.folder)?;
                p.stage = Stage::Idle {
                    started: Instant::now(),
                    cpu: cpu_ticks()?,
                    ticks: 0,
                };
                // SAFETY: replace the probe timer with one-second CPU timing; production has no idle timer.
                unsafe {
                    if SetTimer(Some(owner), 78, 1000, None) == 0 {
                        return Err(Error::from_win32());
                    }
                }
            }
        }
        Stage::Idle {
            started,
            cpu,
            ticks,
        } => {
            let elapsed = started.elapsed().as_secs_f64();
            if elapsed >= 60. {
                let end_cpu = cpu_ticks()?;
                sample(&mut p.resources, 201 + ticks, "idle")?;
                let cores = unsafe {
                    // SAFETY: read-only CPU topology query, all processor groups.
                    GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)
                };
                if cores == 0 {
                    return Err(fail("处理器计数失败"));
                }
                let percent =
                    (end_cpu - cpu) as f64 / 10_000_000. / elapsed / f64::from(cores) * 100.;
                std::fs::write(p.folder.join("idle.csv"),format!("seconds,cpu_seconds,logical_processors,normalized_cpu_percent\n{elapsed:.6},{:.6},{cores},{percent:.6}\n",(end_cpu-cpu) as f64/10_000_000.)).map_err(io)?;
                return Ok(true);
            }
            p.stage = Stage::Idle {
                started,
                cpu,
                ticks: ticks + 1,
            };
        }
        _ => {}
    }
    Ok(false)
}
fn abort(p: &Probe, e: &Error) {
    let _ = std::fs::write(p.folder.join("error.txt"), e.to_string());
}
fn cpu_ticks() -> Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: initialized output buffers and borrowed current-process pseudo handle.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )?;
    }
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    Ok(ticks(kernel) + ticks(user))
}
fn thread_count() -> Result<u32> {
    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                // SAFETY: exclusively owned ToolHelp snapshot.
                let _ = CloseHandle(self.0);
            }
        }
    }
    // SAFETY: read-only system snapshot and initialized THREADENTRY32 output structure.
    unsafe {
        let snap = Snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?);
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        Thread32First(snap.0, &mut entry)?;
        let mut count = 0;
        loop {
            if entry.th32OwnerProcessID == GetCurrentProcessId() {
                count += 1;
            }
            if let Err(e) = Thread32Next(snap.0, &mut entry) {
                if e.code() != windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
                    return Err(e);
                }
                break;
            }
        }
        Ok(count)
    }
}
fn sample(file: &mut File, iteration: u32, phase: &str) -> Result<()> {
    // SAFETY: initialized current-process counters and read-only DXGI adapter budget query. No memory trimming or priority changes.
    unsafe {
        let process = GetCurrentProcess();
        let mut m = PROCESS_MEMORY_COUNTERS_EX2 {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            ..Default::default()
        };
        GetProcessMemoryInfo(
            process,
            (&mut m as *mut PROCESS_MEMORY_COUNTERS_EX2).cast(),
            m.cb,
        )?;
        let mut handles = 0;
        GetProcessHandleCount(process, &mut handles)?;
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let adapter: IDXGIAdapter3 = factory.EnumAdapters1(0)?.cast()?;
        let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        let mut nonlocal = local;
        adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local)?;
        adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut nonlocal)?;
        writeln!(
            file,
            "{iteration},{phase},{},{},{},{handles},{},{},{},{},{}",
            m.PrivateWorkingSetSize,
            m.PrivateUsage,
            m.WorkingSetSize,
            GetGuiResources(process, GR_GDIOBJECTS),
            GetGuiResources(process, GR_USEROBJECTS),
            thread_count()?,
            local.CurrentUsage,
            nonlocal.CurrentUsage
        )
        .map_err(io)?;
        file.flush().map_err(io)
    }
}

/// Isolate sampler, HWND rendering and fresh capture threads when investigating growth.
pub fn isolate(folder: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(folder).map_err(io)?;
    // SAFETY: this isolated diagnostic process owns all Preview windows on this thread.
    unsafe {
        super::preview::register(
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?.into(),
        )?;
    }
    for phase in ["sampler", "preview", "backend_fresh", "composition_fresh"] {
        let mut file = File::create(folder.join(format!("{phase}.csv"))).map_err(io)?;
        writeln!(file,"iteration,phase,private_working_set,private_bytes,total_working_set,handles,gdi,user,threads,gpu_local,gpu_nonlocal").map_err(io)?;
        sample(&mut file, 0, phase)?;
        for index in 1..=100 {
            match phase {
                "preview" => {
                    let (image, _) = super::composition::fixture()?;
                    let preview = super::preview::Preview::open_mode(
                        super::capture::CaptureResult {
                            image,
                            elapsed: Duration::ZERO,
                            screens: Vec::new(),
                            windows: Vec::new(),
                        },
                        false,
                    )?;
                    if let Some(hwnd) = preview.hwnd() {
                        unsafe {
                            // SAFETY: synchronously paint our own visible test preview, then release it before sampling.
                            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
                        }
                    }
                    drop(preview);
                }
                "backend_fresh" => {
                    std::thread::spawn(super::capture::desktop)
                        .join()
                        .map_err(|_| fail("捕获线程退出"))??;
                }
                "composition_fresh" => {
                    std::thread::spawn(|| -> Result<()> {
                        let (image, editor) = super::composition::fixture()?;
                        super::composition::render(&image, image.bounds(), &editor, 0)?;
                        Ok(())
                    })
                    .join()
                    .map_err(|_| fail("成图线程退出"))??;
                }
                _ => {}
            }
            sample(&mut file, index, phase)?;
        }
    }
    Ok(())
}

fn window_inventory(folder: &std::path::Path) -> Result<()> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> windows::core::BOOL {
        // SAFETY: synchronous enumeration, exclusively borrowed vector; allocation failure cancels enumeration.
        unsafe {
            let handles = &mut *(data.0 as *mut Vec<HWND>);
            if handles.try_reserve(1).is_err() {
                return windows::core::BOOL(0);
            }
            handles.push(hwnd);
        }
        windows::core::BOOL(1)
    }
    let mut handles = Vec::<HWND>::new();
    // SAFETY: read-only inventory of this diagnostic UI thread, without window titles or user text.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?;
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        if Thread32First(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32OwnerProcessID == GetCurrentProcessId() {
                    let _ = EnumThreadWindows(
                        entry.th32ThreadID,
                        Some(collect),
                        LPARAM((&mut handles as *mut Vec<HWND>) as isize),
                    );
                }
                if Thread32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        let mut previous = None;
        while let Ok(hwnd) = FindWindowExW(Some(HWND_MESSAGE), previous, None, None) {
            previous = Some(hwnd);
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                handles.push(hwnd);
            }
        }
        let mut text = String::new();
        for hwnd in handles {
            let mut class = [0u16; 256];
            let length = GetClassNameW(hwnd, &mut class).max(0) as usize;
            text.push_str(&format!(
                "class={} visible={}\n",
                String::from_utf16_lossy(&class[..length]),
                IsWindowVisible(hwnd).as_bool()
            ));
        }
        std::fs::write(folder.join("windows.txt"), text).map_err(io)
    }
}

/// Deterministic long-image performance probe, independent of desktop input and clipboard.
pub fn long_images(folder: &std::path::Path) -> Result<()> {
    use crate::core::{
        annotation::{Editor, Tool},
        long_image::{Step, Stitcher, fixture, thumbnail},
        selection::PixelPoint,
    };
    std::fs::create_dir_all(folder).map_err(io)?;
    let mut timings = File::create(folder.join("long-latency.csv")).map_err(io)?;
    let mut resources = File::create(folder.join("long-resources.csv")).map_err(io)?;
    writeln!(
        timings,
        "iteration,phase,elapsed_ms,width,height,encoded_bytes"
    )
    .map_err(io)?;
    writeln!(resources, "iteration,phase,private_working_set,private_bytes,total_working_set,handles,gdi,user,threads,gpu_local,gpu_nonlocal").map_err(io)?;
    let mut timing = |i, phase, started: Instant, w, h, bytes| -> Result<()> {
        writeln!(
            timings,
            "{i},{phase},{:.3},{w},{h},{bytes}",
            started.elapsed().as_secs_f64() * 1000.
        )
        .map_err(io)?;
        timings.flush().map_err(io)
    };
    let fixture_error = |message: String| fail(message);
    sample(&mut resources, 0, "baseline")?;
    for iteration in 1..=100 {
        let output = iteration <= 30;
        let width = if output { 640 } else { 160 };
        let frame_height = if output { 720 } else { 256 };
        let stride = if output { 360 } else { 128 };
        let mut stitch = Stitcher::new(fixture(width, 0, frame_height)).map_err(fixture_error)?;
        let frames = if output { 18 } else { 4 };
        for step in 1..=frames {
            let frame = fixture(width, step * stride, frame_height);
            let started = Instant::now();
            if stitch.accept(&frame, true).map_err(fixture_error)? != Step::Added {
                return Err(fail("性能夹具未按预期拼接"));
            }
            let preview = thumbnail(stitch.image()).map_err(fixture_error)?;
            std::hint::black_box(&preview);
            timing(
                iteration,
                "stitch_to_thumbnail",
                started,
                width,
                stitch.image().bounds().bottom,
                0,
            )?;
        }
        sample(&mut resources, iteration, "stitched")?;
        if output {
            let source = stitch.into_image();
            let bounds = source.bounds();
            let mut editor = Editor::default();
            editor.tool = Tool::Rectangle;
            for y in [32, bounds.bottom / 2, bounds.bottom - 90] {
                editor
                    .begin_at(PixelPoint { x: 20, y }, None)
                    .map_err(fail)?;
                editor.finish(PixelPoint { x: 180, y: y + 56 });
            }
            let started = Instant::now();
            let image = super::composition::render(&source, bounds, &editor, 24)?;
            let encoded = super::image_encode::encode_png(&image)?;
            timing(
                iteration,
                "compose_and_png",
                started,
                width,
                bounds.bottom,
                encoded.len(),
            )?;
            sample(&mut resources, iteration, "output_buffers")?;
        } else {
            if iteration % 2 == 0 {
                let disjoint = fixture(width, 10_000, frame_height);
                if stitch.accept(&disjoint, true).map_err(fixture_error)? != Step::Unmatched {
                    return Err(fail("性能夹具未按预期暂停"));
                }
            }
            drop(stitch);
        }
        sample(&mut resources, iteration, "released")?;
    }
    for (iteration, width, frame_height, shift, steps) in
        [(101, 8, 1024, 512, 62), (102, 4096, 1024, 512, 30)]
    {
        let mut stitch = Stitcher::new(fixture(width, 0, frame_height)).map_err(fixture_error)?;
        let started = Instant::now();
        for n in 1..=steps {
            if stitch
                .accept(&fixture(width, n * shift, frame_height), true)
                .map_err(fixture_error)?
                != Step::Added
            {
                return Err(fail("边界性能夹具提前停止"));
            }
        }
        let next = fixture(width, steps * shift + 1, frame_height);
        if stitch.accept(&next, true).map_err(fixture_error)? != Step::Limit {
            return Err(fail("边界性能夹具未触发上限"));
        }
        timing(
            iteration,
            "limit",
            started,
            width,
            stitch.image().bounds().bottom,
            stitch.image().pixels().len(),
        )?;
        sample(&mut resources, iteration, "limit_reached")?;
        drop(stitch);
        sample(&mut resources, iteration, "released")?;
    }
    let wall = Instant::now();
    let before = cpu_ticks()?;
    for second in 1..=60 {
        std::thread::sleep(Duration::from_secs(1));
        sample(&mut resources, second, "idle")?;
    }
    let cpu = cpu_ticks()?.saturating_sub(before);
    std::fs::write(
        folder.join("long-idle.txt"),
        format!(
            "wall_s={:.3}\ncpu_s={:.6}\nlogical_processors={}\n",
            wall.elapsed().as_secs_f64(),
            cpu as f64 / 10_000_000.,
            std::thread::available_parallelism().map_or(1, |n| n.get())
        ),
    )
    .map_err(io)?;
    std::fs::write(folder.join("long-complete.txt"), "ok\n").map_err(io)
}
