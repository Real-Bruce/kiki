//! One bounded output worker. No image encoding or file IO runs on the UI thread.
use crate::core::{annotation::Editor, geometry::PixelRect, image::Image};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{Com::*, DataExchange::*, Memory::*, SystemInformation::GetLocalTime},
        UI::Shell::*,
    },
    core::{Error, PCWSTR, Result, w},
};
static WORKER: AtomicBool = AtomicBool::new(false);
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        WORKER.store(false, Ordering::Release);
    }
}
pub enum Destination {
    Clipboard,
    File(PathBuf),
    Pin,
}
pub enum Completed {
    Done,
    Pinned(Arc<Image>),
}
pub struct Job {
    pub receiver: mpsc::Receiver<Result<Completed>>,
    cancel: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
fn fail(message: &str) -> Error {
    Error::new(E_FAIL, message)
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(Error::new(E_ABORT, "输出已取消"))
    } else {
        Ok(())
    }
}
pub fn start(
    source: Arc<Image>,
    rect: PixelRect,
    editor: Editor,
    owner: usize,
    destination: Destination,
) -> Result<Job> {
    launch(source, Some((rect, editor)), owner, destination)
}
pub fn start_image(source: Arc<Image>, owner: usize, destination: Destination) -> Result<Job> {
    launch(source, None, owner, destination)
}
fn launch(
    source: Arc<Image>,
    scene: Option<(PixelRect, Editor)>,
    owner: usize,
    destination: Destination,
) -> Result<Job> {
    if WORKER
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(fail("上一项输出仍在结束，请稍后重试"));
    }
    let permit = Permit;
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (tx, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("image-output".into())
        .spawn(move || {
            let _permit = permit;
            let result = std::panic::catch_unwind(|| {
                check(&worker_cancel)?;
                let image = match scene {
                    Some((rect, editor)) => {
                        Arc::new(super::composition::render(&source, rect, &editor)?)
                    }
                    None => source,
                };
                check(&worker_cancel)?;
                match destination {
                    Destination::Clipboard => {
                        clipboard(&image, owner, &worker_cancel)?;
                        Ok(Completed::Done)
                    }
                    Destination::File(path) => {
                        save_atomic(&image, &path, &worker_cancel)?;
                        Ok(Completed::Done)
                    }
                    Destination::Pin => Ok(Completed::Pinned(image)),
                }
            })
            .unwrap_or_else(|_| Err(fail("输出线程失败，编辑内容已保留")));
            let _ = tx.send(result);
        })
        .map_err(|e| fail(&e.to_string()))?;
    Ok(Job { receiver, cancel })
}
thread_local! {static LAST_DIRECTORY:std::cell::RefCell<Option<PathBuf>>=const {std::cell::RefCell::new(None)};}
pub fn save_dialog(owner: HWND) -> Result<Option<PathBuf>> {
    let _apartment = super::composition::Apartment::enter()?;
    // SAFETY: called on the UI STA, with no application-state borrow. Dialog owns strings during calls; returned shell path is freed with CoTaskMemFree.
    unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_FORCEFILESYSTEM
                | FOS_OVERWRITEPROMPT
                | FOS_NOCHANGEDIR
                | FOS_PATHMUSTEXIST,
        )?;
        dialog.SetFileTypes(&[Common::COMDLG_FILTERSPEC {
            pszName: w!("PNG 图片"),
            pszSpec: w!("*.png"),
        }])?;
        dialog.SetDefaultExtension(w!("png"))?;
        let date = GetLocalTime();
        let name = format!(
            "kiki_{:04}{:02}{:02}_{:02}{:02}{:02}.png",
            date.wYear, date.wMonth, date.wDay, date.wHour, date.wMinute, date.wSecond
        );
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        dialog.SetFileName(PCWSTR(name.as_ptr()))?;
        if let Some(directory) = LAST_DIRECTORY.with_borrow(Clone::clone) {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
            if let Ok(folder) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide.as_ptr()), None)
            {
                let _ = dialog.SetFolder(&folder);
            }
        }
        if let Err(e) = dialog.Show(Some(owner)) {
            if e.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(e);
        }
        let item = dialog.GetResult()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        use std::os::windows::ffi::OsStringExt;
        let result = PathBuf::from(std::ffi::OsString::from_wide(path.as_wide()));
        CoTaskMemFree(Some(path.0.cast()));
        LAST_DIRECTORY.with_borrow_mut(|last| *last = result.parent().map(Path::to_path_buf));
        Ok(Some(result))
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
pub fn save_atomic(image: &Image, path: &Path, cancel: &AtomicBool) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    check(cancel)?;
    let parent = path.parent().ok_or_else(|| fail("保存目录无效"))?;
    let mut reserved = None;
    for _ in 0..16 {
        let path = parent.join(format!(
            ".kiki-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                reserved = Some(Temporary(path));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(fail(&format!("无法写入所选目录：{e}"))),
        }
    }
    let temporary = reserved.ok_or_else(|| fail("无法创建临时输出文件"))?;
    super::png::save(image, &temporary.0)?;
    check(cancel)?;
    let from: Vec<u16> = temporary
        .0
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: closed sibling temporary file replaces target only after successful encoding and cancellation check. Both UTF-16 paths live through the call.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}
struct Block(Option<HGLOBAL>);
impl Block {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // SAFETY: this block exclusively owns movable global storage until clipboard transfer. Lock covers exactly the allocation size.
        unsafe {
            let block = Self(Some(GlobalAlloc(GMEM_MOVEABLE, bytes.len())?));
            let handle = block.0.ok_or_else(|| fail("剪贴板内存无效"))?;
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                return Err(fail("无法锁定剪贴板内存"));
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            let _ = GlobalUnlock(handle);
            Ok(block)
        }
    }
    fn publish(&mut self, format: u32) -> Result<()> {
        // SAFETY: clipboard is open; ownership transfers only on successful SetClipboardData.
        unsafe {
            let handle = self.0.ok_or_else(|| fail("剪贴板数据已转移"))?;
            SetClipboardData(format, Some(HANDLE(handle.0)))?;
            self.0 = None;
            Ok(())
        }
    }
}
impl Drop for Block {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // SAFETY: only untransferred storage remains application-owned.
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
        }
    }
}
struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: balances this worker's OpenClipboard.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
fn dib(image: &Image, v5: bool) -> Result<Vec<u8>> {
    let (width, height) = image
        .bounds()
        .size()
        .ok_or_else(|| fail("剪贴板尺寸无效"))?;
    let width = i32::try_from(width).map_err(|_| fail("图片过宽"))?;
    let height = i32::try_from(height).map_err(|_| fail("图片过高"))?;
    let header = if v5 { 124 } else { 40 };
    let length = image
        .pixels()
        .len()
        .checked_add(header)
        .ok_or_else(|| fail("剪贴板图像过大"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| fail("没有足够内存复制图片"))?;
    bytes.resize(length, 0);
    bytes[0..4].copy_from_slice(&(header as u32).to_le_bytes());
    bytes[4..8].copy_from_slice(&width.to_le_bytes());
    bytes[8..12].copy_from_slice(&(-height).to_le_bytes());
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&(image.pixels().len() as u32).to_le_bytes());
    if v5 {
        bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
        for (offset, value) in [
            (40, 0x00ff0000u32),
            (44, 0x0000ff00),
            (48, 0x000000ff),
            (52, 0xff000000),
            (56, 0x73524742),
            (108, 4),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[header..].copy_from_slice(image.pixels());
    if !v5 {
        for p in bytes[header..].chunks_exact_mut(4) {
            let a = u32::from(p[3]);
            for c in p.iter_mut().take(3) {
                *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            p[3] = 255;
        }
    }
    Ok(bytes)
}
fn clipboard(image: &Image, owner: usize, cancel: &AtomicBool) -> Result<()> {
    let mut v5 = Block::from_bytes(&dib(image, true)?)?;
    let mut standard = Block::from_bytes(&dib(image, false)?)?;
    let mut png = Block::from_bytes(&super::png::encode(image)?)?;
    // SAFETY: formats and storage are prepared before touching the clipboard. Open retries are bounded on the worker, never the UI thread.
    unsafe {
        let format = RegisterClipboardFormatW(w!("PNG"));
        if format == 0 {
            return Err(fail("无法注册剪贴板 PNG 格式"));
        }
        let mut opened = false;
        for _ in 0..5 {
            check(cancel)?;
            if OpenClipboard(Some(HWND(owner as *mut _))).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !opened {
            return Err(fail("剪贴板正被其他应用占用，请稍后重试"));
        }
        let _clipboard = Clipboard;
        check(cancel)?;
        EmptyClipboard()?;
        v5.publish(17)?;
        let _ = standard.publish(8);
        let _ = png.publish(format);
        Ok(())
    }
}
#[cfg(feature = "diagnostics")]
pub fn inspect_clipboard(path: &Path) -> Result<()> {
    let (bytes, width, height, legacy) = {
        // SAFETY: read-only inspection of explicitly copied image formats. No transferred handle is freed, and every lock is released before closing the clipboard.
        unsafe {
            OpenClipboard(None)?;
            let _clipboard = Clipboard;
            let format = RegisterClipboardFormatW(w!("PNG"));
            let png = GetClipboardData(format)?;
            let png_handle = HGLOBAL(png.0);
            let length = GlobalSize(png_handle);
            if !(8..=600 * 1024 * 1024).contains(&length) {
                return Err(fail("剪贴板 PNG 长度无效"));
            }
            let pointer = GlobalLock(png_handle);
            if pointer.is_null() {
                return Err(fail("无法读取剪贴板 PNG"));
            }
            let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), length).to_vec();
            let _ = GlobalUnlock(png_handle);
            if &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
                return Err(fail("剪贴板没有 PNG 图片"));
            }
            let dib = GetClipboardData(17)?;
            let handle = HGLOBAL(dib.0);
            if GlobalSize(handle) < 124 {
                return Err(fail("DIBV5 数据无效"));
            }
            let pointer = GlobalLock(handle);
            if pointer.is_null() {
                return Err(fail("无法读取 DIBV5"));
            }
            let header = std::slice::from_raw_parts(pointer.cast::<u8>(), 124);
            let width = i32::from_le_bytes([header[4], header[5], header[6], header[7]]);
            let height = i32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let _ = GlobalUnlock(handle);
            (bytes, width, height, IsClipboardFormatAvailable(8).is_ok())
        }
    };
    std::fs::create_dir_all(path).map_err(|e| fail(&e.to_string()))?;
    std::fs::write(path.join("clipboard.png"), bytes).map_err(|e| fail(&e.to_string()))?;
    std::fs::write(path.join("clipboard.txt"),format!("DIBV5 width={width}, height={height} (negative=top-down)\nCF_DIB available={legacy}\nPNG present=true\n")).map_err(|e|fail(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dib_formats_preserve_top_down_alpha_and_flatten_only_legacy_fallback() {
        let mut image = Image::new(PixelRect {
            left: -1,
            top: 0,
            right: 1,
            bottom: 1,
        })
        .unwrap();
        image
            .pixels_mut()
            .copy_from_slice(&[10, 20, 30, 255, 0, 0, 0, 0]);
        let v5 = dib(&image, true).unwrap();
        assert_eq!(&v5[8..12], &(-1i32).to_le_bytes());
        assert_eq!(&v5[124..], image.pixels());
        assert_eq!(&dib(&image, false).unwrap()[44..48], &[255; 4]);
    }
    #[test]
    fn atomic_save_cancel_and_failure_keep_existing_file() {
        let dir = std::env::temp_dir().join(format!("kiki-output-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("existing.png");
        std::fs::write(&path, b"original").unwrap();
        let image = Image::new(PixelRect {
            left: 0,
            top: 0,
            right: 2,
            bottom: 2,
        })
        .unwrap();
        assert!(save_atomic(&image, &path, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert!(
            save_atomic(
                &image,
                &dir.join("missing/file.png"),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        let blocked = dir.join("directory-target");
        std::fs::create_dir(&blocked).unwrap();
        assert!(save_atomic(&image, &blocked, &AtomicBool::new(false)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        std::fs::remove_dir(blocked).unwrap();
        save_atomic(&image, &path, &AtomicBool::new(false)).unwrap();
        assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
