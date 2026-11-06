//! Native popup menus retain keyboard behavior and expose owner-drawn labels through MSAA.
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Accessibility::{MSAA_MENU_SIG, MSAAMENUINFO},
            Controls::*,
            HiDpi::*,
            WindowsAndMessaging::*,
        },
    },
    core::PWSTR,
};
#[repr(C)]
struct Item {
    accessible: MSAAMENUINFO,
    text: Vec<u16>,
    font: HFONT,
    dpi: u32,
}
struct Menu {
    handle: HMENU,
    // Native dwItemData pointers must remain stable when the vector grows.
    #[allow(clippy::vec_box)]
    items: Vec<Box<Item>>,
    font: HFONT,
    background: HBRUSH,
}
impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.handle);
            let _ = DeleteObject(self.font.into());
            let _ = DeleteObject(self.background.into());
        }
    }
}
pub fn popup(owner: HWND, x: i32, y: i32, entries: &[(u32, &str, bool)]) -> u32 {
    // SAFETY: boxes and UTF-16 buffers stay stable until modal tracking ends and the native menu is destroyed.
    unsafe {
        let Ok(handle) = CreatePopupMenu() else {
            return 0;
        };
        let dpi = GetDpiForWindow(owner).max(96);
        let font = CreateFontW(
            -((14 * dpi / 96) as i32),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            DEFAULT_PITCH.0 as u32,
            windows::core::w!("Segoe UI"),
        );
        let background = CreateSolidBrush(rgb(if super::selection_render::system_dark() {
            0x292929
        } else {
            0xffffff
        }));
        let mut menu = Menu {
            handle,
            items: Vec::new(),
            font,
            background,
        };
        let info = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND,
            hbrBack: background,
            ..Default::default()
        };
        let _ = SetMenuInfo(handle, &info);
        for (id, label, enabled) in entries {
            if *id == 0 {
                let _ = AppendMenuW(handle, MF_SEPARATOR, 0, None);
                continue;
            }
            let mut text: Vec<_> = label.encode_utf16().chain(Some(0)).collect();
            let item = Box::new(Item {
                accessible: MSAAMENUINFO {
                    dwMSAASignature: MSAA_MENU_SIG as u32,
                    cchWText: (text.len() - 1) as u32,
                    pszWText: PWSTR(text.as_mut_ptr()),
                },
                text,
                font,
                dpi,
            });
            let ptr = &*item as *const Item;
            let info = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_ID | MIIM_FTYPE | MIIM_DATA | MIIM_STATE,
                wID: *id,
                fType: MFT_OWNERDRAW,
                fState: if *enabled { MFS_ENABLED } else { MFS_DISABLED },
                dwItemData: ptr as usize,
                ..Default::default()
            };
            if InsertMenuItemW(handle, u32::MAX, true, &info).is_err() {
                return 0;
            }
            menu.items.push(item);
        }
        let _ = SetForegroundWindow(owner);
        let command = TrackPopupMenu(
            handle,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            x,
            y,
            None,
            owner,
            None,
        )
        .0 as u32;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        command
    }
}
pub fn handle(msg: u32, lp: LPARAM) -> Option<LRESULT> {
    // SAFETY: only ODT_MENU messages from our own synchronous native menus are interpreted; every item data points to a stable Item.
    unsafe {
        if lp.0 == 0 {
            return None;
        }
        if msg == WM_MEASUREITEM {
            let info = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
            if info.CtlType != ODT_MENU || info.itemData == 0 {
                return None;
            }
            let item = &*(info.itemData as *const Item);
            let scale = item.dpi as f32 / 96.;
            let dc = GetDC(None);
            let old = SelectObject(dc, item.font.into());
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &item.text[..item.text.len() - 1], &mut size);
            SelectObject(dc, old);
            ReleaseDC(None, dc);
            info.itemWidth = (size.cx.max(190) as f32 + 56. * scale) as u32;
            info.itemHeight = (36. * scale) as u32;
            return Some(LRESULT(1));
        }
        if msg == WM_DRAWITEM {
            let info = &*(lp.0 as *const DRAWITEMSTRUCT);
            if info.CtlType != ODT_MENU || info.itemData == 0 {
                return None;
            }
            let item = &*(info.itemData as *const Item);
            let dark = super::selection_render::system_dark();
            let selected = info.itemState.0 & ODS_SELECTED.0 != 0;
            let disabled = info.itemState.0 & (ODS_DISABLED.0 | ODS_GRAYED.0) != 0;
            let bg = if selected {
                if dark { 0x174b73 } else { 0xdcecff }
            } else if dark {
                0x292929
            } else {
                0xffffff
            };
            let brush = CreateSolidBrush(rgb(bg));
            FillRect(info.hDC, &info.rcItem, brush);
            let _ = DeleteObject(brush.into());
            SetBkMode(info.hDC, TRANSPARENT);
            SetTextColor(
                info.hDC,
                rgb(if disabled {
                    0x858585
                } else if dark {
                    0xf2f2f2
                } else {
                    0x202020
                }),
            );
            let old = SelectObject(info.hDC, item.font.into());
            let mut rect = info.rcItem;
            let pad = (16 * item.dpi / 96) as i32;
            rect.left += pad;
            rect.right -= pad;
            let text = String::from_utf16_lossy(&item.text[..item.text.len() - 1]);
            let parts: Vec<_> = text.split('\t').collect();
            let mut label: Vec<_> = parts[0].encode_utf16().collect();
            DrawTextW(
                info.hDC,
                &mut label,
                &mut rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            if let Some(shortcut) = parts.get(1) {
                let mut shortcut: Vec<_> = shortcut.encode_utf16().collect();
                DrawTextW(
                    info.hDC,
                    &mut shortcut,
                    &mut rect,
                    DT_RIGHT | DT_VCENTER | DT_SINGLELINE,
                );
            }
            SelectObject(info.hDC, old);
            return Some(LRESULT(1));
        }
        None
    }
}
fn rgb(value: u32) -> COLORREF {
    COLORREF(((value & 255) << 16) | (value & 0xff00) | ((value >> 16) & 255))
}
