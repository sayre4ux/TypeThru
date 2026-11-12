//! Fills the panel from the settings. Shared by the Windows app and the Mac preview.

use crate::Panel;
use slint::{ModelRc, SharedString, VecModel};
use typethru_core::settings::Settings;
use typethru_core::typing::{SPEEDS, Shortcut};

fn strings(items: impl Iterator<Item = &'static str>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.map(SharedString::from).collect::<Vec<_>>()))
}

/// `character` tells what a key types on the current keyboard layout.
pub fn create(settings: &Settings, character: impl Fn(u32) -> Option<char>) -> Result<Panel, slint::PlatformError> {
    let panel = Panel::new()?;
    panel.set_speeds(strings(SPEEDS.iter().map(|s| s.title)));
    panel.set_speed_details(strings(SPEEDS.iter().map(|s| s.detail)));
    show_settings(&panel, settings, character);
    Ok(panel)
}

pub fn show_settings(panel: &Panel, settings: &Settings, character: impl Fn(u32) -> Option<char>) {
    let title = |shortcut: Shortcut| SharedString::from(shortcut.title(character(shortcut.key)));
    panel.set_speed_index(i32::try_from(settings.speed_index).unwrap_or(0));
    panel.set_shift_return(settings.shift_return);
    panel.set_copy_from_screen(settings.copy_from_screen);
    panel.set_shortcut(title(settings.shortcut()));
    panel.set_copy_shortcut(title(Shortcut::COPY));
}
