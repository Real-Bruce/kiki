//! Stable window regions let DWM dim the live desktop while the source scrolls.
use super::{PixelRect, fail};
use crate::platform::windows::ui::palette;
use windows::{
    Win32::{
        Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::{Error, Result, w},
};

struct Region(HRGN);

impl Region {
    fn rectangle(rect: RECT) -> Result<Self> {
        // SAFETY: CreateRectRgn copies the coordinates and returns a caller-owned region.
        let region = unsafe { CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom) };
        if region.is_invalid() {
            return Err(fail("无法建立长截图遮罩"));
        }
        Ok(Self(region))
    }

    fn subtract(&self, rect: RECT) -> Result<()> {
        let hole = Self::rectangle(rect)?;
        // SAFETY: both regions are live; GDI permits using the destination as source one.
        if unsafe { CombineRgn(Some(self.0), Some(self.0), Some(hole.0), RGN_DIFF) } == RGN_ERROR {
            return Err(fail("无法裁剪长截图遮罩"));
        }
        Ok(())
    }

    fn install(self, hwnd: HWND) -> Result<()> {
        // SAFETY: Windows owns the region only after a successful call.
        if unsafe { SetWindowRgn(hwnd, Some(self.0), true) } == 0 {
            return Err(fail("无法裁剪长截图窗口"));
        }
        std::mem::forget(self);
        Ok(())
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: this guard retains ownership until install succeeds.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

fn local(rect: PixelRect, origin: POINT) -> RECT {
    RECT {
        left: rect.left - origin.x,
        top: rect.top - origin.y,
        right: rect.right - origin.x,
        bottom: rect.bottom - origin.y,
    }
}

pub(super) fn clip_preview(hwnd: HWND, panel: Option<PixelRect>) -> Result<()> {
    let mut window = RECT::default();
    // SAFETY: the preview is a live borderless top-level HWND on this UI thread.
    unsafe {
        GetWindowRect(hwnd, &mut window)?;
    }
    let origin = POINT {
        x: window.left,
        y: window.top,
    };
    Region::rectangle(panel.map_or(RECT::default(), |panel| local(panel, origin)))?.install(hwnd)
}

fn surround(screen: PixelRect, output: PixelRect, panel: PixelRect) -> Result<Region> {
    let origin = POINT {
        x: screen.left,
        y: screen.top,
    };
    let region = Region::rectangle(local(screen, origin))?;
    region.subtract(local(output, origin))?;
    region.subtract(local(panel, origin))?;
    Ok(region)
}

struct Paint {
    border: RECT,
    accent: COLORREF,
}

pub(super) struct Masks {
    _surround: Mask,
    reference_bands: Option<Mask>,
}

impl Masks {
    pub(super) fn show(
        owner: HWND,
        screen: PixelRect,
        reference: PixelRect,
        output: PixelRect,
        panel: PixelRect,
        dark: bool,
    ) -> Result<Self> {
        let surround = Mask::show(owner, screen, reference, panel, output, dark, false)?;
        // Only the strips used for matching overlap captured pixels. Screen
        // sharing can still see the main surround and a disjoint preview panel.
        let reference_bands = if reference != output {
            Some(Mask::show(
                owner, reference, output, panel, output, dark, true,
            )?)
        } else {
            None
        };
        Ok(Self {
            _surround: surround,
            reference_bands,
        })
    }

    pub(super) fn excluded_window(&self) -> Option<usize> {
        self.reference_bands
            .as_ref()
            .map(|mask| mask.hwnd.0 as usize)
    }
}

struct Mask {
    hwnd: HWND,
    // The paint procedure borrows this allocation only while the HWND is live.
    paint: Box<Paint>,
}

impl Mask {
    fn show(
        owner: HWND,
        bounds: PixelRect,
        opening: PixelRect,
        panel: PixelRect,
        output: PixelRect,
        dark: bool,
        exclude: bool,
    ) -> Result<Self> {
        let origin = POINT {
            x: bounds.left,
            y: bounds.top,
        };
        let mut border = local(output, origin);
        border.left -= 2;
        border.top -= 2;
        border.right += 2;
        border.bottom += 2;
        let rgb = palette::accent(dark);
        let paint = Box::new(Paint {
            border,
            accent: COLORREF((rgb >> 16) | (rgb & 0xff00) | ((rgb & 0xff) << 16)),
        });
        let region = surround(bounds, opening, panel)?;
        // SAFETY: the UI thread owns this popup. It stays hidden until its region,
        // paint data and optional capture exclusion have all been installed.
        unsafe {
            let instance = GetModuleHandleW(None)?.into();
            let class = WNDCLASSW {
                hInstance: instance,
                lpszClassName: w!("kiki.LongCaptureMask"),
                lpfnWndProc: Some(paint_proc),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(Error::from_win32());
            }
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.lpszClassName,
                w!(""),
                WS_POPUP,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                Some(owner),
                None,
                Some(instance),
                None,
            )?;
            let mask = Self { hwnd, paint };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&raw const *mask.paint) as isize);
            region.install(hwnd)?;
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 128, LWA_ALPHA)?;
            if exclude {
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)?;
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
            Ok(mask)
        }
    }
}

impl Drop for Mask {
    fn drop(&mut self) {
        // SAFETY: the mask and its owner are confined to this UI thread. Windows
        // may have already destroyed the owned HWND when closing the preview.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn paint_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA points to the stable allocation in Mask until after
    // DestroyWindow returns. Both stock brushes belong to GDI. No Rust callbacks run.
    unsafe {
        if msg == WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            FillRect(dc, &ps.rcPaint, HBRUSH(GetStockObject(BLACK_BRUSH).0));
            let paint = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Paint;
            if let Some(paint) = paint.as_ref() {
                SetDCBrushColor(dc, paint.accent);
                FillRect(dc, &paint.border, HBRUSH(GetStockObject(DC_BRUSH).0));
            }
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surround_leaves_output_and_panel_open_on_negative_desktop() {
        let screen = PixelRect {
            left: -1920,
            top: -180,
            right: 0,
            bottom: 900,
        };
        let output = PixelRect {
            left: -1700,
            top: 220,
            right: -100,
            bottom: 700,
        };
        let panel = PixelRect {
            left: -290,
            top: 140,
            right: -12,
            bottom: 780,
        };
        let region = surround(screen, output, panel).unwrap();
        let full = surround(screen, screen, panel).unwrap();
        // SAFETY: both test regions remain live; coordinates are window-relative.
        unsafe {
            let includes = |x, y| PtInRegion(region.0, x - screen.left, y - screen.top).as_bool();
            assert!(includes(-1701, 220));
            assert!(!includes(-1700, 220));
            assert!(!includes(-200, 500));
            assert!(!includes(-50, 150));
            assert!(!includes(0, 0));
            assert!(!PtInRegion(full.0, 0, 0).as_bool());
        }
    }
}
