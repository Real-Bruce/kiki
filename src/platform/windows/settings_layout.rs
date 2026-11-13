//! Settings page geometry shared by native control positioning and canvas painting.
use super::startup;
use crate::core::annotation::Tool;
use crate::core::config::{CaptureParameter, Config, OutputFormat};

pub const HEADER: i32 = 68;
pub const START_CAPTURE: u16 = 202;
pub const PARAMETER_SLIDER: u16 = 420;
pub const PARAMETER_EDIT: u16 = 440;
pub const OUTPUT_FORMAT: u16 = 430;
pub const RESET_CAPTURE: u16 = 431;
pub const CAPTURE_TOOL: u16 = 432;
pub const CAPTURE_TOOL_IDS: std::ops::Range<u16> =
    CAPTURE_TOOL..CAPTURE_TOOL + Tool::TOOLBAR.len() as u16;
pub const PARAMETER_LABELS: [&str; 7] = [
    "矩形线宽",
    "箭头线宽",
    "画笔线宽",
    "文字字号",
    "圆角半径",
    "马赛克颗粒大小",
    "编号直径",
];
pub const FOOTER: i32 = 48;
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
    ToolbarPreview(Rect),
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
        let sidebar = if width >= 790 { 176 } else { 148 };
        let margin = if width >= 790 { 36 } else { 24 };
        let body = (width - sidebar - 2 * margin).clamp(1, 720);
        let mut p = Self {
            left: sidebar + (width - sidebar - body).max(0) / 2,
            width: body,
            sidebar,
            items: Vec::new(),
            end: 0,
            y: HEADER + 6,
        };
        match page {
            0 => {
                p.control(START_CAPTURE, 0, 0, 132, 38);
                if hotkey_active {
                    p.items.push(Item::Text(
                        format!("快捷键 {}", cfg.hotkey.label()),
                        p.rect(148, 9, p.width - 148, ROW),
                        Tone::Muted,
                    ));
                } else {
                    p.text(
                        "可在「快捷键」中修改截图快捷键。",
                        148,
                        p.width - 148,
                        Tone::Muted,
                    );
                }
                p.y += 52;
                p.note("拖动框选区域，或单击选择窗口。", Tone::Normal);
                p.note(
                    "按 Enter 复制，再按 Ctrl+V 粘贴。也可按 Ctrl+S 保存或按 Ctrl+P 贴图。",
                    Tone::Normal,
                );
                p.divider();
                p.heading("启动与后台");
                let moved = matches!(start, Ok(startup::State::Moved));
                p.toggle_row("登录 Windows 时自动启动", 200);
                if moved {
                    p.text(
                        "kiki 已移动，开机启动需要更新。",
                        0,
                        p.width - 146,
                        Tone::Warning,
                    );
                    p.control(201, p.width - 132, -7, 132, 36);
                    p.y += ROW;
                }
                p.note(
                    "关闭设置后，仍可从任务栏右下角的 kiki 图标开始截图。",
                    Tone::Muted,
                );
            }
            1 => {
                p.heading("截图快捷键");
                let recorder = (p.width - 124).clamp(120, 216);
                p.control(300, 0, 0, recorder, 36);
                p.control(301, recorder + 12, 0, 112, 36);
                p.y += 48;
                p.note(
                    "更改时，选中上方输入框，按住 Ctrl 或 Alt，再按字母、数字或 F1 至 F24。",
                    Tone::Muted,
                );
                if let Some(error) = shortcut_error {
                    p.note(error, Tone::Warning);
                } else if !hotkey_active {
                    p.note(
                        "当前快捷键不可用。请换一组按键，或从通用页开始截图。",
                        Tone::Warning,
                    );
                }
                p.divider();
                p.heading("贴图快捷键");
                p.row("恢复原始比例", "Ctrl+0");
                p.row("复制", "Ctrl+C");
                p.row("保存", "Ctrl+S");
                p.row("关闭当前贴图", "Esc");
            }
            2 => {
                p.heading("截图工具条");
                let columns = if p.width >= 576 { 4 } else { 2 };
                let gap = 12;
                let cell = (p.width - (columns - 1) * gap) / columns;
                for i in 0..Tool::TOOLBAR.len() {
                    let x = (i as i32 % columns) * (cell + gap);
                    p.control(CAPTURE_TOOL + i as u16, x, 0, cell, 36);
                    if i as i32 % columns == columns - 1 {
                        p.y += 38;
                    }
                }
                p.y += 6;
                p.note("工具条预览", Tone::Muted);
                let preview_height = super::super::capture_toolbar::BAR_HEIGHT as i32;
                p.items
                    .push(Item::ToolbarPreview(p.rect(0, 0, p.width, preview_height)));
                p.y += preview_height + 16;
                p.toggle_row("截图时显示操作提示", 400);
                p.divider();
                p.heading("默认标注样式");
                p.note("用于新标注。尺寸按截图的实际像素计算。", Tone::Muted);
                p.text("标注颜色", 0, 86, Tone::Normal);
                p.choices(410, 8);
                for index in 0..3 {
                    p.parameter_row(index, CaptureParameter::Stroke(usize::from(index)));
                }
                p.parameter_row(3, CaptureParameter::TextSize);
                if Tool::Mosaic
                    .index()
                    .is_some_and(|index| cfg.capture_tools[index])
                {
                    p.parameter_row(5, CaptureParameter::MosaicSize);
                }
                if Tool::Sequence
                    .index()
                    .is_some_and(|index| cfg.capture_tools[index])
                {
                    p.parameter_row(6, CaptureParameter::SequenceSize);
                }
                p.divider();
                p.heading("保存与圆角");
                p.text("默认保存格式", 0, p.width - 126, Tone::Normal);
                p.control(OUTPUT_FORMAT, p.width - 116, -8, 116, 36);
                p.y += ROW;
                p.parameter_row(4, CaptureParameter::CornerRadius);
                p.y += 8;
                p.items.push(Item::CornerPreview(p.rect(0, 0, 108, 72)));
                p.text(
                    match cfg.output_format {
                        OutputFormat::Png => "保存为 PNG 时，圆角外透明。",
                        OutputFormat::Jpeg => "保存为 JPEG 时，圆角外填白。",
                    },
                    122,
                    p.width - 122,
                    Tone::Normal,
                );
                p.y += 26;
                p.text(
                    "圆角半径最多为截图短边的一半。",
                    122,
                    p.width - 122,
                    Tone::Muted,
                );
                p.y += 46;
                p.divider();
                p.control(RESET_CAPTURE, 0, 0, 152, 36);
                p.y += 36;
            }
            3 => {
                p.note("截图时按 Ctrl+P，图片会显示在其他窗口上方。", Tone::Normal);
                p.divider();
                p.heading("窗口操作");
                p.row("移动", "拖动贴图");
                p.row("缩放", "滚轮");
                p.row("复制、保存或关闭", "右键单击贴图");
                p.y += 16;
                p.control(500, 0, 0, 142, 38);
                p.y += 38;
            }
            4 => {
                let gap = if p.width >= 500 { 18 } else { 8 };
                let size = ((p.width - 2 * gap) / 3).clamp(1, 174);
                for i in 0..3 {
                    p.control(600 + i, i32::from(i) * (size + gap), 0, size, 130);
                }
                p.y += 142;
            }
            _ => {
                p.row("版本", concat!("kiki ", env!("KIKI_VERSION")));
                p.divider();
                for id in [701, 700, 702] {
                    p.control(id, 0, 0, 152, 36);
                    p.y += ROW;
                }
            }
        }
        p.end = p.y + 18;
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
        self.y += 32;
    }

    fn text(&mut self, text: impl Into<String>, x: i32, w: i32, tone: Tone) {
        self.items
            .push(Item::Text(text.into(), self.rect(x, 0, w, ROW), tone));
    }

    fn note(&mut self, text: &str, tone: Tone) {
        self.text(text, 0, self.width, tone);
        // A Chinese glyph can occupy a full em; use that bound for narrow wrapping notes.
        let size = if matches!(tone, Tone::Normal) { 14 } else { 12 };
        let lines = (text.chars().count() as u32 * size)
            .div_ceil(self.width as u32)
            .max(1) as i32;
        self.y += lines * 20 + 4;
    }

    fn row(&mut self, label: &str, value: impl Into<String>) {
        let split = self.width / 2;
        self.text(label, 0, split - 12, Tone::Normal);
        self.items.push(Item::Value(
            value.into(),
            self.rect(split, 0, self.width - split, ROW),
        ));
        self.y += ROW;
    }

    fn parameter_row(&mut self, index: u16, parameter: CaptureParameter) {
        let start = (self.width - 360).max(132);
        self.text(
            PARAMETER_LABELS[usize::from(index)],
            0,
            start - 10,
            Tone::Normal,
        );
        let (min, max) = parameter.bounds();
        self.items.push(Item::Text(
            format!("{min}–{max} px"),
            self.rect(0, 20, start - 10, 18),
            Tone::Muted,
        ));
        self.control(
            PARAMETER_SLIDER + index,
            start,
            5,
            self.width - start - 83,
            28,
        );
        self.control(PARAMETER_EDIT + index, self.width - 74, 5, 49, 28);
        self.items.push(Item::Text(
            "px".into(),
            self.rect(self.width - 18, 10, 18, 18),
            Tone::Muted,
        ));
        self.y += 48;
    }

    fn divider(&mut self) {
        self.y += 12;
        self.items.push(Item::Divider(self.y));
        self.y += 21;
    }

    fn toggle_row(&mut self, label: &str, id: u16) {
        self.text(label, 0, self.width - 64, Tone::Normal);
        self.control(id, self.width - 52, -8, 52, 36);
        self.y += 32;
    }

    fn control(&mut self, id: u16, x: i32, dy: i32, w: i32, h: i32) {
        self.items.push(Item::Control(id, self.rect(x, dy, w, h)));
    }

    fn choices(&mut self, id: u16, count: u16) {
        let gap = 4;
        let size = 36;
        let width = i32::from(count) * (size + gap) - gap;
        let compact = self.width < width + 94;
        let start = if compact { 0 } else { self.width - width };
        let dy = if compact { 24 } else { -8 };
        for i in 0..count {
            self.control(id + i, start + i32::from(i) * (size + gap), dy, size, 36);
        }
        self.y += dy + 46;
    }
}
