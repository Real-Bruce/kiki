//! P1 diagnostic surface; not the final capture editor.
use super::capture::CaptureResult;
use std::{cell::Cell, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dxgi::Common::*,
            Gdi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
    },
    core::{Error, Result, w},
};

pub struct Preview(Rc<Cell<Option<HWND>>>);
impl Preview {
    pub fn is_open(&self) -> bool {
        self.0.get().is_some()
    }
    pub fn open(capture: CaptureResult) -> Result<Self> {
        // SAFETY: UI-thread only; Box ownership transfers to the window after resource creation.
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("iCapture.Preview"),
                w!("iCapture · P1 捕获与中文输入验证（非最终编辑器）"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1000,
                720,
                None,
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            let lifetime = Rc::new(Cell::new(Some(hwnd)));
            let result = Surface::new(hwnd, capture, lifetime.clone());
            let surface = match result {
                Ok(surface) => surface,
                Err(error) => {
                    let _ = DestroyWindow(hwnd);
                    return Err(error);
                }
            };
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                Box::into_raw(Box::new(surface)) as isize,
            );
            let _ = ShowWindow(hwnd, SW_SHOW);
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW,
            )?;
            let _ = SetForegroundWindow(hwnd);
            Ok(Self(lifetime))
        }
    }
}
impl Drop for Preview {
    fn drop(&mut self) {
        // SAFETY: Preview is UI-thread-owned; a previously closed window is ignored.
        unsafe {
            if let Some(hwnd) = self.0.get() {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

struct Surface {
    lifetime: Rc<Cell<Option<HWND>>>,
    target: ID2D1HwndRenderTarget,
    bitmap: ID2D1Bitmap,
    text: IDWriteTextFormat,
    brush: ID2D1SolidColorBrush,
    edit: HWND,
    status: Vec<u16>,
}
impl Surface {
    fn new(hwnd: HWND, capture: CaptureResult, lifetime: Rc<Cell<Option<HWND>>>) -> Result<Self> {
        // SAFETY: all COM objects remain on this UI thread; bitmap data copied during CreateBitmap.
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateHwndRenderTarget(
                &D2D1_RENDER_TARGET_PROPERTIES {
                    dpiX: 96.,
                    dpiY: 96.,
                    ..Default::default()
                },
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U {
                        width: 1000,
                        height: 720,
                    },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                },
            )?;
            let (width, height) = capture
                .image
                .bounds
                .size()
                .ok_or_else(|| Error::new(E_FAIL, "无效图像"))?;
            let bitmap = target.CreateBitmap(
                D2D_SIZE_U { width, height },
                Some(capture.image.pixels.as_ptr().cast()),
                capture.image.stride,
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.,
                    dpiY: 96.,
                },
            )?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let text = dw.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                16.,
                w!("zh-CN"),
            )?;
            let brush = target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.12,
                    g: 0.15,
                    b: 0.2,
                    a: 1.,
                },
                None,
            )?;
            let edit = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                w!("中文输入验证：可在这里输入多行文字"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE(
                        ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32,
                    ),
                20,
                590,
                940,
                70,
                Some(hwnd),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?;
            let status = format!(
                "WGC · {} 屏 · {} × {} 物理像素 · 首帧及读回 {:.1} ms · Esc 关闭预览",
                capture.monitors,
                width,
                height,
                capture.elapsed.as_secs_f64() * 1000.
            )
            .encode_utf16()
            .collect();
            Ok(Self {
                lifetime,
                target,
                bitmap,
                text,
                brush,
                edit,
                status,
            })
        }
    }
    fn paint(&self, hwnd: HWND) -> Result<()> {
        // SAFETY: render target and child HWND belong to this UI thread; all structs are initialized.
        unsafe {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect)?;
            let scale = GetDpiForWindow(hwnd) as f32 / 96.;
            self.target.Resize(&D2D_SIZE_U {
                width: rect.right.max(1) as u32,
                height: rect.bottom.max(1) as u32,
            })?;
            self.target.SetDpi(96. * scale, 96. * scale);
            let width = rect.right as f32 / scale;
            let height = rect.bottom as f32 / scale;
            let _ = MoveWindow(
                self.edit,
                (20. * scale) as i32,
                ((height - 90.) * scale) as i32,
                ((width - 40.).max(1.) * scale) as i32,
                (70. * scale) as i32,
                true,
            );
            self.target.BeginDraw();
            self.target.Clear(Some(&D2D1_COLOR_F {
                r: 0.94,
                g: 0.96,
                b: 0.98,
                a: 1.,
            }));
            self.target.DrawText(
                &self.status,
                &self.text,
                &D2D_RECT_F {
                    left: 20.,
                    top: 16.,
                    right: width - 20.,
                    bottom: 64.,
                },
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let size = self.bitmap.GetSize();
            let ratio = ((width - 40.) / size.width)
                .min((height - 180.) / size.height)
                .max(0.01);
            self.target.DrawBitmap(
                &self.bitmap,
                Some(&D2D_RECT_F {
                    left: 20.,
                    top: 70.,
                    right: 20. + size.width * ratio,
                    bottom: 70. + size.height * ratio,
                }),
                1.,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
            self.target.EndDraw(None, None)
        }
    }
}

pub fn register(instance: HINSTANCE) -> Result<()> {
    // SAFETY: class strings are static; callback is valid for the process lifetime.
    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: w!("iCapture.Preview"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(Error::from_win32());
        }
    }
    Ok(())
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match std::panic::catch_unwind(|| dispatch(hwnd, msg, wp, lp)) {
        Ok(result) => result,
        Err(_) => {
            super::show_error("预览操作失败，请关闭后重新截图。");
            LRESULT(0)
        }
    }
}
fn dispatch(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: userdata is the Box installed by open; cleared before it is reclaimed exactly once.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Surface;
        match msg {
            WM_NCDESTROY if !pointer.is_null() => {
                (*pointer).lifetime.set(None);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_PAINT if !pointer.is_null() => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                let result = (*pointer).paint(hwnd);
                let _ = EndPaint(hwnd, &ps);
                if let Err(e) = result {
                    super::show_error(&format!("预览绘制失败：{e}"));
                }
                LRESULT(0)
            }
            WM_SIZE | WM_DPICHANGED => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_KEYDOWN if wp.0 == 27 => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
