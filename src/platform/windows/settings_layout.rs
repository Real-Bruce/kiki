//! Settings page geometry shared by native control positioning and canvas painting.
use super::startup;
use crate::core::config::{Config, OutputFormat};

pub const HEADER: i32 = 84;
pub const START_CAPTURE: u16 = 202;
pub const PARAMETER_SLIDER: u16 = 420;
pub const PARAMETER_EDIT: u16 = 425;
pub const OUTPUT_FORMAT: u16 = 430;
pub const PARAMETER_LABELS: [&str; 5] =
    ["矩形线宽", "箭头线宽", "画笔线宽", "文字字号", "圆角半径"];
pub const FOOTER: i32 = 56;
const GROUP_GAP: i32 = 26;
const ROW: i32 = 42;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub enum Item {
    Heading(&'static str, i32),
    Text(String, Rect, Tone),
    Value(String, Rect),
    Divider(i32),
    CornerPreview(Rect),
    Control(u16, Rect),
}

#[derive(Clone, Copy)]
pub enum Tone {
    Normal,
    Muted,
    Warning,
}

pub struct Page {
    pub left: i32,
    pub width: i32,
    pub sidebar: i32,
    pub items: Vec<Item>,
    pub end: i32,
    y: i32,
}

impl Page {
    pub fn new(
        width: i32,
        page: usize,
        cfg: &Config,
        start: &Result<startup::State, String>,
        hotkey_active: bool,
        shortcut_error: Option<&str>,
    ) -> Self {
        let sidebar = if width >= 790 { 192 } else { 148 };
        let margin = if width >= 790 { 36 } else { 24 };
        let body = (width - sidebar - 2 * margin).clamp(1, 800);
        let mut p = Self {
            left: sidebar + (width - sidebar - body).max(0) / 2,
            width: body,
            sidebar,
            items: Vec::new(),
            end: 0,
            y: HEADER + 16,
        };
        match page {
            0 => {
                p.heading("开始使用");
                p.control(START_CAPTURE, 0, 0, 132, 38);
                if hotkey_active {
                    p.items.push(Item::Text(
                        format!("快捷键 {}", cfg.hotkey.label()),
                        p.rect(148, 11, p.width - 148, ROW),
                        Tone::Muted,
                    ));
                }
                p.y += 46;
                p.divider();
                p.heading("启动");
                let moved = matches!(start, Ok(startup::State::Moved));
                p.text("登录 Windows 时启动 kiki", 0, p.width - 64, Tone::Normal);
                p.control(200, p.width - 52, -8, 52, 38);
                p.y += ROW;
                if moved {
                    p.text("开机启动仍指向旧位置。", 0, p.width - 174, Tone::Warning);
                    p.control(201, p.width - 154, -7, 154, 34);
                    p.y += ROW;
                }
            }
            1 => {
                p.heading("开始截图");
                let recorder = (p.width - 126).clamp(120, 244);
                p.control(300, 0, 0, recorder, 38);
                p.control(301, recorder + 12, 0, 108, 38);
                p.y += 54;
                p.note(
                    "按 Ctrl 或 Alt，再按字母、数字或功能键（F1 至 F24）。",
                    Tone::Muted,
                );
                if let Some(error) = shortcut_error {
                    p.note(error, Tone::Warning);
                } else if !hotkey_active {
                    p.note(
                        "当前快捷键不可用，请更换组合。仍可从托盘开始截图。",
                        Tone::Warning,
                    );
                }
            }
            2 => {
                p.heading("截图提示");
                p.text("显示操作提示", 0, p.width - 64, Tone::Normal);
                p.control(400, p.width - 52, -10, 52, 42);
                p.y += 30;
                p.divider();
                p.heading("默认标注样式");
                let compact = p.width < 510;
                p.text("颜色", 0, p.width, Tone::Normal);
                p.choices(410, 8, 34, compact, 26);
                for index in 0..4 {
                    p.parameter_row(index);
                }
                p.note("成图实际像素 · 线宽 1–64 px · 字号 8–256 px", Tone::Muted);
                p.divider();
                p.heading("图像输出");
                p.parameter_row(4);
                p.text("输出格式", 0, p.width - 126, Tone::Normal);
                p.control(OUTPUT_FORMAT, p.width - 116, -4, 116, 32);
                p.y += ROW;
                p.items.push(Item::CornerPreview(p.rect(0, 0, 108, 72)));
                p.text(
                    match cfg.output_format {
                        OutputFormat::Png => "PNG 四角透明",
                        OutputFormat::Jpeg => "JPEG 白底 · 品质 90%",
                    },
                    122,
                    p.width - 122,
                    Tone::Normal,
                );
                p.y += 24;
                p.text(
                    "0–256 px · 实际半径最多为选区短边的一半",
                    122,
                    p.width - 122,
                    Tone::Muted,
                );
                p.y += 48;
            }
            3 => {
                p.heading("窗口操作");
                p.row("移动", "拖动贴图");
                p.row("缩放", "滚轮");
                p.row("恢复原始比例", "Ctrl+0");

                p.divider();
                p.heading("贴图快捷键");
                p.row("复制", "Ctrl+C");
                p.row("保存", "Ctrl+S");
                p.row("关闭当前贴图", "Esc");
                p.note("右键贴图可复制、保存或关闭。", Tone::Muted);
                p.y += 16;
                p.control(500, 0, 0, 142, 38);
                p.y += 38;
            }
            4 => {
                p.heading("主题");
                let gap = if p.width >= 500 { 18 } else { 8 };
                let size = ((p.width - 2 * gap) / 3).clamp(1, 174);
                for i in 0..3 {
                    p.control(600 + i, i32::from(i) * (size + gap), 0, size, 130);
                }
                p.y += 142;
            }
            _ => {
                p.heading("应用信息");
                p.row("版本", format!("kiki {}", env!("CARGO_PKG_VERSION")));
                p.row("平台", "Windows 11（64 位）");
                p.divider();
                p.heading("帮助与资源");
                for id in [701, 700, 702] {
                    p.control(id, 0, -6, 132, 34);
                    p.y += ROW;
                }
            }
        }
        p.end = p.y + 20;
        p
    }

    fn rect(&self, x: i32, dy: i32, w: i32, h: i32) -> Rect {
        Rect {
            x: self.left + x,
            y: self.y + dy,
            w: w.max(1),
            h,
        }
    }

    fn heading(&mut self, text: &'static str) {
        self.items.push(Item::Heading(text, self.y));
        self.y += 39;
    }

    fn text(&mut self, text: impl Into<String>, x: i32, w: i32, tone: Tone) {
        self.items
            .push(Item::Text(text.into(), self.rect(x, 0, w, ROW), tone));
    }

    fn note(&mut self, text: &str, tone: Tone) {
        self.text(text, 0, self.width, tone);
        // A Chinese glyph can occupy a full em; use that bound for narrow wrapping notes.
        let lines = (text.chars().count() as u32 * 12)
            .div_ceil(self.width as u32)
            .max(1) as i32;
        self.y += lines * 18 + 9;
    }

    fn row(&mut self, label: &str, value: impl Into<String>) {
        let split = self.width * 2 / 5;
        self.text(label, 0, split - 12, Tone::Normal);
        self.items.push(Item::Value(
            value.into(),
            self.rect(split, 0, self.width - split, ROW),
        ));
        self.y += ROW;
    }

    fn parameter_row(&mut self, index: u16) {
        let start = (self.width - 276).max(110);
        self.text(
            PARAMETER_LABELS[usize::from(index)],
            0,
            start - 10,
            Tone::Normal,
        );
        self.control(
            PARAMETER_SLIDER + index,
            start,
            -3,
            self.width - start - 83,
            28,
        );
        self.control(PARAMETER_EDIT + index, self.width - 74, -3, 49, 28);
        self.text("px", self.width - 18, 18, Tone::Muted);
        self.y += ROW;
    }

    fn divider(&mut self) {
        // A regular row already leaves some space below the last text baseline.
        self.y += GROUP_GAP - 10;
        self.items.push(Item::Divider(self.y));
        self.y += GROUP_GAP + 1;
    }

    fn control(&mut self, id: u16, x: i32, dy: i32, w: i32, h: i32) {
        self.items.push(Item::Control(id, self.rect(x, dy, w, h)));
    }

    fn choices(&mut self, id: u16, count: u16, preferred: i32, compact: bool, label_height: i32) {
        let gap = if compact { 3 } else { 6 };
        let n = i32::from(count);
        let size = preferred.min(((self.width - (n - 1) * gap) / n).max(1));
        let start = self.width - n * size - (n - 1) * gap;
        let dy = if compact { label_height + 8 } else { -6 };
        for i in 0..count {
            self.control(id + i, start + i32::from(i) * (size + gap), dy, size, 36);
        }
        self.y += if compact { dy + 55 } else { 76 };
    }
}
