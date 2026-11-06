use super::geometry::PixelRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Sdr,
    Hdr,
    Unsupported,
}

/// A physical desktop snapshot descriptor; UI scaling never resizes capture pixels.
#[derive(Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    pub bounds: PixelRect,
    pub name: String,
    pub scale_percent: i32,
    pub rotation_degrees: u16,
    pub refresh_hz: u32,
    pub color: ColorMode,
}
