//! Bounded asynchronous preference writes with atomic replacement and corruption preservation.
use crate::core::config::{CONFIG_VERSION, Config};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::{
    Win32::{
        Foundation::HWND,
        Storage::FileSystem::*,
        System::Com::CoTaskMemFree,
        UI::{
            Shell::*,
            WindowsAndMessaging::{PostMessageW, WM_APP},
        },
    },
    core::PCWSTR,
};
pub const SAVED: u32 = WM_APP + 60;
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub struct Loaded {
    pub config: Config,
    pub migrate: bool,
    pub warning: Option<String>,
    pub disk: Disk,
}
pub struct Disk {
    pub path: PathBuf,
    protect: bool,
    recover: bool,
}
pub fn default_path() -> Result<PathBuf, String> {
    #[cfg(feature = "diagnostics")]
    {
        let args: Vec<_> = std::env::args_os().collect();
        if let Some(index) = args.iter().position(|arg| arg == "--config-dir") {
            return args
                .get(index + 1)
                .map(|dir| PathBuf::from(dir).join("settings.ini"))
                .ok_or_else(|| "需要配置测试目录".into());
        }
    }
    // SAFETY: known-folder allocation is copied to an owned path before releasing shell memory.
    unsafe {
        let folder = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None)
            .map_err(|e| e.to_string())?;
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_wide(folder.as_wide()))
            .join("kiki")
            .join("settings.ini");
        CoTaskMemFree(Some(folder.0.cast()));
        Ok(path)
    }
}
impl Disk {
    pub fn load(path: PathBuf) -> Loaded {
        let mut disk = Self {
            path,
            protect: false,
            recover: false,
        };
        let mut data = Vec::new();
        match File::open(&disk.path) {
            Ok(file) => {
                if let Err(error) = file.take(65537).read_to_end(&mut data) {
                    disk.protect = true;
                    return Loaded {
                        config: Config::default(),
                        migrate: false,
                        warning: Some(format!("配置无法读取，使用默认值并暂停覆盖：{error}")),
                        disk,
                    };
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Loaded {
                    config: Config::default(),
                    migrate: false,
                    warning: None,
                    disk,
                };
            }
            Err(error) => {
                disk.protect = true;
                return Loaded {
                    config: Config::default(),
                    migrate: false,
                    warning: Some(format!("配置无法读取，使用默认值并暂停覆盖：{error}")),
                    disk,
                };
            }
        }
        let text = std::str::from_utf8(&data).ok();
        if let Some((config, migrate)) = text.and_then(|text| Config::decode(text).ok()) {
            return Loaded {
                config,
                migrate,
                warning: None,
                disk,
            };
        }
        let future = text
            .and_then(|text| text.lines().find_map(|line| line.strip_prefix("version=")))
            .and_then(|version| version.parse::<u32>().ok())
            .is_some_and(|version| version > CONFIG_VERSION);
        disk.protect = future;
        disk.recover = !future;
        Loaded {
            config: Config::default(),
            migrate: false,
            warning: Some(if future {
                "配置来自更高版本，使用默认值运行并保留原文件，不覆盖保存。".into()
            } else {
                "配置损坏，已使用默认值；下次保存会先保留损坏文件备份。".into()
            }),
            disk,
        }
    }
    fn save(&mut self, config: &Config) -> Result<(), String> {
        if self.protect {
            return Err("当前配置受保护，请关闭程序后检查配置文件权限或版本".into());
        }
        let content = config.encode()?;
        if content.len() > 65536 {
            return Err("配置超过 64 KiB，未写入".into());
        }
        let parent = self.path.parent().ok_or("配置目录无效")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        if self.recover && self.path.exists() {
            let mut preserved = false;
            for index in 0..3 {
                let backup = parent.join(format!("settings.corrupt-{index}.ini"));
                if backup.exists() {
                    continue;
                }
                fs::rename(&self.path, &backup)
                    .map_err(|e| format!("无法保留损坏配置，未覆盖原文件：{e}"))?;
                preserved = true;
                break;
            }
            if !preserved {
                return Err("损坏配置备份已满，请检查配置目录；原文件没有被覆盖".into());
            }
        }
        self.recover = false;
        let mut temporary = None;
        for _ in 0..16 {
            let path = parent.join(format!(
                ".settings-{}-{}.tmp",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    temporary = Some((Temp(path), file));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        let (temp, mut file) = temporary.ok_or("无法创建配置临时文件")?;
        file.write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<_> = temp.0.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = self.path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: both sibling paths remain live and the fully flushed temporary file is closed before atomic replacement.
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
#[derive(Clone, Debug)]
pub struct Outcome {
    pub revision: u64,
    pub result: Result<(), String>,
}
#[derive(Default)]
struct Queue {
    pending: Option<(u64, Config)>,
    outcome: Option<Outcome>,
    closing: bool,
}
pub struct Writer {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}
impl Writer {
    pub fn new(mut disk: Disk, owner: Option<HWND>) -> Result<Self, String> {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let worker_queue = queue.clone();
        let owner = owner.map(|hwnd| hwnd.0 as usize);
        let thread = std::thread::Builder::new()
            .name("preferences-writer".into())
            .spawn(move || {
                loop {
                    let (lock, wake) = &*worker_queue;
                    let mut state = lock
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    while state.pending.is_none() && !state.closing {
                        state = wake
                            .wait(state)
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                    if state.pending.is_none() && state.closing {
                        break;
                    }
                    if !state.closing {
                        let revision = state.pending.as_ref().map(|(revision, _)| *revision);
                        let (next, _) = wake
                            .wait_timeout(state, Duration::from_millis(180))
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        state = next;
                        if !state.closing
                            && state.pending.as_ref().map(|(revision, _)| *revision) != revision
                        {
                            continue;
                        }
                    }
                    let Some((revision, config)) = state.pending.take() else {
                        continue;
                    };
                    drop(state);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        disk.save(&config)
                    }))
                    .unwrap_or_else(|_| Err("配置保存线程发生异常".into()));
                    lock.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .outcome = Some(Outcome { revision, result });
                    if let Some(owner) = owner {
                        // SAFETY: only a value notification crosses threads. Owner joins this worker before destroying the host HWND.
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(owner as *mut _)),
                                SAVED,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            queue,
            thread: Some(thread),
        })
    }
    pub fn request(&self, revision: u64, config: Config) {
        let (lock, wake) = &*self.queue;
        lock.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending = Some((revision, config));
        wake.notify_one();
    }
    pub fn outcome(&self) -> Option<Outcome> {
        self.queue
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .outcome
            .take()
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.queue
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closing = true;
        self.queue.1.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_location_uses_local_appdata_not_executable_directory() {
        let _com = super::super::composition::Apartment::enter().unwrap();
        let path = default_path().unwrap();
        assert!(path.is_absolute());
        assert!(path.ends_with(std::path::Path::new("kiki").join("settings.ini")));
    }
    fn folder() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kiki-settings-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn clean(path: &std::path::Path) {
        for entry in fs::read_dir(path).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        fs::remove_dir(path).unwrap();
    }
    #[test]
    fn atomic_preferences_reload_corrupt_recovery_and_future_protection() {
        let dir = folder();
        let path = dir.join("settings.ini");
        let loaded = Disk::load(path.clone());
        let mut disk = loaded.disk;
        let config = Config {
            last_directory: Some("F:\\截图\\项目".into()),
            ..Config::default()
        };
        disk.save(&config).unwrap();
        assert_eq!(Disk::load(path.clone()).config, config);
        fs::write(&path, b"bad bytes\xff").unwrap();
        let mut recovered = Disk::load(path.clone());
        assert!(recovered.warning.is_some());
        recovered.disk.save(&Config::default()).unwrap();
        assert_eq!(
            fs::read(dir.join("settings.corrupt-0.ini")).unwrap(),
            b"bad bytes\xff"
        );
        assert_eq!(Disk::load(path.clone()).config, Config::default());
        fs::write(&path, "version=3\ntheme=future\n").unwrap();
        let mut future = Disk::load(path.clone());
        assert!(future.disk.save(&config).is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "version=3\ntheme=future\n"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        clean(&dir);
    }
    #[test]
    fn failed_replace_keeps_previous_bytes_and_writer_flushes_latest_revision() {
        let dir = folder();
        let path = dir.join("settings.ini");
        fs::write(&path, Config::default().encode().unwrap()).unwrap();
        let original = fs::read(&path).unwrap();
        use std::os::windows::fs::OpenOptionsExt;
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let mut disk = Disk {
            path: path.clone(),
            protect: false,
            recover: false,
        };
        assert!(disk.save(&Config::default()).is_err());
        drop(locked);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let writer = Writer::new(disk, None).unwrap();
        for revision in 1..=100 {
            writer.request(
                revision,
                Config {
                    text_size: 12 + (revision % 60) as u16,
                    ..Config::default()
                },
            );
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let outcome = loop {
            if let Some(outcome) = writer.outcome() {
                if outcome.revision == 100 {
                    break outcome;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "latest save did not complete"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(outcome.result.is_ok());
        writer.request(
            101,
            Config {
                text_size: 53,
                ..Config::default()
            },
        );
        drop(writer);
        assert_eq!(Disk::load(path).config.text_size, 53);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        clean(&dir);
    }
}
