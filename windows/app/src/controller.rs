//! The app on Windows: notification area icon, shortcuts, and the glass panel (the Mac
//! `AppDelegate`). Typing, Copy Text from Screen, Settings, and updates come in later steps.

use crate::native::{self, HotKeys, SingleInstance};
use crate::{Panel, panel, storage};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use typethru_core::settings::Settings;
use typethru_core::typing::Shortcut;

const TYPE_SHORTCUT: i32 = 1;
const COPY_SHORTCUT: i32 = 2;
const PANEL_TITLE: &str = "TypeThru";
const NOT_YET: &str = "This test build shows the panel only. Typing, Copy Text from Screen, and Settings come in the next builds.";

struct App {
    panel: Panel,
    settings: RefCell<Settings>,
    hotkeys: HotKeys,
    tray: RefCell<Option<TrayIcon>>,
    /// When the panel last closed because it lost focus: a click on the icon that caused it must not reopen it.
    hidden_at: Cell<Option<Instant>>,
    glass_applied: Cell<bool>,
    /// Problems with the shortcuts, shown until they are solved.
    shortcut_notice: RefCell<String>,
    _single: SingleInstance,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// Runs `work` with the app on the main thread, from any thread.
fn later(work: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = APP.with(|slot| slot.borrow().clone()) {
            work(&app);
        }
    });
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
    // Free the shortcuts and the single-copy lock before the process ends.
    APP.with(|slot| slot.borrow_mut().take());
}

fn start(single: SingleInstance) -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mut backend = slint::BackendSelector::new().backend_name("winit".into()).with_winit_window_attributes_hook(|attributes| {
        use winit::platform::windows::WindowAttributesExtWindows;
        if attributes.title == PANEL_TITLE {
            // See-through for the glass, and no taskbar button: the panel belongs to the notification area icon.
            attributes.with_transparent(true).with_skip_taskbar(true)
        } else {
            attributes
        }
    });
    // DECISION: --renderer femtovg|software, to compare Slint's renderers while the glass is tested on PCs.
    if let Some(renderer) = args.iter().position(|a| a == "--renderer").and_then(|i| args.get(i + 1)) {
        backend = backend.renderer_name(renderer.clone());
    }
    backend.select()?;

    let settings = storage::load();
    let panel = panel::create(&settings, native::key_character)?;
    panel.set_font("Segoe UI Variable Text".into());
    let hotkeys = HotKeys::new(|id| later(move |app| app.shortcut_pressed(id)))?;
    let app = Rc::new(App {
        panel,
        settings: RefCell::new(settings),
        hotkeys,
        tray: RefCell::new(None),
        hidden_at: Cell::new(None),
        glass_applied: Cell::new(false),
        shortcut_notice: RefCell::new(String::new()),
        _single: single,
    });
    app.connect_panel();
    app.register_shortcuts();
    APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));

    // The icon needs the running message loop, so it is added once the loop starts.
    slint::Timer::single_shot(Duration::ZERO, || {
        later(|app| {
            if let Err(error) = app.add_icon() {
                app.notice(&format!("Could not add the notification area icon: {error}"));
            }
            app.show_panel(None, String::new());
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
        panel.on_type_clipboard(with(|app| app.notice(NOT_YET)));
        panel.on_copy_from_screen_clicked(with(|app| app.notice(NOT_YET)));
        panel.on_open_settings(with(|app| app.notice(NOT_YET)));
        panel.on_whats_new(with(|app| app.notice(NOT_YET)));
        panel.on_install_update(with(|app| app.notice(NOT_YET)));
        panel.on_quit(|| {
            let _ = slint::quit_event_loop();
        });
        panel.on_dismiss(with(|app| app.hide_panel()));
        let weak = Rc::downgrade(self);
        panel.on_run_test(move |_| {
            if let Some(app) = weak.upgrade() {
                app.notice(NOT_YET);
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
                slint::Timer::single_shot(Duration::from_millis(80), move || {
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

    fn change(&self, edit: impl FnOnce(&mut Settings)) {
        let mut settings = self.settings.borrow_mut();
        edit(&mut settings);
        if let Err(error) = storage::save(&settings) {
            drop(settings);
            self.notice(&format!("Could not save the setting: {error}"));
        }
    }

    fn register_shortcuts(&self) {
        let shortcut = self.settings.borrow().shortcut();
        let mut problems = Vec::new();
        for (id, shortcut, what) in [(TYPE_SHORTCUT, shortcut, "Type Clipboard"), (COPY_SHORTCUT, Shortcut::COPY, "Copy Text from Screen")] {
            if id == COPY_SHORTCUT && !self.settings.borrow().copy_from_screen {
                self.hotkeys.unregister(id);
            } else if !self.hotkeys.register(id, shortcut) {
                let title = shortcut.title(native::key_character(shortcut.key));
                problems.push(format!("Could not use {title} for {what}: another app uses it."));
            }
        }
        *self.shortcut_notice.borrow_mut() = problems.join("\n");
    }

    fn shortcut_pressed(self: &Rc<Self>, id: i32) {
        let title = |s: Shortcut| s.title(native::key_character(s.key));
        let what = match id {
            TYPE_SHORTCUT => title(self.settings.borrow().shortcut()),
            COPY_SHORTCUT => title(Shortcut::COPY),
            _ => return,
        };
        self.show_panel(None, format!("{what} works. {NOT_YET}"));
    }

    fn notice(&self, text: &str) {
        self.panel.set_notice(text.into());
    }

    fn add_icon(self: &Rc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        let open = MenuItem::new("Open TypeThru", true, None);
        let quit = MenuItem::new("Quit TypeThru", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit])?;
        let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == quit_id {
                let _ = slint::quit_event_loop();
            } else if event.id == open_id {
                later(|app| app.show_panel(None, String::new()));
            }
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

    fn toggle_panel(self: &Rc<Self>, anchor: (i32, i32)) {
        let just_hidden = self.hidden_at.get().is_some_and(|at| at.elapsed() < Duration::from_millis(400));
        if self.panel.window().is_visible() || just_hidden {
            self.hide_panel();
        } else {
            self.show_panel(Some(anchor), String::new());
        }
    }

    fn hide_panel(&self) {
        if self.panel.window().is_visible() {
            self.hidden_at.set(Some(Instant::now()));
            let _ = self.panel.hide();
        }
    }

    /// Shows the panel above the icon (`anchor`), or at the bottom right of the screen with the pointer.
    fn show_panel(self: &Rc<Self>, anchor: Option<(i32, i32)>, notice: String) {
        let shortcut_notice = self.shortcut_notice.borrow().clone();
        let notice = [shortcut_notice, notice].into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("\n");
        self.panel.set_notice(notice.into());
        panel::show_settings(&self.panel, &self.settings.borrow(), native::key_character);
        if !self.panel.window().is_visible() {
            // Off screen until the size is known, then moved into place below.
            self.panel.window().set_position(PhysicalPosition::new(-32000, -32000));
            if self.panel.show().is_err() {
                return;
            }
        }
        let weak = Rc::downgrade(self);
        slint::Timer::single_shot(Duration::ZERO, move || {
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
}
