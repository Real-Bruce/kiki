//! Native popup menus retain keyboard behavior and expose owner-drawn labels through MSAA.
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        UI::{
            Accessibility::{MSAA_MENU_SIG, MSAAMENUINFO},
            Controls::*,
            HiDpi::*,
            WindowsAndMessaging::*,
        },
    },
    core::{PWSTR, w},
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Tray,
    Pinned,
}

#[repr(C)]
struct Item {
    accessible: MSAAMENUINFO,
    text: Vec<u16>,
    font: HFONT,
    dpi: u32,
    style: Style,
    separator: bool,
}

impl Item {
    fn px(&self, dip: i32) -> i32 {
        (dip as f32 * self.dpi as f32 / 96.).round() as i32
    }
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
pub fn tray(owner: HWND, x: i32, y: i32) -> u32 {
    popup(
        owner,
        x,
        y,
        Style::Tray,
        &[
            (1, "开始截图", true),
            (4, "设置", true),
            (0, "", true),
            (3, "关闭贴图", true),
            (2, "退出", true),
        ],
    )
}

pub fn popup(owner: HWND, x: i32, y: i32, style: Style, entries: &[(u32, &str, bool)]) -> u32 {
    // SAFETY: boxes and UTF-16 buffers stay stable until modal tracking ends and the native menu is destroyed.
    unsafe {
        let Ok(handle) = CreatePopupMenu() else {
            return 0;
        };
        let mut dpi = GetDpiForWindow(owner).max(96);
        if style == Style::Tray {
            let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            let mut vertical = dpi;
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut vertical);
        }
        let dpi = dpi.max(96);
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
            if style == Style::Tray {
                CLEARTYPE_QUALITY
            } else {
                DEFAULT_QUALITY
            },
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        );
        let background = CreateSolidBrush(rgb(menu_background(
            style,
            super::selection_render::system_dark(),
        )));
        let mut menu = Menu {
            handle,
            items: Vec::new(),
            font,
            background,
        };
        let info = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as u32,
            fMask: if style == Style::Tray {
                MIM_BACKGROUND | MIM_STYLE
            } else {
                MIM_BACKGROUND
            },
            dwStyle: if style == Style::Tray {
                MNS_NOCHECK
            } else {
                Default::default()
            },
            hbrBack: background,
            ..Default::default()
        };
        let _ = SetMenuInfo(handle, &info);
        for (id, label, enabled) in entries {
            let separator = *id == 0;
            if separator && style == Style::Pinned {
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
                style,
                separator,
            });
            let ptr = &*item as *const Item;
            let info = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_ID | MIIM_FTYPE | MIIM_DATA | MIIM_STATE,
                wID: *id,
                fType: if separator {
                    MFT_OWNERDRAW | MFT_SEPARATOR
                } else {
                    MFT_OWNERDRAW
                },
                fState: if *enabled && !separator {
                    MFS_ENABLED
                } else {
                    MFS_DISABLED
                },
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
        if style == Style::Tray {
            super::settings_render::release_control_surface();
        }
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
            if item.separator {
                info.itemWidth = 0;
                info.itemHeight = item.px(9) as u32;
                return Some(LRESULT(1));
            }
            let scale = item.dpi as f32 / 96.;
            let dc = GetDC(None);
            let old = SelectObject(dc, item.font.into());
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &item.text[..item.text.len() - 1], &mut size);
            SelectObject(dc, old);
            ReleaseDC(None, dc);
            (info.itemWidth, info.itemHeight) = if item.style == Style::Tray {
                ((size.cx + item.px(24)) as u32, item.px(34) as u32)
            } else {
                (
                    (size.cx.max(190) as f32 + 56. * scale) as u32,
                    (36. * scale) as u32,
                )
            };
            return Some(LRESULT(1));
        }
        if msg == WM_DRAWITEM {
            let info = &*(lp.0 as *const DRAWITEMSTRUCT);
            if info.CtlType != ODT_MENU || info.itemData == 0 {
                return None;
            }
            let item = &*(info.itemData as *const Item);
            let dark = super::selection_render::system_dark();
            let surface = menu_background(item.style, dark);
            if item.separator {
                fill(info.hDC, &info.rcItem, surface);
                let pad = item.px(9);
                let y = (info.rcItem.top + info.rcItem.bottom) / 2;
                fill(
                    info.hDC,
                    &RECT {
                        left: info.rcItem.left + pad,
                        right: info.rcItem.right - pad,
                        top: y,
                        bottom: y + 1,
                    },
                    if dark { 0x46484c } else { 0xe2e5ea },
                );
                return Some(LRESULT(1));
            }
            let selected = info.itemState.0 & ODS_SELECTED.0 != 0;
            let disabled = info.itemState.0 & (ODS_DISABLED.0 | ODS_GRAYED.0) != 0;
            let bg = if selected {
                if dark { 0x174b73 } else { 0xdcecff }
            } else {
                surface
            };
            if item.style == Style::Tray && selected && !disabled {
                let horizontal = item.px(4);
                let vertical = item.px(1);
                let r = info.rcItem;
                let rounded =
                    super::settings_render::paint_control(info.hDC, r, surface, |shape| {
                        shape.rounded(
                            RECT {
                                left: r.left + horizontal,
                                right: r.right - horizontal,
                                top: r.top + vertical,
                                bottom: r.bottom - vertical,
                            },
                            item.px(12),
                            bg,
                        );
                    });
                if rounded.is_none() {
                    fill(info.hDC, &r, bg);
                }
            } else {
                fill(info.hDC, &info.rcItem, bg);
            }
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
            let pad = item.px(if item.style == Style::Tray { 12 } else { 16 });
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
/// Style the live tray menu delivered by WM_ENTERIDLE without changing system settings.
pub fn style_tray_popup(owner: HWND, popup: HWND) {
    if popup.is_invalid() || popup == owner {
        return;
    }
    // SAFETY: this is the live HWND from the synchronous menu notification. Only
    // the system popup class is eligible, and DWM copies the preference values.
    unsafe {
        if !is_class(popup, "#32768") {
            return;
        }
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            popup,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            std::mem::size_of_val(&corner) as u32,
        );
        hide_tray_shadow(popup);
    }
}

fn hide_tray_shadow(popup: HWND) {
    // SAFETY: work only on the native shadow immediately behind the live popup on
    // its owning UI thread. The shadow is owned and destroyed by Windows.
    unsafe {
        let Ok(shadow) = GetWindow(popup, GW_HWNDNEXT) else {
            return;
        };
        let thread = GetWindowThreadProcessId(popup, None);
        if thread == 0
            || GetWindowThreadProcessId(shadow, None) != thread
            || !is_class(shadow, "SysShadow")
            || !IsWindowVisible(shadow).as_bool()
        {
            return;
        }
        let mut menu_bounds = RECT::default();
        let mut shadow_bounds = RECT::default();
        if GetWindowRect(popup, &mut menu_bounds).is_ok()
            && GetWindowRect(shadow, &mut shadow_bounds).is_ok()
            && shadow_bounds.left == menu_bounds.left
            && shadow_bounds.top == menu_bounds.top
            && shadow_bounds.right >= menu_bounds.right
            && shadow_bounds.bottom >= menu_bounds.bottom
        {
            let _ = ShowWindow(shadow, SW_HIDE);
        }
    }
}

fn is_class(window: HWND, expected: &str) -> bool {
    // SAFETY: GetClassNameW writes only within the local buffer and returns its length.
    unsafe {
        let mut class = [0u16; 16];
        let len = GetClassNameW(window, &mut class) as usize;
        len != 0 && class[..len].iter().copied().eq(expected.encode_utf16())
    }
}

fn menu_background(style: Style, dark: bool) -> u32 {
    match (style, dark) {
        (Style::Tray, _) => super::settings_render::background(dark),
        (Style::Pinned, true) => 0x292929,
        (Style::Pinned, false) => 0xffffff,
    }
}

fn fill(dc: HDC, rect: &RECT, color: u32) {
    // SAFETY: the DC is borrowed for this paint and FillRect does not retain the brush.
    unsafe {
        let brush = CreateSolidBrush(rgb(color));
        FillRect(dc, rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn rgb(value: u32) -> COLORREF {
    COLORREF(((value & 255) << 16) | (value & 0xff00) | ((value >> 16) & 255))
}
