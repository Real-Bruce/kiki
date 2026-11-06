//! Versioned, bounded preferences without platform resources or a serialization runtime.
use std::collections::BTreeSet;
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub theme: Theme,
    pub hotkey: Hotkey,
    pub show_hints: bool,
    pub first_tip_seen: bool,
    pub color: [u8; 3],
    pub stroke_width: u8,
    pub text_size: u8,
    pub last_directory: Option<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            hotkey: Hotkey::default(),
            show_hints: true,
            first_tip_seen: false,
            color: [255, 59, 64],
            stroke_width: 3,
            text_size: 24,
            last_directory: None,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        self.hotkey.validate()?;
        if !(1..=12).contains(&self.stroke_width) || !(12..=72).contains(&self.text_size) {
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
        Ok(format!(
            "version=1\ntheme={}\nhotkey={}\nshow_hints={}\nfirst_tip_seen={}\ncolor={:02X}{:02X}{:02X}\nstroke_width={}\ntext_size={}\nlast_directory={}\n",
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
            self.stroke_width,
            self.text_size,
            self.last_directory.as_deref().unwrap_or("")
        ))
    }
    pub fn decode(text: &str) -> Result<Self, String> {
        if text.len() > 65536 {
            return Err("配置文件超过 64 KiB 上限".into());
        }
        let mut config = Self::default();
        let mut seen = BTreeSet::new();
        let mut version = None;
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').ok_or("配置行缺少等号")?;
            if !seen.insert(key) {
                return Err(format!("配置项重复：{key}"));
            }
            match key {
                "version" => version = Some(value),
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
                "stroke_width" => {
                    config.stroke_width = value.parse().map_err(|_| "默认线宽无效")?
                }
                "text_size" => config.text_size = value.parse().map_err(|_| "默认字号无效")?,
                "last_directory" => {
                    config.last_directory = (!value.is_empty()).then(|| value.to_string())
                }
                _ => {}
            }
        }
        if version != Some("1") {
            return Err("配置版本不受支持".into());
        }
        config.validate()?;
        Ok(config)
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
        assert_eq!(Config::decode(&config.encode().unwrap()).unwrap(), config);
        assert_eq!(config.hotkey.label(), "Ctrl+Shift+F10");
        assert_eq!(Config::decode("version=1\n").unwrap(), Config::default());
    }
    #[test]
    fn invalid_preferences_never_partially_apply() {
        for text in [
            "theme=dark",
            "version=2",
            "version=1\nshow_hints=maybe",
            "version=1\ncolor=🚀xx",
            "version=1\ntext_size=255",
            "version=1\nstroke_width=0",
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
