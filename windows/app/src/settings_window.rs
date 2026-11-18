//! The Settings window (the Mac `Settings.swift`): options people change rarely.

use crate::controller::{App, COPY_SHORTCUT, TYPE_SHORTCUT, alert, strings, title};
use crate::ui::SettingsWindow;
use crate::{install, native, updater};
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use std::rc::Rc;
use typethru_core::settings::TypingMethod;
use typethru_core::text_rules::MarkdownMode;
use typethru_core::typing::{Modifiers, SPEEDS, Shortcut};
use typethru_core::update_feed::display;

const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const VK_LWIN: i32 = 0x5B;
const VK_RWIN: i32 = 0x5C;

impl App {
    pub(crate) fn open_settings(self: &Rc<Self>) {
        if self.settings_window.borrow().is_none() {
            let Ok(window) = SettingsWindow::new() else { return };
            window.set_font("Segoe UI Variable Text".into());
            window.set_methods(strings(TypingMethod::ALL.iter().map(|m| m.title())));
            window.set_speeds(strings(SPEEDS.iter().map(|s| s.title)));
            window.set_markdown_modes(strings(MarkdownMode::ALL.iter().map(|m| m.title())));
            window.set_version(display(&updater::current_version()).into());
            self.connect_settings(&window);
            *self.settings_window.borrow_mut() = Some(window);
        }
        self.fill_settings();
        if let Some(window) = &*self.settings_window.borrow()
            && window.show().is_ok()
        {
            window.window().with_winit_window(|w| w.focus_window());
        }
    }

    /// Shows the current settings in the Settings window, if it is open.
    pub(crate) fn fill_settings(&self) {
        let Some(window) = &*self.settings_window.borrow() else { return };
        let settings = self.settings.borrow();
        let index = |i: usize| i32::try_from(i).unwrap_or(0);
        window.set_launch_at_login(install::launch_at_login());
        window.set_shortcut(title(settings.shortcut()).into());
        window.set_recording(self.recording.get());
        window.set_method_index(index(TypingMethod::ALL.iter().position(|m| *m == settings.method).unwrap_or(0)));
        window.set_method_help(settings.method.help().into());
        window.set_speed_index(index(settings.speed_index));
        window.set_replace_symbols(settings.replace_symbols);
        window.set_markdown_index(index(MarkdownMode::ALL.iter().position(|m| *m == settings.markdown).unwrap_or(0)));
        window.set_shift_return(settings.shift_return);
        window.set_copy_from_screen(settings.copy_from_screen);
        window.set_copy_shortcut(title(Shortcut::COPY).into());
        window.set_check_updates(settings.check_updates);
        window.set_update_available(self.update.borrow().is_some());
        window.set_installing(self.installing.get());
    }

    fn connect_settings(self: &Rc<Self>, window: &SettingsWindow) {
        macro_rules! on {
            ($callback:ident, |$app:ident $(, $arg:ident)?| $body:expr) => {{
                let weak = Rc::downgrade(self);
                window.$callback(move |$($arg)?| {
                    if let Some($app) = weak.upgrade() {
                        $body;
                    }
                });
            }};
        }
        on!(on_launch_changed, |app, on| {
            if !install::set_launch_at_login(on) {
                alert("Could not change the login item", "Windows did not allow the change.");
            }
            app.fill_settings()
        });
        on!(on_toggle_recording, |app| app.toggle_recording());
        on!(on_method_changed, |app, i| app.change(|s| s.method = TypingMethod::ALL[usize::try_from(i).unwrap_or(0).min(2)]));
        on!(on_speed_changed, |app, i| app.change(|s| s.speed_index = usize::try_from(i).unwrap_or(0)));
        on!(on_replace_changed, |app, on| app.change(|s| s.replace_symbols = on));
        on!(on_markdown_changed, |app, i| app.change(|s| s.markdown = MarkdownMode::ALL[usize::try_from(i).unwrap_or(0).min(2)]));
        on!(on_shift_return_changed, |app, on| app.change(|s| s.shift_return = on));
        on!(on_copy_changed, |app, on| {
            app.change(|s| s.copy_from_screen = on);
            app.register_shortcuts()
        });
        on!(on_check_updates_changed, |app, on| app.change(|s| s.check_updates = on));
        on!(on_check_now, |app| app.check_for_updates(true));
        on!(on_update_notes, |app| app.show_update_notes());
        on!(on_install_update, |app| app.install_update());
        on!(on_whats_new, |app| app.show_whats_new());
        on!(on_diagnostics, |app| app.show_diagnostics());
        on!(on_uninstall, |app| app.uninstall());

        let weak = Rc::downgrade(self);
        window.window().on_close_requested(move || {
            if let Some(app) = weak.upgrade()
                && app.recording.replace(false)
            {
                app.register_shortcuts();
                app.set_shortcut_note("");
            }
            slint::CloseRequestResponse::HideWindow
        });
        // While recording, the next key pressed in this window becomes the shortcut. Only this
        // window's own key messages are read; there is no keyboard hook.
        let weak = Rc::downgrade(self);
        window.window().on_winit_window_event(move |_, event| {
            use winit::platform::scancode::PhysicalKeyExtScancode;
            let Some(app) = weak.upgrade().filter(|app| app.recording.get()) else { return EventResult::Propagate };
            if let winit::event::WindowEvent::KeyboardInput { event, .. } = event {
                if event.state == winit::event::ElementState::Pressed
                    && let Some(scan) = event.physical_key.to_scancode()
                {
                    app.record(native::scan_to_key(scan));
                }
                return EventResult::PreventDefault;
            }
            EventResult::Propagate
        });
    }

    fn set_shortcut_note(&self, note: &str) {
        if let Some(window) = &*self.settings_window.borrow() {
            window.set_shortcut_note(note.into());
        }
    }

    /// The global shortcut is off while a new one is recorded.
    fn toggle_recording(&self) {
        let recording = !self.recording.get();
        self.recording.set(recording);
        if recording {
            self.hotkeys.unregister(TYPE_SHORTCUT);
            self.hotkeys.unregister(COPY_SHORTCUT);
            self.set_shortcut_note("Press the new shortcut. Esc cancels.");
        } else {
            self.register_shortcuts();
            self.set_shortcut_note("");
        }
        self.fill_settings();
    }

    fn record(&self, key: u32) {
        if key == 0 || [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5].contains(&(key as i32)) {
            return;
        }
        let mut modifiers = Modifiers::NONE;
        for (vk, modifier) in [(VK_CONTROL, Modifiers::CONTROL), (VK_MENU, Modifiers::ALT), (VK_SHIFT, Modifiers::SHIFT)] {
            if native::is_held(vk) {
                modifiers = modifiers | modifier;
            }
        }
        if native::is_held(VK_LWIN) || native::is_held(VK_RWIN) {
            modifiers = modifiers | Modifiers::WINDOWS;
        }
        if key == Shortcut::ESCAPE && modifiers == Modifiers::NONE {
            self.toggle_recording();
            return;
        }
        let shortcut = Shortcut { modifiers, key };
        if let Some(problem) = shortcut.problem() {
            self.set_shortcut_note(problem);
            return;
        }
        if shortcut == Shortcut::COPY && self.settings.borrow().copy_from_screen {
            self.set_shortcut_note(&format!("{} copies text from the screen. Choose another shortcut.", title(Shortcut::COPY)));
            return;
        }
        let previous = self.settings.borrow().shortcut();
        self.settings.borrow_mut().set_shortcut(shortcut);
        self.recording.set(false);
        if self.register_shortcuts() {
            self.save();
            self.set_shortcut_note(shortcut.caution().unwrap_or(""));
        } else {
            self.settings.borrow_mut().set_shortcut(previous);
            self.register_shortcuts();
            self.set_shortcut_note(&format!("Windows did not accept {}. Another app may use it.", title(shortcut)));
        }
        self.change(|_| {});
    }
}
