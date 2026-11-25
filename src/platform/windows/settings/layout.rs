//! Settings page geometry shared by native control positioning and canvas painting.
use super::render::Paragraph;
use crate::core::annotation::Tool;
use crate::core::config::{CaptureParameter, Config};
use crate::platform::windows::app::startup;
use windows::Win32::Graphics::DirectWrite::IDWriteFactory;

pub const HEADER: i32 = 68;
pub const OPEN_CONFIG_FOLDER: u16 = 203;
pub const PARAMETER_SLIDER: u16 = 420;
pub const OUTPUT_FORMAT: u16 = 430;
pub const RESET_CAPTURE: u16 = 431;
pub const CAPTURE_TOOL: u16 = 432;
pub const CAPTURE_TOOL_IDS: std::ops::Range<u16> =
    CAPTURE_TOOL..CAPTURE_TOOL + Tool::TOOLBAR.len() as u16;
pub const TOOLBAR_PREVIEW: u16 = 440;

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

pub fn parameter_visible(parameter: CaptureParameter, cfg: &Config) -> bool {
    let tool = match parameter {
        CaptureParameter::Stroke(0) => Tool::Rectangle,
        CaptureParameter::Stroke(1) => Tool::Arrow,
        CaptureParameter::Stroke(2) => Tool::Pen,
        CaptureParameter::TextSize => Tool::Text,
        CaptureParameter::MosaicSize => Tool::Mosaic,
        CaptureParameter::SequenceSize => Tool::Sequence,
        _ => return true,
    };
    tool.index().is_some_and(|index| cfg.capture_tools[index])
}

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
    Paragraph(Paragraph, Rect, Tone),
    Value(String, Rect),
    ParameterValue(u16, Rect),
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
    pub panels: Vec<Rect>,
    y: i32,
    panel_top: i32,
    text_factory: IDWriteFactory,
}

impl Page {
    pub fn new(
        width: i32,
        (page, tools_expanded): (usize, bool),
        cfg: &Config,
        start: &Result<startup::State, String>,
        hotkey_active: bool,
        shortcut_error: Option<&str>,
        text_factory: &IDWriteFactory,
    ) -> Self {
        let sidebar = if width >= 790 { 176 } else { 148 };
        let margin = if width >= 790 { 32 } else { 24 };
        let body = (width - sidebar - 2 * margin).clamp(1, 720);
        let mut p = Self {
            left: sidebar + (width - sidebar - body).max(0) / 2 + 20,
            width: (body - 40).max(1),
            sidebar,
            items: Vec::new(),
            end: 0,
            panels: Vec::new(),
            y: HEADER + 26,
            panel_top: HEADER + 6,
            text_factory: text_factory.clone(),
        };
        match page {
            0 => {
                p.heading("启动与后台");
                let moved = matches!(start, Ok(startup::State::Moved));
                p.toggle_row("登录 Windows 时自动启动", 200);
                if moved {
                    p.rule();
                    p.note("kiki 已移动，开机启动需要更新。", Tone::Muted);
                    p.y += 8;
                    p.control(201, 0, 0, 152, 36);
                    p.y += 36;
                }
                p.divider();
                p.heading("设置文件");
                p.control(OPEN_CONFIG_FOLDER, 0, 0, 152, 36);
                p.y += 36;
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
                        "当前快捷键不可用。请换一组按键，或从系统托盘开始截图。",
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
                p.heading("工具条");
                let preview_height =
                    crate::platform::windows::editor::toolbar::BAR_HEIGHT as i32 + 8;
                p.control(TOOLBAR_PREVIEW, 0, 0, p.width, preview_height);
                p.y += preview_height + 12;
                if tools_expanded {
                    for id in CAPTURE_TOOL_IDS {
                        p.control(id, 0, -8, p.width, 36);
                        p.y += ROW;
                    }
                    p.rule();
                }
                p.toggle_row("截图时显示操作提示", 400);
                p.divider();
                p.heading("样式调整");
                p.text("标注颜色", 0, 86, Tone::Normal);
                p.choices(410, 8);
                for (index, parameter) in [
                    (0, CaptureParameter::Stroke(0)),
                    (1, CaptureParameter::Stroke(1)),
                    (2, CaptureParameter::Stroke(2)),
                    (3, CaptureParameter::TextSize),
                    (5, CaptureParameter::MosaicSize),
                    (6, CaptureParameter::SequenceSize),
                ] {
                    if parameter_visible(parameter, cfg) {
                        p.parameter_row(index);
                    }
                }
                p.divider();
                p.heading("保存与圆角");
                p.text("默认保存格式", 0, p.width - 126, Tone::Normal);
                p.control(OUTPUT_FORMAT, p.width - 116, -8, 116, 36);
                p.y += ROW;
                p.items.push(Item::CornerPreview(p.rect(0, 0, 108, 72)));
                let start = 132;
                p.control(PARAMETER_SLIDER + 4, start, 22, p.width - start - 78, 28);
                p.items.push(Item::ParameterValue(
                    PARAMETER_SLIDER + 4,
                    p.rect(p.width - 72, 26, 72, 22),
                ));
                p.y += 72;
                p.rule();
                p.control(RESET_CAPTURE, 0, 0, 152, 36);
                p.y += 36;
            }
            3 => {
                p.heading("置顶贴图");
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
                p.heading("主题");
                let gap = if p.width >= 500 { 18 } else { 8 };
                let size = ((p.width - 2 * gap) / 3).clamp(1, 174);
                for i in 0..3 {
                    p.control(600 + i, i32::from(i) * (size + gap), 0, size, 130);
                }
                p.y += 142;
            }
            _ => {
                p.heading("kiki");
                p.note(
                    concat!("版本 ", env!("KIKI_VERSION"), " · Windows 11 x64"),
                    Tone::Muted,
                );
                p.divider();
                p.heading("使用帮助");
                p.help_topic(text_factory, "开始截图", &capture_help(cfg, hotkey_active));
                p.help_topic(
                    text_factory,
                    "框选与标注",
                    "拖动框选或单击选择窗口，拖动选区边缘调整范围。用工具条添加标注，Ctrl+Z 撤销，Ctrl+Y 重做，Esc 取消。",
                );
                p.help_topic(
                    text_factory,
                    "复制、保存与贴图",
                    "Enter 复制，Ctrl+V 粘贴。Ctrl+S 可保存为 PNG 或 JPEG，Ctrl+P 将截图置顶。",
                );
                p.help_topic(
                    text_factory,
                    "管理贴图",
                    "拖动移动，滚轮缩放，Ctrl+0 恢复原始比例。Ctrl+C 复制，Ctrl+S 保存，Esc 关闭。",
                );
                p.help_topic(
                    text_factory,
                    "系统托盘",
                    "右键单击 kiki 图标，可打开设置、关闭所有贴图或退出。",
                );
            }
        }
        p.finish_panel();
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
        self.y += 36;
    }

    fn text(&mut self, text: impl Into<String>, x: i32, w: i32, tone: Tone) {
        self.items
            .push(Item::Text(text.into(), self.rect(x, 0, w, ROW), tone));
    }

    fn note(&mut self, text: &str, tone: Tone) {
        let size = if matches!(tone, Tone::Normal) {
            14.
        } else {
            12.
        };
        if let Ok(paragraph) = Paragraph::new(&self.text_factory, text, size, self.width) {
            let height = paragraph.height;
            self.items.push(Item::Paragraph(
                paragraph,
                self.rect(0, 0, self.width, height),
                tone,
            ));
            self.y += height + 8;
        } else {
            self.text(text, 0, self.width, tone);
            self.y += ROW;
        }
    }

    fn help_topic(&mut self, factory: &IDWriteFactory, title: &str, detail: &str) {
        self.paragraph(factory, title, Tone::Normal);
        self.y += 4;
        self.paragraph(factory, detail, Tone::Muted);
        self.y += 16;
    }

    fn paragraph(&mut self, factory: &IDWriteFactory, text: &str, tone: Tone) {
        let size = if matches!(tone, Tone::Normal) {
            14.
        } else {
            12.
        };
        if let Ok(paragraph) = Paragraph::new(factory, text, size, self.width) {
            let height = paragraph.height;
            self.items.push(Item::Paragraph(
                paragraph,
                self.rect(0, 0, self.width, height),
                tone,
            ));
            self.y += height;
        } else {
            self.note(text, tone);
        }
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

    fn parameter_row(&mut self, index: u16) {
        let start = (self.width - 360).max(132);
        self.text(
            PARAMETER_LABELS[usize::from(index)],
            0,
            start - 10,
            Tone::Normal,
        );
        self.control(
            PARAMETER_SLIDER + index,
            start,
            5,
            self.width - start - 78,
            28,
        );
        self.items.push(Item::ParameterValue(
            PARAMETER_SLIDER + index,
            self.rect(self.width - 72, 9, 72, 22),
        ));
        self.y += 48;
    }

    fn divider(&mut self) {
        self.finish_panel();
        self.y += 36;
        self.panel_top = self.y - 20;
    }

    fn finish_panel(&mut self) {
        self.y += 20;
        self.panels.push(Rect {
            x: self.left - 20,
            y: self.panel_top,
            w: self.width + 40,
            h: self.y - self.panel_top,
        });
    }

    fn rule(&mut self) {
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

fn capture_help(cfg: &Config, hotkey_active: bool) -> String {
    if hotkey_active {
        format!("按 {}，也可从系统托盘开始截图。", cfg.hotkey.label())
    } else {
        "从系统托盘开始截图。可在「快捷键」页修改截图快捷键。".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Key;

    #[test]
    fn general_removes_capture_entry_and_separates_startup_repair() {
        let factory = super::super::render::text_factory().unwrap();
        let cfg = Config::default();
        for width in [630, 660, 944] {
            for state in [
                startup::State::Disabled,
                startup::State::Enabled,
                startup::State::Moved,
            ] {
                let moved = matches!(state, startup::State::Moved);
                let page = Page::new(width, (0, false), &cfg, &Ok(state), false, None, &factory);
                let control = |id| {
                    page.items.iter().find_map(|item| match item {
                        Item::Control(control_id, bounds) if *control_id == id => Some(bounds),
                        _ => None,
                    })
                };
                assert_eq!(page.panels.len(), 2);
                assert!(control(202).is_none());
                assert!(control(OPEN_CONFIG_FOLDER).is_some());
                assert_eq!(control(201).is_some(), moved);
                if let Some(repair) = control(201) {
                    let toggle = control(200).unwrap();
                    assert!(repair.y - (toggle.y + toggle.h) >= 16);
                    assert!(repair.y + repair.h < page.panels[1].y);
                }
            }
        }
    }

    #[test]
    fn grouped_pages_keep_controls_inside_nonoverlapping_panels() {
        let factory = super::super::render::text_factory().unwrap();
        let startup = Ok(startup::State::Moved);
        for width in [630, 660, 790, 944, 1280] {
            for tools in [
                [false; 8],
                crate::core::config::DEFAULT_CAPTURE_TOOLS,
                [true; 8],
            ] {
                let cfg = Config {
                    capture_tools: tools,
                    ..Config::default()
                };
                for index in 0..6 {
                    for expanded in [false, true] {
                        let page = Page::new(
                            width,
                            (index, expanded),
                            &cfg,
                            &startup,
                            false,
                            Some("快捷键已被其他程序使用，请换一组按键。"),
                            &factory,
                        );
                        assert!(!page.panels.is_empty());
                        if index == 2 {
                            assert_eq!(page.panels.len(), 3);
                        }
                        for panels in page.panels.windows(2) {
                            assert!(panels[1].y - (panels[0].y + panels[0].h) >= 16);
                        }
                        for item in &page.items {
                            let bounds = match item {
                                Item::Control(_, r)
                                | Item::ParameterValue(_, r)
                                | Item::CornerPreview(r)
                                | Item::Paragraph(_, r, _) => r,
                                _ => continue,
                            };
                            assert!(bounds.w > 0 && bounds.h > 0);
                            assert!(
                                page.panels.iter().any(|panel| bounds.x >= panel.x + 19
                                    && bounds.x + bounds.w <= panel.x + panel.w - 19
                                    && bounds.y >= panel.y + 19
                                    && bounds.y + bounds.h <= panel.y + panel.h - 19),
                                "page {index}, width {width}: control outside panel"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn capture_preview_moves_only_following_content_and_keeps_parameter_rows() {
        let mut cfg = Config::default();
        let mosaic = Tool::Mosaic.index().unwrap();
        assert!(!parameter_visible(CaptureParameter::MosaicSize, &cfg));
        cfg.capture_tools[mosaic] = true;
        let startup = Ok(startup::State::Disabled);
        let factory = super::super::render::text_factory().unwrap();
        let bounds = |page: &Page, id| {
            page.items.iter().find_map(|item| match item {
                Item::Control(control_id, bounds) if *control_id == id => Some(*bounds),
                _ => None,
            })
        };
        for width in [630, 944] {
            let page =
                |expanded| Page::new(width, (2, expanded), &cfg, &startup, true, None, &factory);
            let closed = page(false);
            let open = page(true);
            let corner = closed
                .items
                .iter()
                .find_map(|item| match item {
                    Item::CornerPreview(bounds) => Some(bounds),
                    _ => None,
                })
                .unwrap();
            let corner_slider = bounds(&closed, PARAMETER_SLIDER + 4).unwrap();
            assert!(corner.x + corner.w < corner_slider.x);
            assert_eq!(
                corner.y + corner.h / 2,
                corner_slider.y + corner_slider.h / 2
            );
            let height_added = open.end - closed.end;
            assert!(height_added > 0);
            let preview = bounds(&open, TOOLBAR_PREVIEW).unwrap();
            assert_eq!(preview.w, open.width);
            assert_eq!(preview.y, bounds(&closed, TOOLBAR_PREVIEW).unwrap().y);
            let mut previous_bottom = preview.y + preview.h;
            for id in CAPTURE_TOOL_IDS {
                assert!(bounds(&closed, id).is_none());
                let row = bounds(&open, id).unwrap();
                assert_eq!((row.x, row.w, row.h), (open.left, open.width, 36));
                assert!(row.y >= previous_bottom);
                previous_bottom = row.y + row.h;
            }
            for index in 0..=5 {
                let id = PARAMETER_SLIDER + index;
                let slider = bounds(&closed, id).unwrap();
                assert_eq!(bounds(&open, id).unwrap().y - slider.y, height_added);
                assert!(closed.items.iter().any(|item| matches!(item,
                    Item::ParameterValue(control_id, readout) if *control_id == id
                        && readout.x > slider.x + slider.w
                        && readout.x + readout.w == closed.left + closed.width
                )));
            }
            for (tool, hidden_row) in [
                (Tool::Rectangle, 0),
                (Tool::Arrow, 1),
                (Tool::Pen, 2),
                (Tool::Text, 3),
                (Tool::Mosaic, 5),
            ] {
                let mut config = cfg.clone();
                config.capture_tools[tool.index().unwrap()] = false;
                let updated = Page::new(width, (2, false), &config, &startup, true, None, &factory);
                assert_eq!(closed.end - updated.end, 48);
                for index in 0..=5 {
                    assert_eq!(
                        bounds(&updated, PARAMETER_SLIDER + index).is_some(),
                        index != hidden_row
                    );
                }
            }
        }
    }

    #[test]
    fn about_embeds_help_and_uses_registered_shortcut() {
        let mut cfg = Config::default();
        cfg.hotkey.key = Key::Function(18);
        let shortcut = cfg.hotkey.label();
        let startup = Ok(startup::State::Disabled);
        let factory = super::super::render::text_factory().unwrap();
        let active = Page::new(660, (5, false), &cfg, &startup, true, None, &factory);
        let unavailable = capture_help(&cfg, false);

        assert!(
            active
                .items
                .iter()
                .any(|item| matches!(item, Item::Heading("使用帮助", _)))
        );
        assert!(capture_help(&cfg, true).contains(&shortcut));
        assert!(unavailable.contains("系统托盘") && unavailable.contains("「快捷键」"));
        assert!(!unavailable.contains(&shortcut));
        assert!(
            !active
                .items
                .iter()
                .any(|item| matches!(item, Item::Control(..)))
        );
    }

    #[test]
    fn about_spacing_follows_wrapped_text_height() {
        let cfg = Config::default();
        let startup = Ok(startup::State::Disabled);
        let factory = super::super::render::text_factory().unwrap();
        let paragraphs = |width| {
            Page::new(width, (5, false), &cfg, &startup, true, None, &factory)
                .items
                .into_iter()
                .skip_while(|item| !matches!(item, Item::Heading("使用帮助", _)))
                .filter_map(|item| match item {
                    Item::Paragraph(_, bounds, _) => Some(bounds),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let wide = paragraphs(944);
        let narrow = paragraphs(660);
        for blocks in [&wide, &narrow] {
            assert_eq!(blocks.len(), 10);
            for topic in blocks.chunks_exact(2) {
                assert_eq!(topic[1].y - (topic[0].y + topic[0].h), 4);
            }
            for topics in blocks.windows(2).skip(1).step_by(2) {
                assert_eq!(topics[1].y - (topics[0].y + topics[0].h), 16);
            }
        }
        assert_eq!(wide[3].h, wide[1].h);
        assert!(narrow[3].h > wide[3].h);
    }
}
