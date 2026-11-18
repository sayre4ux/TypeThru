//! Where TypeThru lives: installed per user, without an administrator password, with a Start
//! menu shortcut and an optional login item (the C# `Installation`).

use std::path::{Path, PathBuf};
use std::process::Command;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn env_folder(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_default())
}

pub fn folder() -> PathBuf {
    env_folder("LOCALAPPDATA").join("Programs").join("TypeThru")
}

pub fn installed_exe() -> PathBuf {
    folder().join("TypeThru.exe")
}

pub fn data_folder() -> PathBuf {
    env_folder("APPDATA").join("TypeThru")
}

fn start_menu_shortcut() -> PathBuf {
    env_folder("APPDATA").join(r"Microsoft\Windows\Start Menu\Programs\TypeThru.lnk")
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| installed_exe())
}

fn same(a: &Path, b: &Path) -> bool {
    let full = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    full(a) == full(b)
}

pub fn is_installed() -> bool {
    same(&current_exe(), &installed_exe())
}

/// Copies this exe to the per-user programs folder and adds it to the Start menu.
pub fn install() -> std::io::Result<()> {
    std::fs::create_dir_all(folder())?;
    std::fs::copy(current_exe(), installed_exe())?;
    // A missing Start menu shortcut is not worth failing the install for.
    let _ = create_shortcut();
    Ok(())
}

fn create_shortcut() -> windows::core::Result<()> {
    // SAFETY: COM is initialized for this thread (a second call is harmless); every string lives
    // until the call that takes it returns.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(installed_exe().as_os_str()))?;
        link.SetWorkingDirectory(&HSTRING::from(folder().as_os_str()))?;
        link.SetDescription(&HSTRING::from("Type the clipboard as key presses"))?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(start_menu_shortcut().as_os_str()), true)
    }
}

pub fn launch_at_login() -> bool {
    let mut buffer = vec![0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: RegGetValueW writes at most `size` bytes into the buffer and sets `size` to what it wrote.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            &HSTRING::from("TypeThru"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if result != ERROR_SUCCESS {
        return false;
    }
    buffer.truncate(size as usize / 2);
    let value = String::from_utf16_lossy(&buffer).trim_end_matches('\0').to_lowercase();
    value.contains(&current_exe().to_string_lossy().to_lowercase())
}

pub fn set_launch_at_login(on: bool) -> bool {
    let (key, name) = (HSTRING::from(RUN_KEY), HSTRING::from("TypeThru"));
    // SAFETY: the value is a NUL-terminated UTF-16 string of the size given.
    let result = unsafe {
        if on {
            let value: Vec<u16> = format!("\"{}\"", current_exe().display()).encode_utf16().chain([0]).collect();
            RegSetKeyValueW(HKEY_CURRENT_USER, &key, &name, REG_SZ.0, Some(value.as_ptr().cast()), (value.len() * 2) as u32)
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name)
        }
    };
    result == ERROR_SUCCESS || !on
}

/// Removes TypeThru's shortcut, login item, settings, and files. The exe is deleted after TypeThru quits.
pub fn uninstall() {
    set_launch_at_login(false);
    let _ = std::fs::remove_file(start_menu_shortcut());
    let _ = std::fs::remove_file(data_folder().join("settings.json"));
    let _ = std::fs::remove_dir(data_folder());
    if !is_installed() {
        return;
    }
    // DECISION: a running exe cannot delete itself; cmd waits two seconds, then deletes only
    // TypeThru's own files and the folder if it is empty.
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let (exe, folder) = (installed_exe(), folder());
    let script = format!(
        "/c timeout /t 2 /nobreak >nul & del /f /q \"{exe}\" \"{exe}.old\" & rmdir \"{folder}\"",
        exe = exe.display(),
        folder = folder.display()
    );
    let _ = Command::new("cmd.exe").raw_arg(script).creation_flags(CREATE_NO_WINDOW).spawn();
}
