//! Saved settings, as JSON in `%APPDATA%\TypeThru\settings.json`, with the Mac app's options.

use crate::text_rules::MarkdownMode;
use crate::typing::{Modifiers, SPEEDS, Shortcut, Speed};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypingMethod {
    #[default]
    ScanCodes,
    VirtualKeys,
    Unicode,
}

impl TypingMethod {
    pub const ALL: [Self; 3] = [Self::ScanCodes, Self::VirtualKeys, Self::Unicode];

    pub fn title(self) -> &'static str {
        match self {
            Self::ScanCodes => "Hardware scan codes (recommended)",
            Self::VirtualKeys => "Virtual keys",
            Self::Unicode => "Unicode text",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Self::ScanCodes => "Key presses as a physical keyboard sends them. VDI and remote desktop clients read these.",
            Self::VirtualKeys => "Windows key codes. Try this if scan codes give the wrong characters in a local app.",
            Self::Unicode => "Characters instead of keys. Works in local apps for any character, but most remote sessions ignore it.",
        }
    }

    /// The name saved in settings.
    pub fn name(self) -> &'static str {
        match self {
            Self::ScanCodes => "ScanCodes",
            Self::VirtualKeys => "VirtualKeys",
            Self::Unicode => "Unicode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub method: TypingMethod,
    pub speed_index: usize,
    pub replace_symbols: bool,
    // DECISION: on by default: Shift+Return makes a new line in chat apps and word processors
    // instead of sending. Spreadsheets move up a cell, so the panel can turn it off.
    pub shift_return: bool,
    pub markdown: MarkdownMode,
    pub copy_from_screen: bool,
    pub check_updates: bool,
    shortcut: Shortcut,
    pub launch_at_login_asked: bool,
    pub install_asked: bool,
    pub last_run_version: Option<String>,
    /// Seconds since 1970 (UTC).
    pub last_update_check: Option<i64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            method: TypingMethod::ScanCodes,
            speed_index: 0,
            replace_symbols: true,
            shift_return: true,
            markdown: MarkdownMode::Format,
            copy_from_screen: true,
            check_updates: true,
            shortcut: Shortcut::STANDARD,
            launch_at_login_asked: false,
            install_asked: false,
            last_run_version: None,
            last_update_check: None,
        }
    }
}

/// An enum saved by name; an index is accepted too.
fn choice<T: Copy>(value: Option<&Value>, all: &[T], name: impl Fn(T) -> &'static str) -> Option<T> {
    match value? {
        Value::String(s) => all.iter().copied().find(|&v| name(v) == s),
        Value::Number(n) => all.get(usize::try_from(n.as_u64()?).ok()?).copied(),
        _ => None,
    }
}

impl Settings {
    pub fn speed(&self) -> Speed {
        SPEEDS[self.speed_index.min(SPEEDS.len() - 1)]
    }

    pub fn shortcut(&self) -> Shortcut {
        self.shortcut
    }

    /// Saves `shortcut`, or the standard one when it cannot be used.
    pub fn set_shortcut(&mut self, shortcut: Shortcut) {
        let usable = shortcut.key > 0 && shortcut.key < 0xFF && shortcut.modifiers.0 & !Modifiers::ALL.0 == 0 && shortcut.problem().is_none();
        self.shortcut = if usable { shortcut } else { Shortcut::STANDARD };
    }

    /// Reads saved settings. Each value is checked on its own, so a damaged or missing value gets
    /// its default and the rest are kept, instead of a crash.
    pub fn parse(json: Option<&str>) -> Self {
        let mut settings = Self::default();
        let Some(Ok(Value::Object(saved))) = json.map(serde_json::from_str::<Value>) else { return settings };
        let flag = |name: &str, default: bool| saved.get(name).and_then(Value::as_bool).unwrap_or(default);
        let number = |name: &str| saved.get(name).and_then(Value::as_i64);

        settings.method = choice(saved.get("Method"), &TypingMethod::ALL, TypingMethod::name).unwrap_or_default();
        settings.markdown = choice(saved.get("Markdown"), &MarkdownMode::ALL, MarkdownMode::name).unwrap_or_default();
        settings.speed_index = number("SpeedIndex").map_or(0, |i| i.clamp(0, SPEEDS.len() as i64 - 1) as usize);
        settings.replace_symbols = flag("ReplaceSymbols", true);
        settings.shift_return = flag("ShiftReturn", true);
        settings.copy_from_screen = flag("CopyFromScreen", true);
        settings.check_updates = flag("CheckUpdates", true);
        settings.launch_at_login_asked = flag("LaunchAtLoginAsked", false);
        settings.install_asked = flag("InstallAsked", false);
        settings.last_run_version = saved.get("LastRunVersion").and_then(Value::as_str).map(str::to_owned);
        settings.last_update_check = number("LastUpdateCheck");
        if let (Some(modifiers), Some(key)) = (number("ShortcutModifiers"), number("ShortcutKey"))
            && let (Ok(modifiers), Ok(key)) = (u32::try_from(modifiers), u32::try_from(key))
        {
            settings.set_shortcut(Shortcut { modifiers: Modifiers(modifiers), key });
        }
        settings
    }

    pub fn serialize(&self) -> String {
        let mut map = Map::new();
        map.insert("Method".into(), json!(self.method.name()));
        map.insert("SpeedIndex".into(), json!(self.speed_index));
        map.insert("ReplaceSymbols".into(), json!(self.replace_symbols));
        map.insert("ShiftReturn".into(), json!(self.shift_return));
        map.insert("Markdown".into(), json!(self.markdown.name()));
        map.insert("CopyFromScreen".into(), json!(self.copy_from_screen));
        map.insert("CheckUpdates".into(), json!(self.check_updates));
        map.insert("ShortcutModifiers".into(), json!(self.shortcut.modifiers.0));
        map.insert("ShortcutKey".into(), json!(self.shortcut.key));
        map.insert("LaunchAtLoginAsked".into(), json!(self.launch_at_login_asked));
        map.insert("InstallAsked".into(), json!(self.install_asked));
        map.insert("LastRunVersion".into(), json!(self.last_run_version));
        map.insert("LastUpdateCheck".into(), json!(self.last_update_check));
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }
}
