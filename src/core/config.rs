//! Versioned, bounded preferences without platform resources or a serialization runtime.
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Function(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}
impl Default for Hotkey {
    fn default() -> Self {
        Self {
            control: true,
            alt: true,
            shift: false,
            key: Key::Character('A'),
        }
    }
}
impl Hotkey {
    pub fn validate(self) -> Result<(), String> {
        if !self.control && !self.alt {
            return Err("截图热键至少包含 Ctrl 或 Alt".into());
        }
        match self.key {
            Key::Character(c) if c.is_ascii_uppercase() || c.is_ascii_digit() => Ok(()),
            Key::Function(1..=24) => Ok(()),
            _ => Err("热键需要字母、数字或 F1–F24".into()),
        }
    }
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        parts.push(match self.key {
            Key::Character(c) => c.to_string(),
            Key::Function(n) => format!("F{n}"),
        });
        parts.join("+")
    }
    fn parse(value: &str) -> Result<Self, String> {
        let mut hotkey = Self {
            control: false,
            alt: false,
            shift: false,
            key: Key::Character(' '),
        };
        let mut seen = BTreeSet::new();
        let mut key = None;
        for part in value.split('+') {
            if !seen.insert(part) {
                return Err("热键包含重复按键".into());
            }
            match part {
                "Ctrl" => hotkey.control = true,
                "Alt" => hotkey.alt = true,
                "Shift" => hotkey.shift = true,
                _ => {
                    if key.is_some() {
                        return Err("热键只能有一个主按键".into());
                    }
                    key = Some(
                        if let Some(number) =
                            part.strip_prefix('F').filter(|number| !number.is_empty())
                        {
                            Key::Function(number.parse().map_err(|_| "无效功能键")?)
                        } else {
                            let mut chars = part.chars();
                            let c = chars.next().ok_or("缺少主按键")?;
                            if chars.next().is_some() {
                                return Err("无效主按键".into());
                            }
                            Key::Character(c)
                        },
                    );
                }
            }
        }
        hotkey.key = key.ok_or("缺少主按键")?;
        hotkey.validate()?;
        Ok(hotkey)
    }
}
pub const CONFIG_VERSION: u32 = 2;
pub const MIN_STROKE_WIDTH: u8 = 1;
pub const MAX_STROKE_WIDTH: u8 = 64;
pub const DEFAULT_STROKE_WIDTH: u8 = 3;
const STROKE_KEYS: [&str; 3] = ["rectangle_width", "arrow_width", "pen_width"];

pub fn parse_stroke_width(value: &str) -> Result<u8, &'static str> {
    value
        .parse::<u8>()
        .ok()
        .filter(|width| (MIN_STROKE_WIDTH..=MAX_STROKE_WIDTH).contains(width))
        .filter(|_| value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or("请输入 1–64 之间的整数线宽（px）。")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub theme: Theme,
    pub hotkey: Hotkey,
    pub show_hints: bool,
    pub first_tip_seen: bool,
    pub color: [u8; 3],
    pub stroke_widths: [u8; 3],
    pub text_size: u8,
    pub last_directory: Option<String>,
    pub extra: BTreeMap<String, String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            hotkey: Hotkey::default(),
            show_hints: true,
            first_tip_seen: false,
            color: [255, 59, 64],
            stroke_widths: [DEFAULT_STROKE_WIDTH; 3],
            text_size: 24,
            last_directory: None,
            extra: BTreeMap::new(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        self.hotkey.validate()?;
        if self
            .stroke_widths
            .iter()
            .any(|width| !(MIN_STROKE_WIDTH..=MAX_STROKE_WIDTH).contains(width))
            || !(12..=72).contains(&self.text_size)
        {
            return Err("工具默认线宽或字号超出支持范围".into());
        }
        if self.last_directory.as_ref().is_some_and(|path| {
            path.encode_utf16().count() > 32767 || path.chars().any(char::is_control)
        }) {
            return Err("保存目录无效或过长".into());
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        let mut text = format!(
            "version={CONFIG_VERSION}\ntheme={}\nhotkey={}\nshow_hints={}\nfirst_tip_seen={}\ncolor={:02X}{:02X}{:02X}\ntext_size={}\nlast_directory={}\n",
            match self.theme {
                Theme::System => "system",
                Theme::Light => "light",
                Theme::Dark => "dark",
            },
            self.hotkey.label(),
            self.show_hints,
            self.first_tip_seen,
            self.color[0],
            self.color[1],
            self.color[2],
            self.text_size,
            self.last_directory.as_deref().unwrap_or("")
        );
        use std::fmt::Write as _;
        for (key, value) in STROKE_KEYS.into_iter().zip(self.stroke_widths) {
            let _ = writeln!(text, "{key}={value}");
        }
        for (key, value) in &self.extra {
            let _ = writeln!(text, "{key}={value}");
        }
        Ok(text)
    }
    pub fn decode(text: &str) -> Result<(Self, bool), String> {
        if text.len() > 65536 {
            return Err("配置文件超过 64 KiB 上限".into());
        }
        let mut config = Self::default();
        let mut seen = BTreeSet::new();
        let version = text.lines().find_map(|line| line.strip_prefix("version="));
        let legacy = match version {
            Some("1") => true,
            Some("2") => false,
            _ => return Err("配置版本不受支持".into()),
        };
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').ok_or("配置行缺少等号")?;
            if !seen.insert(key) {
                return Err(format!("配置项重复：{key}"));
            }
            match key {
                "version" => {}
                "theme" => {
                    config.theme = match value {
                        "system" => Theme::System,
                        "light" => Theme::Light,
                        "dark" => Theme::Dark,
                        _ => return Err("主题配置无效".into()),
                    }
                }
                "hotkey" => config.hotkey = Hotkey::parse(value)?,
                "show_hints" => config.show_hints = value.parse().map_err(|_| "提示配置无效")?,
                "first_tip_seen" => {
                    config.first_tip_seen = value.parse().map_err(|_| "首次提示状态无效")?
                }
                "color" => {
                    if value.len() != 6 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err("默认颜色无效".into());
                    }
                    for i in 0..3 {
                        config.color[i] = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
                            .map_err(|_| "默认颜色无效")?;
                    }
                }
                "stroke_width" if legacy => {
                    let width = value.parse::<u8>().map_err(|_| "默认线宽无效")?;
                    if !(1..=12).contains(&width) {
                        return Err("旧版默认线宽超出支持范围".into());
                    }
                    config.stroke_widths = [width; 3];
                }
                "text_size" => config.text_size = value.parse().map_err(|_| "默认字号无效")?,
                "last_directory" => {
                    config.last_directory = (!value.is_empty()).then(|| value.to_string())
                }
                _ => {
                    if let Some(index) = STROKE_KEYS.iter().position(|name| *name == key) {
                        if !legacy {
                            config.stroke_widths[index] = parse_stroke_width(value)?;
                        }
                    } else {
                        config.extra.insert(key.to_owned(), value.to_owned());
                    }
                }
            }
        }
        config.validate()?;
        Ok((config, legacy))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_roundtrip_preserves_unicode_paths_and_equal_signs() {
        let config = Config {
            theme: Theme::Dark,
            hotkey: Hotkey {
                control: true,
                alt: false,
                shift: true,
                key: Key::Function(10),
            },
            last_directory: Some("F:\\截图资料\\a=b 项目".into()),
            ..Config::default()
        };
        assert_eq!(
            Config::decode(&config.encode().unwrap()).unwrap(),
            (config.clone(), false)
        );
        assert_eq!(config.hotkey.label(), "Ctrl+Shift+F10");
        assert_eq!(
            Config::decode("version=1\n").unwrap(),
            (Config::default(), true)
        );
    }
    #[test]
    fn migration_preserves_v1_values_and_v2_unknown_fields() {
        let (mut config, legacy) = Config::decode(
            "theme=dark\nversion=1\nstroke_width=8\ntext_size=32\nshow_hints=false\ncorner_radius=24\nfuture=a=b",
        ).unwrap();
        assert!(legacy);
        assert_eq!(config.stroke_widths, [8; 3]);
        assert_eq!(config.theme, Theme::Dark);
        assert_eq!(config.text_size, 32);
        assert!(!config.show_hints);
        config.stroke_widths = [1, 23, 64];
        let encoded = config.encode().unwrap();
        assert!(encoded.contains("corner_radius=24\n"));
        assert!(encoded.contains("future=a=b\n"));
        assert_eq!(Config::decode(&encoded).unwrap(), (config, false));
        assert_eq!(
            Config::decode("version=2\npen_width=13")
                .unwrap()
                .0
                .stroke_widths,
            [3, 3, 13]
        );
        for value in ["", "0", "65", "+3", "3.0", "x", "255"] {
            assert!(parse_stroke_width(value).is_err(), "{value}");
        }
    }
    #[test]
    fn invalid_preferences_never_partially_apply() {
        for text in [
            "theme=dark",
            "version=3",
            "version=1\nshow_hints=maybe",
            "version=1\ncolor=🚀xx",
            "version=1\ntext_size=255",
            "version=1\nstroke_width=0",
            "version=1\nstroke_width=13",
            "version=2\npen_width=65",
            "version=1\nhotkey=Shift+A",
            "version=1\nhotkey=Ctrl+Ctrl+A",
            "version=1\nhotkey=Ctrl+F25",
            "version=1\ntheme=light\ntheme=dark",
        ] {
            assert!(Config::decode(text).is_err(), "{text}");
        }
        assert!(Config::decode(&"x".repeat(65537)).is_err());
    }
}
