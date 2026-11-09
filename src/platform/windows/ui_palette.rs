//! Warm neutrals and pine accents shared by the native UI.
pub const fn background(dark: bool) -> u32 {
    if dark { 0x232321 } else { 0xfaf9f6 }
}
pub const fn sidebar(dark: bool) -> u32 {
    if dark { 0x2d2a26 } else { 0xf1eee8 }
}
pub const fn foreground(dark: bool) -> u32 {
    if dark { 0xf4f0e8 } else { 0x25231f }
}
pub const fn muted(dark: bool) -> u32 {
    if dark { 0xc7bfb4 } else { 0x625d55 }
}
pub const fn divider(dark: bool) -> u32 {
    if dark { 0x49453f } else { 0xdfdbd2 }
}
pub const fn accent(dark: bool) -> u32 {
    if dark { 0xa0cebb } else { 0x365f56 }
}
pub const fn selected(dark: bool) -> u32 {
    if dark { 0x34483e } else { 0xe3ebe5 }
}
pub const fn hover(dark: bool) -> u32 {
    if dark { 0x383530 } else { 0xeeeae4 }
}
pub const fn pressed(dark: bool) -> u32 {
    if dark { 0x433d35 } else { 0xe5dfd6 }
}
pub const fn on_accent(dark: bool) -> u32 {
    if dark { 0x1c2924 } else { 0xffffff }
}
pub const fn accent_hover(dark: bool) -> u32 {
    if dark { 0x96c3b1 } else { 0x30564d }
}
pub const fn accent_pressed(dark: bool) -> u32 {
    if dark { 0x8bb6a5 } else { 0x2a4d45 }
}
