//! The key map from the target app's layout, typing with SendInput, and the stop watcher (the
//! Mac `Typer` and `StopWatcher`). Construction and sending stay together so a modifier is never
//! left held.

use crate::native::{self, KeyInput};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use typethru_core::settings::TypingMethod;
use typethru_core::text_rules::{TextFormat, is_line_break};
use typethru_core::typing::{settle_after, timing};

const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0D;
const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const SCAN_SHIFT: u32 = 0x2A;
const SCAN_CONTROL: u32 = 0x1D;
const KEYEVENTF_EXTENDEDKEY: u32 = 0x1;
const KEYEVENTF_KEYUP: u32 = 0x2;
const KEYEVENTF_UNICODE: u32 = 0x4;
const KEYEVENTF_SCANCODE: u32 = 0x8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub key: u16,
    pub shift: bool,
}

/// The key for each character on the keyboard layout of the app being typed into.
#[derive(Debug, Clone, Copy)]
pub struct KeyMap {
    layout: isize,
    shift_return: bool,
}

impl KeyMap {
    pub fn new(window: isize, shift_return: bool) -> Self {
        Self { layout: native::layout_of(window), shift_return }
    }

    /// The key and Shift for a character, or None when it needs another modifier or has no key.
    pub fn key(&self, element: &str) -> Option<KeyStroke> {
        if is_line_break(element) {
            return Some(KeyStroke { key: VK_RETURN, shift: self.shift_return });
        }
        if element == "\t" {
            return Some(KeyStroke { key: VK_TAB, shift: false });
        }
        let mut units = element.encode_utf16();
        let (Some(unit), None) = (units.next(), units.next()) else { return None };
        if element.chars().next().is_none_or(char::is_control) {
            return None;
        }
        // DECISION: Shift only, as on the Mac. AltGr (Control+Alt) would also reach the remote session.
        match native::key_scan(unit, self.layout)? {
            (key, 0) => Some(KeyStroke { key, shift: false }),
            (key, 1) => Some(KeyStroke { key, shift: true }),
            _ => None,
        }
    }

    pub fn typeable(&self, element: &str, method: TypingMethod) -> bool {
        if method == TypingMethod::Unicode {
            element.encode_utf16().count() <= 20
                && (element == "\t" || is_line_break(element) || element.chars().next().is_some_and(|c| !c.is_control()))
        } else {
            self.key(element).is_some()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Stopped { typed: usize, total: usize, reason: String },
}

/// Sends key presses with SendInput.
#[derive(Debug)]
pub struct Typer {
    map: KeyMap,
    method: TypingMethod,
    hold: Duration,
    gap: Duration,
}

impl Typer {
    pub fn new(map: KeyMap, method: TypingMethod, interval: f64) -> Self {
        let (hold, gap) = timing(interval);
        Self { map, method, hold: Duration::from_millis(hold.into()), gap: Duration::from_millis(gap.into()) }
    }

    pub fn type_text(&self, elements: &[String], formats: &BTreeMap<usize, Vec<TextFormat>>, target: isize) -> Outcome {
        let total = elements.len();
        let watcher = StopWatcher::start(target);
        let stopped = |typed, reason: &str| Outcome::Stopped { typed, total, reason: reason.to_owned() };
        let toggle = |index: usize| {
            formats.get(&index).is_none_or(|list| {
                list.iter()
                    .all(|format| self.map.key(&format.key().to_string()).is_some_and(|key| self.press(KeyStroke { shift: false, ..key }, true)))
            })
        };
        for (index, element) in elements.iter().enumerate() {
            if let Some(reason) = watcher.reason() {
                return stopped(index, &reason);
            }
            if !toggle(index) {
                return stopped(index, "bold or italic could not be switched");
            }
            let sent = if self.method == TypingMethod::Unicode && element != "\t" && !is_line_break(element) {
                self.send_unicode(element)
            } else {
                self.map.key(element).is_some_and(|stroke| self.press(stroke, false))
            };
            if !sent {
                return stopped(
                    index,
                    "Windows did not accept the key presses. A window running as administrator or the lock screen may be in front",
                );
            }
            thread::sleep(Duration::from_millis(settle_after(element).into()));
        }
        if toggle(total) { Outcome::Done } else { stopped(total, "bold or italic could not be switched off") }
    }

    fn press(&self, stroke: KeyStroke, control: bool) -> bool {
        let modifier = if control {
            Some((VK_CONTROL, SCAN_CONTROL))
        } else if stroke.shift {
            Some((VK_SHIFT, SCAN_SHIFT))
        } else {
            None
        };
        if let Some((key, scan)) = modifier {
            if !native::send(&[self.event(key, scan, true)]) {
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let scan = native::scan_code(stroke.key, self.map.layout);
        let mut ok = native::send(&[self.event(stroke.key, scan, true)]);
        if ok {
            thread::sleep(self.hold);
        }
        // Release in any case, so a key or modifier is never left held.
        ok &= native::send(&[self.event(stroke.key, scan, false)]);
        if let Some((key, scan)) = modifier {
            ok &= native::send(&[self.event(key, scan, false)]);
        }
        thread::sleep(self.gap);
        ok
    }

    fn event(&self, key: u16, scan: u32, down: bool) -> KeyInput {
        let mut input = KeyInput { key, scan: (scan & 0xFF) as u16, flags: if down { 0 } else { KEYEVENTF_KEYUP } };
        if self.method == TypingMethod::ScanCodes {
            // Remote desktop and VDI clients read hardware scan codes, not key codes.
            input.key = 0;
            input.flags |= KEYEVENTF_SCANCODE;
            if scan & 0xFF00 == 0xE000 {
                input.flags |= KEYEVENTF_EXTENDEDKEY;
            }
        }
        input
    }

    fn send_unicode(&self, element: &str) -> bool {
        for unit in element.encode_utf16() {
            let down = KeyInput { key: 0, scan: unit, flags: KEYEVENTF_UNICODE };
            if !native::send(&[down, KeyInput { flags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, ..down }]) {
                return false;
            }
        }
        thread::sleep(self.hold + self.gap);
        true
    }
}

/// Stops typing on Esc, a new mouse click, or a change of the front window. Polls every 10 ms.
struct StopWatcher {
    finished: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<&'static str>>>,
}

impl StopWatcher {
    fn start(target: isize) -> Self {
        const ESCAPE: i32 = 0x1B;
        const BUTTONS: [i32; 3] = [0x01, 0x02, 0x04];
        let finished = Arc::new(AtomicBool::new(false));
        let reason = Arc::new(Mutex::new(None));
        let (done, found) = (finished.clone(), reason.clone());
        thread::spawn(move || {
            // Buttons already down when typing starts (finishing a click on the target field) do not count.
            let mut was_down = BUTTONS.map(native::is_down);
            while !done.load(Ordering::Relaxed) {
                let down = BUTTONS.map(native::is_down);
                let clicked = down.iter().zip(was_down).any(|(&now, before)| now && !before);
                was_down = down;
                let stop = if native::is_down(ESCAPE) {
                    Some("Esc pressed")
                } else if clicked {
                    Some("the mouse was clicked")
                } else if native::foreground() != target {
                    Some("the front window changed")
                } else {
                    None
                };
                if stop.is_some() {
                    if let Ok(mut slot) = found.lock() {
                        *slot = stop;
                    }
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        Self { finished, reason }
    }

    fn reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|r| r.map(str::to_owned))
    }
}

impl Drop for StopWatcher {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Relaxed);
    }
}

/// Waits until the shortcut's modifiers (and key, when `key` is given) are up, so they do not mix
/// with the typed keys. False after 5 seconds.
pub fn wait_for_release(key: Option<u32>) -> bool {
    const MODIFIERS: [i32; 5] = [0x11, 0x12, 0x10, 0x5B, 0x5C];
    for _ in 0..500 {
        let held = MODIFIERS.into_iter().any(native::is_down) || key.is_some_and(|k| native::is_down(k as i32));
        if !held {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
