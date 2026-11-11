//! One bounded output worker. No image encoding or file IO runs on the UI thread.
use crate::core::{
    annotation::Editor,
    config::OutputFormat,
    geometry::PixelRect,
    image::{Image, blend_over_white},
};
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
        System::{
            Com::*, DataExchange::*, Memory::*, Ole::CF_UNICODETEXT,
            SystemInformation::GetLocalTime,
        },
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
    File(SaveTarget),
    Pin,
}
#[derive(Clone)]
pub(super) struct SaveTarget {
    path: PathBuf,
    format: OutputFormat,
}

impl SaveTarget {
    pub(super) fn new(path: PathBuf, format: OutputFormat) -> Result<Self> {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension()))
        {
            return Err(fail(&format!(
                "请选择 .{} 扩展名保存 {} 图片",
                format.extension(),
                format.label()
            )));
        }
        Ok(Self { path, format })
    }
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
    corner_radius: u16,
    owner: usize,
    destination: Destination,
) -> Result<Job> {
    launch(
        source,
        Some((rect, editor, corner_radius)),
        owner,
        destination,
    )
}
pub fn start_image(source: Arc<Image>, owner: usize, destination: Destination) -> Result<Job> {
    launch(source, None, owner, destination)
}
pub fn start_color_copy(text: String, owner: usize) -> Result<Job> {
    start_job("color-output", move |cancel| {
        let bytes: Vec<u8> = text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut block = Block::from_bytes(&bytes)?;
        let _clipboard = Clipboard::open(owner, cancel)?;
        check(cancel)?;
        // SAFETY: the worker holds the open clipboard, and Block transfers movable memory on success.
        unsafe {
            EmptyClipboard()?;
        }
        block.publish(u32::from(CF_UNICODETEXT.0))?;
        Ok(Completed::Done)
    })
}
fn launch(
    source: Arc<Image>,
    scene: Option<(PixelRect, Editor, u16)>,
    owner: usize,
    destination: Destination,
) -> Result<Job> {
    start_job("image-output", move |cancel| {
        let image = match scene {
            Some((rect, editor, corner_radius)) => Arc::new(super::composition::render(
                &source,
                rect,
                &editor,
                corner_radius,
            )?),
            None => source,
        };
        check(cancel)?;
        match destination {
            Destination::Clipboard => clipboard(&image, owner, cancel)?,
            Destination::File(target) => save_atomic(&image, &target, cancel)?,
            Destination::Pin => return Ok(Completed::Pinned(image)),
        }
        Ok(Completed::Done)
    })
}

fn start_job(
    name: &str,
    work: impl FnOnce(&AtomicBool) -> Result<Completed> + Send + std::panic::UnwindSafe + 'static,
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
        .name(name.into())
        .spawn(move || {
            let _permit = permit;
            let result = std::panic::catch_unwind(|| {
                check(&worker_cancel)?;
                work(&worker_cancel)
            })
            .unwrap_or_else(|_| Err(fail("输出线程失败，编辑内容已保留")));
            let _ = tx.send(result);
        })
        .map_err(|e| fail(&e.to_string()))?;
    Ok(Job { receiver, cancel })
}
thread_local! {static LAST_DIRECTORY:std::cell::RefCell<Option<PathBuf>>=const {std::cell::RefCell::new(None)};}
#[cfg(test)]
thread_local! {pub(super) static TEST_DIALOG:std::cell::RefCell<Option<Option<SaveTarget>>>=const{std::cell::RefCell::new(None)};}
pub(super) fn save_dialog(owner: HWND) -> Result<Option<SaveTarget>> {
    #[cfg(test)]
    if let Some(path) = TEST_DIALOG.with_borrow_mut(Option::take) {
        return Ok(path);
    }
    let config = super::preferences::current();
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
                | FOS_PATHMUSTEXIST
                | FOS_STRICTFILETYPES,
        )?;
        dialog.SetFileTypes(&[
            Common::COMDLG_FILTERSPEC {
                pszName: w!("PNG 图片 (*.png)"),
                pszSpec: w!("*.png"),
            },
            Common::COMDLG_FILTERSPEC {
                pszName: w!("JPEG 图片 (*.jpeg)"),
                pszSpec: w!("*.jpeg"),
            },
        ])?;
        let (index, extension) = match config.output_format {
            OutputFormat::Png => (1, w!("png")),
            OutputFormat::Jpeg => (2, w!("jpeg")),
        };
        dialog.SetFileTypeIndex(index)?;
        dialog.SetDefaultExtension(extension)?;
        let date = GetLocalTime();
        // The shell appends the extension for the current filter, including after a type change.
        let name = format!(
            "kiki_{:04}{:02}{:02}_{:02}{:02}{:02}",
            date.wYear, date.wMonth, date.wDay, date.wHour, date.wMinute, date.wSecond
        );
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        dialog.SetFileName(PCWSTR(name.as_ptr()))?;
        if let Some(directory) = config
            .last_directory
            .map(PathBuf::from)
            .or_else(|| LAST_DIRECTORY.with_borrow(Clone::clone))
        {
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
        let format = match dialog.GetFileTypeIndex()? {
            1 => OutputFormat::Png,
            2 => OutputFormat::Jpeg,
            _ => return Err(fail("无效的保存格式")),
        };
        let item = dialog.GetResult()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        use std::os::windows::ffi::OsStringExt;
        let result = PathBuf::from(std::ffi::OsString::from_wide(path.as_wide()));
        CoTaskMemFree(Some(path.0.cast()));
        // Validate the shell's confirmed path without changing it after overwrite confirmation.
        let target = SaveTarget::new(result, format)?;
        LAST_DIRECTORY.with_borrow_mut(|last| *last = target.path.parent().map(Path::to_path_buf));
        if let Some(parent) = target.path.parent() {
            super::preferences::remember_directory(parent);
        }
        Ok(Some(target))
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
pub(super) fn save_atomic(image: &Image, target: &SaveTarget, cancel: &AtomicBool) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    check(cancel)?;
    let parent = target.path.parent().ok_or_else(|| fail("保存目录无效"))?;
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
    super::image_encode::save(image, &temporary.0, target.format)?;
    check(cancel)?;
    let from: Vec<u16> = temporary
        .0
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let to: Vec<u16> = target
        .path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
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
impl Clipboard {
    fn open(owner: usize, cancel: &AtomicBool) -> Result<Self> {
        for _ in 0..5 {
            check(cancel)?;
            // SAFETY: the caller supplied its owner HWND. A successful open is balanced by Drop on this worker.
            if unsafe { OpenClipboard(Some(HWND(owner as *mut _))) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err(fail("剪贴板正被其他应用占用，请稍后重试"))
    }
}
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
            let a = p[3];
            for c in p.iter_mut().take(3) {
                *c = blend_over_white(*c, a);
            }
            p[3] = 255;
        }
    }
    Ok(bytes)
}
fn clipboard(image: &Image, owner: usize, cancel: &AtomicBool) -> Result<()> {
    let mut v5 = Block::from_bytes(&dib(image, true)?)?;
    let mut standard = Block::from_bytes(&dib(image, false)?)?;
    let mut png = Block::from_bytes(&super::image_encode::encode_png(image)?)?;
    // SAFETY: the format name is static and the worker holds the clipboard while publishing the data.
    unsafe {
        let format = RegisterClipboardFormatW(w!("PNG"));
        if format == 0 {
            return Err(fail("无法注册剪贴板 PNG 格式"));
        }
        let _clipboard = Clipboard::open(owner, cancel)?;
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
        let target = SaveTarget::new(path.clone(), OutputFormat::Png).unwrap();
        assert!(SaveTarget::new(path.clone(), OutputFormat::Jpeg).is_err());
        let image = Image::new(PixelRect {
            left: 0,
            top: 0,
            right: 2,
            bottom: 2,
        })
        .unwrap();
        assert!(save_atomic(&image, &target, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert!(
            save_atomic(
                &image,
                &SaveTarget::new(dir.join("missing/file.png"), OutputFormat::Png).unwrap(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        let blocked = dir.join("directory-target.png");
        std::fs::create_dir(&blocked).unwrap();
        let blocked_target = SaveTarget::new(blocked.clone(), OutputFormat::Png).unwrap();
        assert!(save_atomic(&image, &blocked_target, &AtomicBool::new(false)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        std::fs::remove_dir(blocked).unwrap();
        save_atomic(&image, &target, &AtomicBool::new(false)).unwrap();
        assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        let jpeg_path = dir.join("编码失败.jpeg");
        std::fs::write(&jpeg_path, b"previous JPEG").unwrap();
        let jpeg = SaveTarget::new(jpeg_path.clone(), OutputFormat::Jpeg).unwrap();
        let too_wide = Image::new(PixelRect {
            left: 0,
            top: 0,
            right: 65_536,
            bottom: 1,
        })
        .unwrap();
        let error = save_atomic(&too_wide, &jpeg, &AtomicBool::new(false)).unwrap_err();
        assert!(error.message().contains("65535"));
        assert_eq!(std::fs::read(&jpeg_path).unwrap(), b"previous JPEG");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_file(jpeg_path).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
