//! Settings in `%APPDATA%\TypeThru\settings.json`. Only options and dates are saved, never text.

use std::path::PathBuf;
use typethru_core::settings::Settings;

fn file() -> Option<PathBuf> {
    // Only on Windows: the Mac preview of the panel saves nothing.
    let folder = PathBuf::from(std::env::var_os("APPDATA").filter(|_| cfg!(windows))?);
    Some(folder.join("TypeThru").join("settings.json"))
}

pub fn load() -> Settings {
    let saved = file().and_then(|path| std::fs::read_to_string(path).ok());
    Settings::parse(saved.as_deref())
}

#[cfg_attr(not(windows), allow(dead_code))] // the Mac preview saves nothing
/// Writes a new file and then replaces the old one, so a crash never leaves half a file.
pub fn save(settings: &Settings) -> std::io::Result<()> {
    let path = file().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no settings folder"))?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let staged = path.with_extension("json.new");
    std::fs::write(&staged, settings.serialize())?;
    std::fs::rename(&staged, &path)
}
