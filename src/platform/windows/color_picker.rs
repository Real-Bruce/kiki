#[derive(Clone, Copy, Default)]
pub(super) enum Format {
    #[default]
    Hex,
    Rgb,
}

impl Format {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Hex => Self::Rgb,
            Self::Rgb => Self::Hex,
        };
    }

    pub fn encode(self, [r, g, b]: [u8; 3]) -> String {
        match self {
            Self::Hex => format!("#{r:02X}{g:02X}{b:02X}"),
            Self::Rgb => format!("RGB({r}, {g}, {b})"),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Hex => "单击复制 · Shift 切换 RGB",
            Self::Rgb => "单击复制 · Shift 切换 HEX",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Notice {
    Copying,
    Copied,
    Failed,
}

impl Notice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Copying => "正在复制颜色…",
            Self::Copied => "颜色已复制",
            Self::Failed => "复制失败，请单击重试",
        }
    }
}

pub(super) struct Readout {
    pub format: Format,
    pub rgb: Option<[u8; 3]>,
    pub notice: Option<Notice>,
}
