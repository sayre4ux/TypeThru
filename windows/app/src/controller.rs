//! The app on Windows (the Mac `AppDelegate`): notification area icon, shortcuts, the glass panel,
//! typing, Copy Text from Screen, updates, What's New, and the first-run questions.

use crate::keyboard::{self, KeyMap, Outcome, Typer};
use crate::native::{self, HotKeys, SingleInstance};
use crate::screen_copy::{self, Area};
use crate::ui::{NoteLine, NotesWindow, Selector, SettingsWindow, Toast};
use crate::{Panel, install, panel, storage, updater};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, SharedString, StyledText, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use typethru_core::notes::{parse_changelog, parse_inline, since};
use typethru_core::settings::{Settings, TypingMethod};
use typethru_core::text_rules::{self, MarkdownMode, elements, without_trailing_line_breaks};
use typethru_core::typing::{SPEEDS, Shortcut, TypingTest};
use typethru_core::update_feed::{ChangelogEntry, Release, display, is_newer};

pub(crate) const TYPE_SHORTCUT: i32 = 1;
pub(crate) const COPY_SHORTCUT: i32 = 2;
const PANEL_TITLE: &str = "TypeThru";
const TOAST_TITLE: &str = "TypeThru notice";
const SELECTOR_TITLE: &str = "TypeThru selection";
const FONT: &str = "Segoe UI Variable Text";
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

pub(crate) struct App {
    pub(crate) panel: Panel,
    pub(crate) settings_window: RefCell<Option<SettingsWindow>>,
    notes_window: RefCell<Option<NotesWindow>>,
    toast: RefCell<Option<Toast>>,
    toast_timer: Timer,
    selector: Selector,
    selecting: Cell<bool>,
    selection_start: Cell<Option<(i32, i32)>>,
    pub(crate) settings: RefCell<Settings>,
    pub(crate) hotkeys: HotKeys,
    tray: RefCell<Option<TrayIcon>>,
    /// When the panel last closed because it lost focus: a click on the icon that caused it must not reopen it.
    hidden_at: Cell<Option<Instant>>,
    glass_applied: Cell<bool>,
    /// Problems with the shortcuts, shown in the panel until they are solved.
    shortcut_notice: RefCell<String>,
    busy: Cell<bool>,
    copying: Cell<bool>,
    checking: Cell<bool>,
    pub(crate) update: RefCell<Option<Release>>,
    pub(crate) installing: Cell<bool>,
    update_status: RefCell<String>,
    update_timer: Timer,
    pub(crate) recording: Cell<bool>,
    last_attempt: RefCell<String>,
    last_copy: RefCell<String>,
    _single: SingleInstance,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// Runs `work` with the app on the main thread, from any thread.
pub(crate) fn later(work: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = APP.with(|slot| slot.borrow().clone()) {
            work(&app);
        }
    });
}

/// A message box on a thread of its own, so the app keeps running while it is open.
pub(crate) fn alert(title: &str, text: &str) {
    let (title, text) = (title.to_owned(), text.to_owned());
    std::thread::spawn(move || native::message(&title, &text));
}

/// A Yes/No question; `then` runs on the main thread with the answer.
pub(crate) fn ask(title: &str, text: &str, yes: &str, no: &str, then: impl FnOnce(&Rc<App>, bool) + Send + 'static) {
    let (title, text) = (title.to_owned(), format!("{text}\n\nYes: {yes}\nNo: {no}"));
    std::thread::spawn(move || {
        let answer = native::ask(&title, &text);
        later(move |app| then(app, answer));
    });
}

pub(crate) fn title(shortcut: Shortcut) -> String {
    shortcut.title(native::key_character(shortcut.key))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn run() {
    let Some(single) = SingleInstance::acquire() else {
        native::message("TypeThru", "TypeThru is already running. Look for its icon in the notification area, next to the clock.");
        return;
    };
    if let Err(error) = start(single) {
        native::message("TypeThru", &format!("TypeThru could not start: {error}"));
        return;
    }
    let _ = slint::run_event_loop_until_quit();
    // Free the shortcuts and the single-copy lock before the process ends, so an updated copy can start.
    APP.with(|slot| slot.borrow_mut().take());
}

fn start(single: SingleInstance) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut backend = slint::BackendSelector::new().backend_name("winit".into()).with_winit_window_attributes_hook(|attributes| {
        use winit::platform::windows::WindowAttributesExtWindows;
        match attributes.title.as_str() {
            // See-through for the glass, and no taskbar button: the panel belongs to the notification area icon.
            PANEL_TITLE | SELECTOR_TITLE => attributes.with_transparent(true).with_skip_taskbar(true),
            // The notice never takes focus from the app in front.
            TOAST_TITLE => attributes.with_transparent(true).with_skip_taskbar(true).with_active(false),
            _ => attributes,
        }
    });
    // DECISION: --renderer femtovg|software, to compare Slint's renderers while the glass is tested on PCs.
    if let Some(renderer) = args.iter().position(|a| a == "--renderer").and_then(|i| args.get(i + 1)) {
        backend = backend.renderer_name(renderer.clone());
    }
    backend.select()?;
    updater::clean_up();

    let settings = storage::load();
    let panel = panel::create(&settings, native::key_character)?;
    panel.set_font(FONT.into());
    let selector = Selector::new()?;
    selector.set_font(FONT.into());
    let hotkeys = HotKeys::new(|id| later(move |app| app.shortcut_pressed(id)))?;
    let app = Rc::new(App {
        panel,
        settings_window: RefCell::new(None),
        notes_window: RefCell::new(None),
        toast: RefCell::new(None),
        toast_timer: Timer::default(),
        selector,
        selecting: Cell::new(false),
        selection_start: Cell::new(None),
        settings: RefCell::new(settings),
        hotkeys,
        tray: RefCell::new(None),
        hidden_at: Cell::new(None),
        glass_applied: Cell::new(false),
        shortcut_notice: RefCell::new(String::new()),
        busy: Cell::new(false),
        copying: Cell::new(false),
        checking: Cell::new(false),
        update: RefCell::new(None),
        installing: Cell::new(false),
        update_status: RefCell::new(String::new()),
        update_timer: Timer::default(),
        recording: Cell::new(false),
        last_attempt: RefCell::new("No attempt in this session.".into()),
        last_copy: RefCell::new("No text copied from the screen in this session.".into()),
        _single: single,
    });
    app.connect_panel();
    app.connect_selector();
    if !app.register_shortcuts() {
        let shortcut = title(app.settings.borrow().shortcut());
        alert(&format!("Could not use {shortcut}"), "Another app may use this shortcut. Choose a different one in TypeThru Settings.");
    }
    APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));

    // The icon needs the running message loop, so it is added once the loop starts.
    Timer::single_shot(Duration::ZERO, || {
        later(|app| {
            if let Err(error) = app.add_icon() {
                alert("TypeThru", &format!("Could not add the notification area icon: {error}"));
            }
            app.onboarding();
            app.update_timer.start(TimerMode::Repeated, Duration::from_secs(3600), || later(|app| app.check_for_updates(false)));
            Timer::single_shot(Duration::from_secs(10), || later(|app| app.check_for_updates(false)));
            app.show_whats_new_after_update();
        });
    });
    Ok(())
}

impl App {
    fn connect_panel(self: &Rc<Self>) {
        let panel = &self.panel;
        let weak = Rc::downgrade(self);
        let with = move |work: fn(&Rc<App>)| {
            let weak = weak.clone();
            move || {
                if let Some(app) = weak.upgrade() {
                    work(&app);
                }
            }
        };
        panel.on_type_clipboard(with(|app| app.start_typing(true, None)));
        panel.on_copy_from_screen_clicked(with(|app| app.copy_from_screen()));
        panel.on_open_settings(with(|app| {
            app.hide_panel();
            app.open_settings();
        }));
        panel.on_whats_new(with(|app| app.show_update_notes()));
        panel.on_install_update(with(|app| app.install_update()));
        panel.on_quit(with(|app| app.quit()));
        panel.on_dismiss(with(|app| app.hide_panel()));
        let weak = Rc::downgrade(self);
        panel.on_run_test(move |index| {
            if let Some(app) = weak.upgrade()
                && let Some(test) = usize::try_from(index).ok().and_then(|i| TypingTest::ALL.get(i))
            {
                app.start_typing(true, Some(*test));
            }
        });
        let weak = Rc::downgrade(self);
        panel.on_speed_changed(move |index| {
            if let Some(app) = weak.upgrade() {
                app.change(|s| s.speed_index = usize::try_from(index).unwrap_or(0));
            }
        });
        let weak = Rc::downgrade(self);
        panel.on_shift_return_changed(move |on| {
            if let Some(app) = weak.upgrade() {
                app.change(|s| s.shift_return = on);
            }
        });
        // Close when another window comes to the front, as a flyout does.
        let weak = Rc::downgrade(self);
        panel.window().on_winit_window_event(move |_, event| {
            if let winit::event::WindowEvent::Focused(false) = event {
                let weak = weak.clone();
                // The speed menu is a window of its own: wait a moment, then close only if focus left TypeThru.
                Timer::single_shot(Duration::from_millis(80), move || {
                    if let Some(app) = weak.upgrade()
                        && !native::foreground_is_ours()
                    {
                        app.hide_panel();
                    }
                });
            }
            EventResult::Propagate
        });
    }

    /// Saves a changed setting and shows it in every open window.
    pub(crate) fn change(&self, edit: impl FnOnce(&mut Settings)) {
        edit(&mut self.settings.borrow_mut());
        self.save();
        panel::show_settings(&self.panel, &self.settings.borrow(), native::key_character);
        self.fill_settings();
    }

    pub(crate) fn save(&self) {
        if let Err(error) = storage::save(&self.settings.borrow()) {
            alert("Could not save the settings", &error.to_string());
        }
    }

    /// Registers both shortcuts; false when Windows refuses the typing shortcut.
    pub(crate) fn register_shortcuts(&self) -> bool {
        let shortcut = self.settings.borrow().shortcut();
        let mut problems = Vec::new();
        let mut typing = true;
        for (id, shortcut, what) in [(TYPE_SHORTCUT, shortcut, "Type Clipboard"), (COPY_SHORTCUT, Shortcut::COPY, "Copy Text from Screen")] {
            if id == COPY_SHORTCUT && !self.settings.borrow().copy_from_screen {
                self.hotkeys.unregister(id);
            } else if !self.hotkeys.register(id, shortcut) {
                typing &= id != TYPE_SHORTCUT;
                problems.push(format!("Could not use {} for {what}: another app may use it. Choose another in Settings.", title(shortcut)));
            }
        }
        *self.shortcut_notice.borrow_mut() = problems.join("\n");
        typing
    }

    fn shortcut_pressed(self: &Rc<Self>, id: i32) {
        match id {
            TYPE_SHORTCUT => self.start_typing(false, None),
            COPY_SHORTCUT => self.copy_from_screen(),
            _ => {}
        }
    }

    fn add_icon(self: &Rc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let items = [
            MenuItem::new("Type Clipboard", true, None),
            MenuItem::new("Copy Text from Screen", true, None),
            MenuItem::new("Settings", true, None),
            MenuItem::new("What's New", true, None),
            MenuItem::new("Quit TypeThru", true, None),
        ];
        let menu = Menu::new();
        menu.append_items(&[&items[0], &items[1], &items[2], &items[3], &PredefinedMenuItem::separator(), &items[4]])?;
        let ids: Vec<_> = items.iter().map(|item| item.id().clone()).collect();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let Some(index) = ids.iter().position(|id| *id == event.id) else { return };
            later(move |app| match index {
                0 => app.start_typing(true, None),
                1 => app.copy_from_screen(),
                2 => app.open_settings(),
                3 => app.show_whats_new(),
                _ => app.quit(),
            });
        }));
        TrayIconEvent::set_event_handler(Some(|event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                let anchor = (rect.position.x as i32 + rect.size.width as i32 / 2, rect.position.y as i32);
                later(move |app| app.toggle_panel(anchor));
            }
        }));
        let icon = tray_icon::Icon::from_resource(1, None)?;
        let tray =
            TrayIconBuilder::new().with_icon(icon).with_tooltip("TypeThru").with_menu(Box::new(menu)).with_menu_on_left_click(false).build()?;
        *self.tray.borrow_mut() = Some(tray);
        Ok(())
    }

    // MARK: the panel

    fn toggle_panel(self: &Rc<Self>, anchor: (i32, i32)) {
        let just_hidden = self.hidden_at.get().is_some_and(|at| at.elapsed() < Duration::from_millis(400));
        if self.panel.window().is_visible() || just_hidden {
            self.hide_panel();
        } else {
            self.show_panel(Some(anchor));
        }
    }

    fn hide_panel(&self) {
        if self.panel.window().is_visible() {
            self.hidden_at.set(Some(Instant::now()));
            let _ = self.panel.hide();
        }
    }

    fn refresh_panel(&self) {
        let panel = &self.panel;
        panel.set_notice(self.shortcut_notice.borrow().as_str().into());
        panel.set_busy(self.busy.get());
        panel.set_status(if self.busy.get() { "Typing…" } else { "Ready" }.into());
        // Windows needs no helper or permission to send keys, so TypeThru is always ready.
        panel.set_ready(true);
        let update = self.update.borrow();
        panel.set_update_available(update.is_some());
        panel.set_installing(self.installing.get());
        let text = match &*update {
            Some(_) if self.installing.get() => self.update_status.borrow().clone(),
            Some(release) => format!("Version {} · you have {}", display(&release.version), display(&updater::current_version())),
            None => String::new(),
        };
        panel.set_update_text(text.into());
        panel::show_settings(panel, &self.settings.borrow(), native::key_character);
    }

    /// Shows the panel above the icon (`anchor`), or at the bottom right of the screen with the pointer.
    fn show_panel(self: &Rc<Self>, anchor: Option<(i32, i32)>) {
        self.refresh_panel();
        if !self.panel.window().is_visible() {
            // Off screen until the size is known, then moved into place below.
            self.panel.window().set_position(PhysicalPosition::new(-32000, -32000));
            if self.panel.show().is_err() {
                return;
            }
        }
        let weak = Rc::downgrade(self);
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(app) = weak.upgrade() {
                app.place_panel(anchor);
            }
        });
    }

    fn place_panel(&self, anchor: Option<(i32, i32)>) {
        let window = self.panel.window();
        let size = window.size();
        let (width, height) = (size.width as i32, size.height as i32);
        let (x, y) = anchor.unwrap_or_else(native::cursor);
        let area = native::work_area_at(x, y);
        let margin = (12.0 * window.scale_factor()) as i32;
        let (left, top) = match anchor {
            // Centered on the icon, above it (or below it when the taskbar is at the top), inside the screen.
            Some((x, y)) => {
                let left = (x - width / 2).clamp(area.left + margin, (area.right - width - margin).max(area.left + margin));
                let top = if y > (area.top + area.bottom) / 2 { area.bottom - height - margin } else { area.top + margin };
                (left, top)
            }
            None => (area.right - width - margin, area.bottom - height - margin),
        };
        window.set_position(PhysicalPosition::new(left, top));
        self.apply_glass();
        window.with_winit_window(|w| w.focus_window());
    }

    /// Windows' own frosted glass (Acrylic) behind the panel; an opaque background if it is not available.
    fn apply_glass(&self) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = self.panel.window().window_handle();
        let Ok(raw) = handle.window_handle().map(|h| h.as_raw()) else { return };
        if let RawWindowHandle::Win32(win32) = raw {
            native::style_window(win32.hwnd.get(), self.panel.get_dark());
        }
        if !self.glass_applied.replace(true) && window_vibrancy::apply_acrylic(&handle, None).is_err() {
            self.panel.set_solid(true);
        }
    }

    /// A short message above the notification area that goes away by itself.
    pub(crate) fn toast(self: &Rc<Self>, text: &str) {
        // A new window each time: only a new window opens without taking focus.
        let Ok(toast) = Toast::new() else { return };
        toast.set_font(FONT.into());
        toast.set_message(text.into());
        toast.window().set_position(PhysicalPosition::new(-32000, -32000));
        if toast.show().is_err() {
            return;
        }
        let window = toast.as_weak();
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(toast) = window.upgrade() {
                let size = toast.window().size();
                let (x, y) = native::cursor();
                let area = native::work_area_at(x, y);
                let margin = (16.0 * toast.window().scale_factor()) as i32;
                toast
                    .window()
                    .set_position(PhysicalPosition::new(area.right - size.width as i32 - margin, area.bottom - size.height as i32 - margin));
            }
        });
        if let Some(previous) = self.toast.replace(Some(toast)) {
            let _ = previous.hide();
        }
        let weak = Rc::downgrade(self);
        self.toast_timer.start(TimerMode::SingleShot, Duration::from_millis(1800), move || {
            if let Some(app) = weak.upgrade()
                && let Some(toast) = app.toast.take()
            {
                let _ = toast.hide();
            }
        });
    }

    // MARK: typing

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        if let Some(tray) = &*self.tray.borrow() {
            let _ = tray.set_tooltip(Some(if busy { "TypeThru: typing… Esc stops" } else { "TypeThru" }));
        }
        self.refresh_panel();
        self.fill_settings();
    }

    /// From the shortcut, types into the window in front. From the panel or menu, first gives the
    /// user a moment to click the target field (three seconds for tests), as on the Mac.
    fn start_typing(self: &Rc<Self>, from_panel: bool, test: Option<TypingTest>) {
        if self.busy.get() {
            native::beep();
            return;
        }
        if !from_panel {
            self.trigger(native::foreground(), false, test);
            return;
        }
        self.hide_panel();
        self.set_busy(true);
        let weak = Rc::downgrade(self);
        Timer::single_shot(Duration::from_millis(if test.is_none() { 1000 } else { 3000 }), move || {
            if let Some(app) = weak.upgrade() {
                app.set_busy(false);
                app.trigger(native::foreground(), true, test);
            }
        });
    }

    fn note(&self, line: &str) {
        let mut attempt = self.last_attempt.borrow_mut();
        attempt.push('\n');
        attempt.push_str(line);
    }

    fn trigger(self: &Rc<Self>, window: isize, from_panel: bool, test: Option<TypingTest>) {
        let settings = self.settings.borrow().clone();
        let method = settings.method;
        let trigger = match test {
            Some(test) => format!("{} test", test.title()),
            None if from_panel => "panel".into(),
            None => "shortcut".into(),
        };
        *self.last_attempt.borrow_mut() = format!("Attempt: {}\nTrigger: {trigger}\nMethod: {}", native::local_time(), method.title());
        let copied = test.map(|t| t.text().to_owned()).or_else(native::clipboard_text).unwrap_or_default();
        let original = without_trailing_line_breaks(&copied);
        if original.is_empty() {
            self.note("Not started: clipboard has no text.");
            alert("Nothing typed", "The clipboard has no text. You can try a typing test from the TypeThru panel.");
            return;
        }
        if original.len() < copied.len() {
            self.note("Left out the line break at the end.");
        }
        let map = KeyMap::new(window, settings.shift_return);
        if settings.shift_return {
            self.note("Line breaks: Shift+Enter.");
        }
        let typeable = |e: &str| map.typeable(e, method);
        // The smart symbols test exists to show the replacements, so it always replaces.
        let (mut text, replaced) = if (settings.replace_symbols || test == Some(TypingTest::SmartSymbols)) && method != TypingMethod::Unicode {
            text_rules::replace_untypable(&original, typeable)
        } else {
            (original, 0)
        };
        if replaced > 0 {
            self.note(&format!("Replaced {replaced} symbols that have no key."));
        }
        let mut formats = BTreeMap::new();
        if settings.markdown != MarkdownMode::Keep && test.is_none() {
            let parsed = text_rules::markdown(&text);
            if parsed.spans > 0 {
                text = parsed.text;
                if settings.markdown == MarkdownMode::Format && map.key("b").is_some() && map.key("i").is_some() {
                    formats = parsed.formats;
                }
                let how = if formats.is_empty() { "marks removed." } else { "typed as bold or italic." };
                self.note(&format!("Markdown: {} marked spans, {how}", parsed.spans));
            }
        }
        if !text_rules::unsupported(&text, typeable).is_empty() {
            self.note("Not started: unsupported characters.");
            alert(
                "Nothing typed",
                if method == TypingMethod::Unicode {
                    "The text contains a character too long for a Unicode key event."
                } else {
                    "Some characters have no key on the current keyboard layout. Try Unicode text in Settings, or check your keyboard layout."
                },
            );
            return;
        }
        let process = native::window_process(window);
        if window == 0 || process == 0 || process == native::current_process() {
            self.note("Not started: no target window.");
            alert("Nothing typed", "Click the text field you want to type into, then try again.");
            return;
        }
        if native::is_elevated(Some(process)) && !native::is_elevated(None) {
            self.note("Not started: the target runs as administrator.");
            alert(
                "Nothing typed",
                "Windows does not let TypeThru type into an app running as administrator. Open that app normally, or run TypeThru as administrator.",
            );
            return;
        }

        self.set_busy(true);
        let interval = match test {
            Some(test) if !test.uses_selected_speed() => SPEEDS[0].interval,
            _ => settings.speed().interval,
        };
        let elements: Vec<String> = elements(&text).into_iter().map(str::to_owned).collect();
        self.note(&format!("Speed: {} ms per character\nWaiting to start.", (interval * 1000.0).round()));
        let key = (!from_panel).then_some(settings.shortcut().key);
        std::thread::spawn(move || {
            let total = elements.len();
            let released = keyboard::wait_for_release(key);
            if released {
                std::thread::sleep(Duration::from_millis(if from_panel { 150 } else { 750 }));
            }
            let stopped = |reason: &str| Outcome::Stopped { typed: 0, total, reason: reason.into() };
            let outcome = if !released {
                stopped("the shortcut or modifier keys were not released")
            } else if native::foreground() != window {
                stopped("the target window changed before typing")
            } else {
                Typer::new(map, method, interval).type_text(&elements, &formats, window)
            };
            later(move |app| app.typing_finished(outcome, total));
        });
    }

    fn typing_finished(self: &Rc<Self>, outcome: Outcome, total: usize) {
        self.set_busy(false);
        match outcome {
            Outcome::Done => self.note(&format!("Sent key presses for {total} characters. Remote acceptance is unverified.")),
            Outcome::Stopped { typed, total, reason } => {
                self.note(&format!("Stopped after sending {typed} of {total} characters: {reason}."));
                native::beep();
                alert("Typing stopped", &format!("Stopped after sending {typed} of {total} characters because {reason}."));
            }
        }
    }

    // MARK: Copy Text from Screen

    fn connect_selector(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.selector.on_started(move || {
            if let Some(app) = weak.upgrade() {
                // Screen pixels from the pointer itself, so the area is right on screens of any scale.
                app.selection_start.set(Some(native::cursor()));
            }
        });
        let weak = Rc::downgrade(self);
        self.selector.on_finished(move || {
            if let Some(app) = weak.upgrade() {
                app.selection_finished();
            }
        });
        let weak = Rc::downgrade(self);
        self.selector.on_cancel(move || {
            if let Some(app) = weak.upgrade() {
                app.end_selection();
            }
        });
        let weak = Rc::downgrade(self);
        self.selector.window().on_winit_window_event(move |_, event| {
            if let winit::event::WindowEvent::Focused(false) = event
                && let Some(app) = weak.upgrade()
            {
                app.end_selection();
            }
            EventResult::Propagate
        });
    }

    pub(crate) fn copy_from_screen(self: &Rc<Self>) {
        if !self.settings.borrow().copy_from_screen || self.busy.get() || self.copying.get() {
            native::beep();
            return;
        }
        self.hide_panel();
        self.copying.set(true);
        self.selecting.set(true);
        self.selection_start.set(None);
        let (left, top, width, height) = native::virtual_screen();
        let window = self.selector.window();
        window.set_position(PhysicalPosition::new(left, top));
        window.set_size(PhysicalSize::new(width.max(1) as u32, height.max(1) as u32));
        if self.selector.show().is_err() {
            self.end_selection();
            return;
        }
        window.with_winit_window(|w| w.focus_window());
    }

    /// Closes the selection without copying.
    fn end_selection(&self) {
        if self.selecting.replace(false) {
            let _ = self.selector.hide();
            self.copying.set(false);
        }
    }

    fn selection_finished(self: &Rc<Self>) {
        if !self.selecting.replace(false) {
            return;
        }
        let _ = self.selector.hide();
        let (Some((x1, y1)), (x2, y2)) = (self.selection_start.take(), native::cursor()) else {
            self.copying.set(false);
            return;
        };
        let area = Area { left: x1.min(x2), top: y1.min(y2), width: (x1 - x2).abs(), height: (y1 - y2).abs() };
        if area.width < 4 || area.height < 4 {
            self.copying.set(false);
            return;
        }
        let time = native::local_time();
        std::thread::spawn(move || {
            // Let the selection window disappear before the picture is taken.
            std::thread::sleep(Duration::from_millis(150));
            let result = screen_copy::read(area);
            later(move |app| app.copy_finished(result, &time));
        });
    }

    fn copy_finished(self: &Rc<Self>, result: Result<String, String>, time: &str) {
        self.copying.set(false);
        match result {
            Ok(text) if text.is_empty() => {
                *self.last_copy.borrow_mut() = format!("Last copy from the screen: {time}, no text found.");
                self.toast("No text found in the selection");
            }
            Ok(text) if screen_copy::set_clipboard(self.hotkeys.handle(), &text) => {
                let lines = text.split('\n').count();
                *self.last_copy.borrow_mut() = format!("Last copy from the screen: {time}, {lines} lines, {} characters.", elements(&text).len());
                self.toast(&if lines == 1 { "Copied 1 line of text".to_owned() } else { format!("Copied {lines} lines of text") });
            }
            Ok(_) => {
                *self.last_copy.borrow_mut() = format!("Last copy from the screen: {time}, failed.");
                alert("Could not copy text", "Another app is using the clipboard. Try again.");
            }
            Err(reason) => {
                *self.last_copy.borrow_mut() = format!("Last copy from the screen: {time}, failed.");
                alert("Could not copy text", &reason);
            }
        }
    }

    // MARK: updates

    pub(crate) fn check_for_updates(self: &Rc<Self>, manual: bool) {
        let (enabled, last) = {
            let settings = self.settings.borrow();
            (settings.check_updates, settings.last_update_check)
        };
        if (!manual && (!enabled || last.is_some_and(|last| (now() - last).abs() < 23 * 3600))) || self.checking.replace(true) {
            return;
        }
        *self.update_status.borrow_mut() = "Checking…".into();
        std::thread::spawn(move || {
            let result = updater::check();
            later(move |app| app.update_checked(result, manual));
        });
    }

    fn update_checked(self: &Rc<Self>, result: Result<Option<Release>, String>, manual: bool) {
        self.checking.set(false);
        match result {
            Ok(release) => {
                self.settings.borrow_mut().last_update_check = Some(now());
                self.save();
                let status = match &release {
                    Some(r) => format!("Version {} is available.", display(&r.version)),
                    None => "TypeThru is up to date.".into(),
                };
                *self.update_status.borrow_mut() = status.clone();
                let found = release.is_some();
                *self.update.borrow_mut() = release;
                if manual && !found {
                    alert("TypeThru is up to date", &format!("You have version {}.", display(&updater::current_version())));
                } else if manual {
                    self.show_panel(None);
                } else if found {
                    self.toast(&format!("{status} Open the TypeThru panel to install it."));
                }
            }
            Err(_) => {
                *self.update_status.borrow_mut() = "Could not check for updates.".into();
                if manual {
                    alert("Could not check for updates", "Check your internet connection and try again.");
                }
            }
        }
        self.refresh_panel();
        self.fill_settings();
    }

    pub(crate) fn install_update(self: &Rc<Self>) {
        let Some(release) = self.update.borrow().clone() else { return };
        if self.installing.get() {
            return;
        }
        if release.download.is_none() {
            native::open_page(release.page.as_str());
            return;
        }
        self.installing.set(true);
        *self.update_status.borrow_mut() = "Downloading and checking…".into();
        self.hide_panel();
        self.refresh_panel();
        self.fill_settings();
        std::thread::spawn(move || {
            let result = updater::install(&release);
            let page = release.page.to_string();
            later(move |app| app.update_installed(result, page));
        });
    }

    fn update_installed(self: &Rc<Self>, result: Result<std::path::PathBuf, String>, page: String) {
        match result.and_then(|exe| std::process::Command::new(exe).arg("--updated").spawn().map_err(|e| e.to_string())) {
            Ok(_) => self.quit(),
            Err(reason) => {
                self.installing.set(false);
                *self.update_status.borrow_mut() = "The update was not installed.".into();
                self.refresh_panel();
                self.fill_settings();
                let text = format!("{reason}\n\nYou can download it from the release page instead.");
                ask("The update was not installed", &text, "Open Release Page", "Close", move |_, yes| {
                    if yes {
                        native::open_page(&page);
                    }
                });
            }
        }
    }

    // MARK: What's New

    /// After an update, the notes of every version since the one last run here. Nothing for a new user.
    fn show_whats_new_after_update(self: &Rc<Self>) {
        let current = updater::current_version();
        let last = self.settings.borrow_mut().last_run_version.replace(current.clone());
        self.save();
        let Some(last) = last.filter(|last| is_newer(&current, last)) else { return };
        let bundled = parse_changelog(CHANGELOG);
        let entries: Vec<ChangelogEntry> = since(&last, &current, &bundled).into_iter().cloned().collect();
        if !entries.is_empty() {
            let title = if entries.len() == 1 { "What's New in TypeThru" } else { "What's New Since Your Last Version" };
            self.show_notes(&entries, title);
        }
    }

    pub(crate) fn show_whats_new(self: &Rc<Self>) {
        let current = updater::current_version();
        let entries: Vec<ChangelogEntry> = parse_changelog(CHANGELOG).into_iter().filter(|e| !is_newer(&e.version, &current)).take(3).collect();
        self.show_notes(&entries, "What's New in TypeThru");
    }

    pub(crate) fn show_update_notes(self: &Rc<Self>) {
        let Some(release) = self.update.borrow().clone() else { return };
        self.hide_panel();
        // A release remembered from an earlier check has no notes yet; GitHub's page has them.
        if release.notes.is_empty() {
            native::open_page(release.page.as_str());
        } else {
            self.show_notes(&release.notes, &format!("What's New in TypeThru {}", display(&release.version)));
        }
    }

    fn show_notes(self: &Rc<Self>, entries: &[ChangelogEntry], heading: &str) {
        if self.notes_window.borrow().is_none() {
            let Ok(window) = NotesWindow::new() else { return };
            window.set_font(FONT.into());
            let weak = window.as_weak();
            window.on_done(move || {
                if let Some(window) = weak.upgrade() {
                    let _ = window.hide();
                }
            });
            *self.notes_window.borrow_mut() = Some(window);
        }
        let notes = self.notes_window.borrow();
        let Some(window) = notes.as_ref() else { return };
        let mut lines = Vec::new();
        for entry in entries {
            lines.push(NoteLine { text: styled(&format!("**TypeThru {}**", escape(&entry.title))), bullet: false, heading: true });
            for line in &entry.lines {
                let (bullet, rest) = match line.strip_prefix("- ") {
                    Some(rest) => (true, rest),
                    None => (false, line.as_str()),
                };
                lines.push(NoteLine { text: styled(&inline_markdown(rest)), bullet, heading: false });
            }
        }
        window.set_heading(heading.into());
        window.set_lines(ModelRc::new(VecModel::from(lines)));
        if window.show().is_ok() {
            window.window().with_winit_window(|w| w.focus_window());
        }
    }

    // MARK: Diagnostics, uninstall, first run, quit

    pub(crate) fn show_diagnostics(&self) {
        let settings = self.settings.borrow();
        let installed = if install::is_installed() {
            "yes".to_owned()
        } else {
            format!("no, running from {}", install::current_exe().parent().map(|p| p.display().to_string()).unwrap_or_default())
        };
        let copy = if settings.copy_from_screen { title(Shortcut::COPY) } else { "off".into() };
        let text = format!(
            "Version: {}\nInstalled: {installed}\nShortcut: {}\nCopy Text from Screen: {copy}\nRunning as administrator: {}\n{}\n\n{}\n\n\
             Sent key presses do not confirm that the remote session accepted them. Clipboard contents are not recorded.",
            display(&updater::current_version()),
            title(settings.shortcut()),
            if native::is_elevated(None) { "yes" } else { "no" },
            self.last_copy.borrow(),
            self.last_attempt.borrow(),
        );
        alert("TypeThru Diagnostics", &text);
    }

    pub(crate) fn uninstall(&self) {
        ask(
            "Uninstall TypeThru?",
            "TypeThru removes its Start menu shortcut, login item, settings, and program files, then quits.",
            "Uninstall",
            "Cancel",
            |app, yes| {
                if yes {
                    install::uninstall();
                    app.quit();
                }
            },
        );
    }

    /// First run: install for this user, then the login question. Each is asked once.
    fn onboarding(self: &Rc<Self>) {
        let (install_asked, folder) = (self.settings.borrow().install_asked, install::folder());
        if install::is_installed() || install_asked {
            self.ask_launch_at_login();
            return;
        }
        self.settings.borrow_mut().install_asked = true;
        self.save();
        let text = format!(
            "TypeThru copies itself to {} and adds itself to the Start menu. No administrator password is needed, and updates install from the app.",
            folder.display()
        );
        ask("Install TypeThru?", &text, "Install", "Not Now", |app, yes| {
            if !yes {
                app.ask_launch_at_login();
                return;
            }
            match install::install().and_then(|()| std::process::Command::new(install::installed_exe()).spawn()) {
                Ok(_) => app.quit(),
                Err(error) => {
                    alert("Could not install TypeThru", &error.to_string());
                    app.ask_launch_at_login();
                }
            }
        });
    }

    fn ask_launch_at_login(self: &Rc<Self>) {
        if self.settings.borrow().launch_at_login_asked {
            return;
        }
        self.settings.borrow_mut().launch_at_login_asked = true;
        self.save();
        let shortcut = title(self.settings.borrow().shortcut());
        let welcome = format!("TypeThru is in the notification area. Press {shortcut} to type the clipboard.");
        if install::launch_at_login() {
            self.toast(&welcome);
            return;
        }
        let text = format!("TypeThru starts when you sign in, so {shortcut} always works. You can change this in Settings.");
        ask("Open TypeThru at login?", &text, "Open at Login", "Not Now", move |app, yes| {
            if yes {
                install::set_launch_at_login(true);
            }
            app.toast(&welcome);
        });
    }

    pub(crate) fn quit(&self) {
        let _ = self.panel.hide();
        let _ = self.selector.hide();
        if let Some(window) = &*self.settings_window.borrow() {
            let _ = window.hide();
        }
        if let Some(window) = &*self.notes_window.borrow() {
            let _ = window.hide();
        }
        if let Some(toast) = self.toast.take() {
            let _ = toast.hide();
        }
        self.update_timer.stop();
        // Removes the notification area icon.
        self.tray.borrow_mut().take();
        let _ = slint::quit_event_loop();
    }
}

fn styled(markdown: &str) -> StyledText {
    StyledText::from_markdown(markdown).unwrap_or_else(|_| StyledText::from_plain_text(markdown))
}

/// Escapes text so Markdown shows it as written.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// One line of release notes as Markdown for display: bold, italic, and code; links show only their label.
fn inline_markdown(line: &str) -> String {
    parse_inline(line)
        .into_iter()
        .map(|part| {
            let mut text = if part.code { format!("<font color=\"#6b94ff\">{}</font>", escape(&part.text)) } else { escape(&part.text) };
            if part.italic {
                text = format!("*{text}*");
            }
            if part.bold {
                text = format!("**{text}**");
            }
            text
        })
        .collect()
}

pub(crate) fn strings(items: impl Iterator<Item = &'static str>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.map(SharedString::from).collect::<Vec<_>>()))
}
