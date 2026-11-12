//! Speeds, key timing, typing tests, and shortcut rules, the same as the Mac app.

use crate::text_rules::is_line_break;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speed {
    pub title: &'static str,
    /// Seconds per key.
    pub interval: f64,
    pub detail: &'static str,
}

pub const SPEEDS: [Speed; 5] = [
    Speed { title: "Average typist · 50 WPM", interval: 0.24, detail: "240 ms per key. A typical typist, and the most reliable." },
    Speed { title: "Fast typist · 100 WPM", interval: 0.12, detail: "120 ms per key. The fastest 5% of typists." },
    Speed { title: "Record typist · 200 WPM", interval: 0.06, detail: "60 ms per key. World-record pace." },
    Speed { title: "Superhuman · 400 WPM", interval: 0.03, detail: "30 ms per key. Some remote sessions drop keys." },
    Speed { title: "Unrealistic · 600 WPM", interval: 0.02, detail: "20 ms per key. Expect dropped keys on slow connections." },
];

/// How long a key is held and the pause after it, in milliseconds. A damaged saved setting
/// (huge, infinite, or NaN) is clamped instead of crashing.
pub fn timing(interval: f64) -> (u32, u32) {
    let milliseconds = if interval.is_nan() { 240.0 } else { (interval * 1000.0).clamp(20.0, 580.0) };
    // Rounds half away from zero, as Swift's rounded() in VirtualKeyboard.timing.
    let total = milliseconds.round() as u32;
    // DECISION: keep the proven 80 ms hold when there is time; faster speeds hold for half the interval.
    let hold = (total / 2).min(80);
    (hold, total - hold)
}

// DECISION: remote apps can handle a line break after the keys that follow it (a new line
// appeared a few characters late, and a blank line was lost), so give each one time to land.
pub const LINE_BREAK_SETTLE_MS: u32 = 300;

pub fn settle_after(element: &str) -> u32 {
    if is_line_break(element) { LINE_BREAK_SETTLE_MS } else { 0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingTest {
    Short,
    Capitals,
    Symbols,
    SmartSymbols,
    Long,
}

impl TypingTest {
    pub const ALL: [Self; 5] = [Self::Short, Self::Capitals, Self::Symbols, Self::SmartSymbols, Self::Long];

    pub fn title(self) -> &'static str {
        match self {
            Self::Short => "Short",
            Self::Capitals => "Capitals",
            Self::Symbols => "Symbols",
            Self::SmartSymbols => "Smart symbols",
            Self::Long => "Long",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Short => "abc123",
            Self::Capitals => "AbC123!",
            Self::Symbols => "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~",
            Self::SmartSymbols => "• Bullet – en — em “double” ‘single’ … 3×4÷2 → ≤ ≥ ≠ © ® ™",
            Self::Long => concat!(
                "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs!\n",
                "Invoice #2048: $1,299.50 (incl. 8% tax) due 2026-10-31; ref A-17/B.\n",
                "\tIndented: test@example.com, C:\\Temp\\notes.txt, {braces} [brackets] <angles>.\n",
                "Sphinx of black quartz, judge my vow? 0123456789 ~ ` ^ | _ + = \" '",
            ),
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Self::Short => "Types abc123 at the slowest speed.",
            Self::Capitals => "Types AbC123! at the slowest speed, to check Shift.",
            Self::Symbols => "Types all 32 keyboard symbols at the slowest speed.",
            Self::SmartSymbols => "Types bullets, smart quotes, and dashes as plain keys.",
            Self::Long => "Types four lines with Tab and Return at your selected speed. Use a multi-line text field.",
        }
    }

    // DECISION: short tests use the slowest speed to isolate the method; the long test checks the selected speed.
    pub fn uses_selected_speed(self) -> bool {
        self == Self::Long
    }
}

/// Shortcut modifiers, with the bit values `RegisterHotKey` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(pub u32);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const ALT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const SHIFT: Self = Self(4);
    pub const WINDOWS: Self = Self(8);
    pub const ALL: Self = Self(0xF);

    pub fn contains_any(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A global shortcut: modifiers plus a Windows virtual-key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub modifiers: Modifiers,
    pub key: u32,
}

impl Shortcut {
    pub const ESCAPE: u32 = 0x1B;
    pub const F1: u32 = 0x70;
    pub const F24: u32 = 0x87;
    pub const BACKSLASH: u32 = 0xDC;
    pub const RIGHT_BRACKET: u32 = 0xDD;

    // DECISION: Control+Backslash by default. The hotkey consumes its key but not its modifiers,
    // which still reach the remote session: a lone Alt opens the Windows menu bar, and the
    // Windows key opens Start. Control alone is harmless there, and apps rarely use Ctrl+\.
    pub const STANDARD: Self = Self { modifiers: Modifiers::CONTROL, key: Self::BACKSLASH };
    // DECISION: Control+] for Copy Text from Screen, next to the typing shortcut.
    pub const COPY: Self = Self { modifiers: Modifiers::CONTROL, key: Self::RIGHT_BRACKET };

    pub fn is_function_key(self) -> bool {
        (Self::F1..=Self::F24).contains(&self.key)
    }

    /// Why this shortcut cannot be used, if it cannot.
    pub fn problem(self) -> Option<&'static str> {
        if self.key == Self::ESCAPE {
            Some("Esc stops typing, so it cannot start it.")
        } else if !self.is_function_key() && !self.modifiers.contains_any(Modifiers::CONTROL | Modifiers::ALT | Modifiers::WINDOWS) {
            Some("Add Control, Alt, or the Windows key, or use a function key such as F13.")
        } else {
            None
        }
    }

    /// A warning for shortcuts that work but can trigger something in the remote session.
    pub fn caution(self) -> Option<&'static str> {
        self.modifiers
            .contains_any(Modifiers::ALT | Modifiers::WINDOWS | Modifiers::SHIFT)
            .then_some("Alt, the Windows key, and Shift also reach the remote session and can trigger shortcuts there. Control alone is safest.")
    }
}

impl Shortcut {
    /// "Ctrl+\" and similar. `character` is what the key types on the current layout, if anything.
    pub fn title(self, character: Option<char>) -> String {
        let mut parts: Vec<String> =
            [(Modifiers::CONTROL, "Ctrl"), (Modifiers::ALT, "Alt"), (Modifiers::SHIFT, "Shift"), (Modifiers::WINDOWS, "Win")]
                .into_iter()
                .filter(|(m, _)| self.modifiers.contains_any(*m))
                .map(|(_, name)| name.to_owned())
                .collect();
        parts.push(match character {
            _ if self.is_function_key() => format!("F{}", self.key - Self::F1 + 1),
            Some(c) if c > ' ' && !c.is_control() => c.to_uppercase().collect(),
            _ => format!("Key {:02X}", self.key),
        });
        parts.join("+")
    }
}
