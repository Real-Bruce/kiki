//! Settings page geometry shared by native control positioning and canvas painting.
use super::startup;
use crate::core::config::Config;

pub const HEADER: i32 = 98;
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
                p.heading("启动与后台");
                let moved = matches!(start, Ok(startup::State::Moved));
                p.text("开机启动", 0, p.width - 64, Tone::Normal);
                p.text_at("登录 Windows 后自动运行", 25, Tone::Muted);
                p.control(200, p.width - 52, 0, 52, 42);
                p.y += 60;
                if moved {
                    p.text(
                        "启动项指向旧路径，可更新或关闭",
                        0,
                        p.width - 174,
                        Tone::Warning,
                    );
                    p.control(201, p.width - 154, -7, 154, 34);
                    p.y += ROW;
                }
                p.row("启动方式", "驻留托盘");
                p.row("关闭设置窗口后", "继续后台运行");
                p.divider();
                p.heading("截图完成后");
                p.row("复制", "复制到剪贴板后退出截图");
                p.row("保存", "保存成功后退出截图");
                p.row("贴图", "生成置顶贴图后退出截图");
                p.note("复制和贴图不会自动保存文件", Tone::Muted);
                p.divider();
                p.heading("文件保存");
                p.row("图片格式", "PNG");
                p.row("保存位置", "每次询问，记住上次目录");
                p.row("默认文件名", "kiki_年月日_时分秒.png");
            }
            1 => {
                p.heading("开始截图");
                let recorder = (p.width - 126).clamp(120, 244);
                p.control(300, 0, 0, recorder, 38);
                p.control(301, recorder + 12, 0, 108, 38);
                p.y += 54;
                p.note(
                    "按 Ctrl 或 Alt 加字母、数字或 F1–F24。冲突时保留原组合。",
                    Tone::Muted,
                );
                p.note("编辑快捷键时暂停唤起截图。", Tone::Muted);
                if !hotkey_active {
                    p.note("快捷键注册失败，仍可从托盘开始截图。", Tone::Warning);
                }
            }
            2 => {
                p.heading("操作提示");
                p.text("显示截图时的快捷键提示", 0, p.width - 64, Tone::Muted);
                p.control(400, p.width - 52, -10, 52, 42);
                p.y += 30;
                p.divider();
                p.heading("默认标注样式");
                let compact = p.width < 510;
                p.text("颜色", 0, p.width, Tone::Normal);
                p.text_at("新标注使用的初始颜色", 25, Tone::Muted);
                p.choices(410, 8, 34, compact, 48);
                p.text("线宽", 0, p.width, Tone::Normal);
                p.choices(420, 4, 76, compact, 26);
                p.text("字号", 0, p.width, Tone::Normal);
                p.choices(430, 4, 76, compact, 26);
            }
            3 => {
                p.heading("窗口操作");
                p.row("移动", "拖动贴图");
                p.row("缩放", "滚轮");
                p.row("恢复原始比例", "Ctrl+0");
                p.note("初次显示按原始大小，超出工作区时自动缩小。", Tone::Muted);
                p.divider();
                p.heading("复制、保存与关闭");
                p.row("复制", "Ctrl+C");
                p.row("保存", "Ctrl+S");
                p.row("关闭当前贴图", "Esc");
                p.note("右键打开菜单，悬停显示复制、保存和关闭按钮。", Tone::Muted);
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
                p.y += 158;
                p.note("立即应用到设置、截图工具、文字输入和贴图。", Tone::Muted);
                p.note("跟随系统会在 Windows 主题变化时更新。", Tone::Muted);
            }
            _ => {
                p.heading("应用信息");
                p.row("版本", format!("kiki {}", env!("CARGO_PKG_VERSION")));
                p.row("平台", "Windows 11 · x64");
                p.row("截图热键", cfg.hotkey.label());
                p.divider();
                p.heading("帮助与资源");
                for (i, label) in ["配置目录", "使用帮助", "第三方许可"]
                    .into_iter()
                    .enumerate()
                {
                    p.text(label, 0, p.width - 132, Tone::Normal);
                    p.control(700 + i as u16, p.width - 120, -6, 120, 34);
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

    fn text_at(&mut self, text: &str, dy: i32, tone: Tone) {
        self.items.push(Item::Text(
            text.into(),
            self.rect(0, dy, self.width, ROW),
            tone,
        ));
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
