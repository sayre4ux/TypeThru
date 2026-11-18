//! Direct Windows calls, kept in one place so the unsafe code is easy to review. DECISION: no
//! keyboard hooks and no raw input (the privacy contract); shortcuts use RegisterHotKey, which
//! tells TypeThru only that its own shortcut was pressed, and key state is only polled while
//! TypeThru waits, types, or records a new shortcut in Settings.

use std::cell::RefCell;
use std::ffi::c_void;
use typethru_core::typing::Shortcut;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId, ReleaseMutex, WaitForSingleObject};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, HKL, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, MAPVK_VK_TO_VSC_EX, MAPVK_VSC_TO_VK_EX,
    SendInput, VIRTUAL_KEY, VkKeyScanExW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, HOT_KEY_MODIFIERS, MAPVK_VK_TO_CHAR, MOD_NOREPEAT, MapVirtualKeyExW, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, HWND_MESSAGE, MB_ICONINFORMATION,
    MB_OK, MessageBoxW, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_HOTKEY, WNDCLASSW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, IDYES, MB_ICONQUESTION, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNORMAL,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

/// One TypeThru per Windows user: a second copy would fight over the shortcuts.
#[derive(Debug)]
pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// None when another copy keeps running. An update starts the new copy while the old one
    /// quits, so a running copy gets 5 seconds to finish.
    pub fn acquire() -> Option<Self> {
        // SAFETY: plain Win32 calls with a constant name; the handle is closed in Drop or below.
        unsafe {
            let handle = CreateMutexW(None, true, w!("Local\\io.github.sayre4ux.typethru")).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let waited = WaitForSingleObject(handle, 5000);
                if waited != WAIT_OBJECT_0 && waited != WAIT_ABANDONED {
                    let _ = CloseHandle(handle);
                    return None;
                }
            }
            Some(Self(handle))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and this copy owns the mutex.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

/// A message box. It blocks the calling thread, so the app shows it from a thread of its own.
pub fn message(title: &str, text: &str) {
    // SAFETY: both strings live until MessageBoxW returns.
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND);
    }
}

/// A Yes/No question; true for Yes. Blocks like `message`.
pub fn ask(title: &str, text: &str) -> bool {
    // SAFETY: both strings live until MessageBoxW returns.
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND) == IDYES }
}

pub fn beep() {
    // SAFETY: no arguments besides a constant.
    let _ = unsafe { MessageBeep(MB_ICONWARNING) };
}

/// Opens a web page in the default browser. Only called with TypeThru's checked release pages.
pub fn open_page(url: &str) {
    // SAFETY: the strings live until ShellExecuteW returns.
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

type HotKeyHandler = Box<dyn Fn(i32)>;

thread_local! {
    static HOTKEY_HANDLER: RefCell<Option<HotKeyHandler>> = const { RefCell::new(None) };
}

extern "system" fn hotkey_window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_HOTKEY {
        let id = i32::try_from(wparam.0).unwrap_or(-1);
        HOTKEY_HANDLER.with(|handler| {
            if let Some(handler) = &*handler.borrow() {
                handler(id);
            }
        });
        return LRESULT(0);
    }
    // SAFETY: forwards every other message unchanged to the default handler.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Global shortcuts, delivered to a hidden message-only window on the main thread.
#[derive(Debug)]
pub struct HotKeys {
    window: HWND,
}

impl HotKeys {
    /// `handler` gets the id of a pressed shortcut. It runs inside the window message loop, so it
    /// should only queue work.
    pub fn new(handler: impl Fn(i32) + 'static) -> windows::core::Result<Self> {
        HOTKEY_HANDLER.with(|slot| *slot.borrow_mut() = Some(Box::new(handler)));
        // SAFETY: registers a window class whose procedure is the function above, then creates
        // a message-only window of that class; both strings are static.
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(hotkey_window_proc),
                hInstance: instance.into(),
                lpszClassName: w!("TypeThruShortcuts"),
                ..Default::default()
            };
            RegisterClassW(&class);
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("TypeThruShortcuts"),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )?;
            Ok(Self { window })
        }
    }

    /// False when Windows refuses, usually because another app uses the shortcut.
    pub fn register(&self, id: i32, shortcut: Shortcut) -> bool {
        // SAFETY: the window is ours and alive; ids are TypeThru's own.
        unsafe {
            let _ = UnregisterHotKey(Some(self.window), id);
            RegisterHotKey(Some(self.window), id, HOT_KEY_MODIFIERS(shortcut.modifiers.0) | MOD_NOREPEAT, shortcut.key).is_ok()
        }
    }

    /// The hidden window, which also owns what Copy Text from Screen puts on the clipboard.
    pub fn handle(&self) -> isize {
        self.window.0 as isize
    }

    pub fn unregister(&self, id: i32) {
        // SAFETY: as above; unregistering an id that is not registered is harmless.
        unsafe {
            let _ = UnregisterHotKey(Some(self.window), id);
        }
    }
}

impl Drop for HotKeys {
    fn drop(&mut self) {
        self.unregister(1);
        self.unregister(2);
        // SAFETY: the window was created in new() and is destroyed once.
        unsafe {
            let _ = DestroyWindow(self.window);
        }
        HOTKEY_HANDLER.with(|slot| *slot.borrow_mut() = None);
    }
}

/// What the key types on the current keyboard layout, for showing shortcuts.
pub fn key_character(key: u32) -> Option<char> {
    // SAFETY: reads the layout of this thread; no pointers involved.
    let mapped = unsafe { MapVirtualKeyExW(key, MAPVK_VK_TO_CHAR, Some(GetKeyboardLayout(0))) };
    char::from_u32(mapped & 0x7FFF).filter(|c| *c > ' ')
}

/// The usable area (without the taskbar) of the screen at a point, in screen pixels.
pub fn work_area_at(x: i32, y: i32) -> RECT {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: MonitorFromPoint always returns a monitor with NEAREST; info has its size set.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(monitor, &mut info);
    }
    info.rcWork
}

pub fn cursor() -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: writes into a local POINT.
    let _ = unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}

/// Whether the window in front belongs to TypeThru (the panel, or its open speed menu).
pub fn foreground_is_ours() -> bool {
    let mut process = 0;
    // SAFETY: reads the foreground window's process id into a local.
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut process));
        process == GetCurrentProcessId()
    }
}

/// Windows 11 rounded corners, and the light or dark Acrylic tint.
pub fn style_window(hwnd: isize, dark: bool) {
    let window = HWND(hwnd as *mut c_void);
    let dark = BOOL::from(dark);
    // SAFETY: the window is TypeThru's own and alive; each value is a local of the documented size.
    // Errors are ignored: Windows 10 has no rounded corners, and the window still works.
    unsafe {
        let _ = DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE, (&raw const dark).cast(), size_of::<BOOL>() as u32);
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, (&raw const corner).cast(), size_of_val(&corner) as u32);
    }
}

pub fn foreground() -> isize {
    // SAFETY: no arguments.
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn window_process(window: isize) -> u32 {
    let mut process = 0;
    // SAFETY: reads the window's process id into a local; an invalid window gives 0.
    unsafe { GetWindowThreadProcessId(HWND(window as *mut c_void), Some(&mut process)) };
    process
}

pub fn current_process() -> u32 {
    // SAFETY: no arguments.
    unsafe { GetCurrentProcessId() }
}

/// The keyboard layout of the app that owns `window`, which may differ from TypeThru's.
pub fn layout_of(window: isize) -> isize {
    // SAFETY: plain reads; an invalid window gives thread 0, the layout of this thread.
    unsafe { GetKeyboardLayout(GetWindowThreadProcessId(HWND(window as *mut c_void), None)).0 as isize }
}

/// The key and shift state for a UTF-16 unit on `layout`: (virtual key, state), state 1 is Shift.
pub fn key_scan(unit: u16, layout: isize) -> Option<(u16, u8)> {
    // SAFETY: plain lookup in the layout's tables.
    let result = unsafe { VkKeyScanExW(unit, HKL(layout as *mut c_void)) };
    (result != -1).then_some(((result as u16) & 0xFF, ((result as u16) >> 8) as u8))
}

/// The hardware scan code of a virtual key on `layout`, with 0xE0 in the high byte for extended keys.
pub fn scan_code(key: u16, layout: isize) -> u32 {
    // SAFETY: plain lookup in the layout's tables.
    unsafe { MapVirtualKeyExW(u32::from(key), MAPVK_VK_TO_VSC_EX, Some(HKL(layout as *mut c_void))) }
}

/// The virtual key for a scan code (0xE0 prefix for extended keys) on TypeThru's layout.
pub fn scan_to_key(scan: u32) -> u32 {
    // SAFETY: plain lookup in the layout's tables.
    unsafe { MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, Some(GetKeyboardLayout(0))) }
}

/// One key event for SendInput: `flags` are KEYEVENTF_* values.
#[derive(Debug, Clone, Copy)]
pub struct KeyInput {
    pub key: u16,
    pub scan: u16,
    pub flags: u32,
}

/// Sends key events; false when Windows did not accept all of them (UIPI or the lock screen).
pub fn send(keys: &[KeyInput]) -> bool {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|k| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VIRTUAL_KEY(k.key), wScan: k.scan, dwFlags: KEYBD_EVENT_FLAGS(k.flags), time: 0, dwExtraInfo: 0 },
            },
        })
        .collect();
    // SAFETY: the inputs are fully initialized keyboard events and live until SendInput returns.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

/// Whether a key or mouse button is down now. Polled only while TypeThru waits or types.
pub fn is_down(key: i32) -> bool {
    // SAFETY: plain read of the key state.
    unsafe { GetAsyncKeyState(key) as u16 & 0x8000 != 0 }
}

/// Whether a modifier is down in the message just received (for recording a shortcut).
pub fn is_held(key: i32) -> bool {
    // SAFETY: plain read of this thread's key state.
    unsafe { GetKeyState(key) as u16 & 0x8000 != 0 }
}

/// The text on the clipboard, if any. Another app may hold the clipboard for a moment.
pub fn clipboard_text() -> Option<String> {
    for _ in 0..5 {
        // SAFETY: the clipboard is opened, read under GlobalLock within GlobalSize, and closed.
        unsafe {
            if IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT.0)).is_err() {
                return None;
            }
            if OpenClipboard(None).is_err() {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            let mut text = None;
            if let Ok(handle) = GetClipboardData(u32::from(CF_UNICODETEXT.0)) {
                let memory = HGLOBAL(handle.0);
                let data = GlobalLock(memory) as *const u16;
                if !data.is_null() {
                    let units = std::slice::from_raw_parts(data, GlobalSize(memory) / 2);
                    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
                    text = Some(String::from_utf16_lossy(&units[..end]));
                    let _ = GlobalUnlock(memory);
                }
            }
            let _ = CloseClipboard();
            return text;
        }
    }
    None
}

/// Whether a process runs as administrator. Windows lets a normal app send keys only to normal windows.
pub fn is_elevated(process: Option<u32>) -> bool {
    // SAFETY: handles are checked and closed; the token information is read into a local of its size.
    unsafe {
        let handle = match process {
            Some(id) => match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, id) {
                Ok(handle) => handle,
                Err(_) => return false,
            },
            None => GetCurrentProcess(),
        };
        let mut token = HANDLE::default();
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0;
        let elevated = OpenProcessToken(handle, TOKEN_QUERY, &mut token).is_ok()
            && GetTokenInformation(token, TokenElevation, Some((&raw mut elevation).cast()), size_of::<TOKEN_ELEVATION>() as u32, &mut length)
                .is_ok()
            && elevation.TokenIsElevated != 0;
        if !token.is_invalid() {
            let _ = CloseHandle(token);
        }
        if process.is_some() {
            let _ = CloseHandle(handle);
        }
        elevated
    }
}

/// The display scale at a point, 1.0 at 96 dpi.
pub fn scale_at(x: i32, y: i32) -> f64 {
    let (mut dpi, mut unused) = (96, 0);
    // SAFETY: MonitorFromPoint always returns a monitor with NEAREST; both outputs are locals.
    let ok = unsafe { GetDpiForMonitor(MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) };
    if ok.is_ok() { f64::from(dpi) / 96.0 } else { 1.0 }
}

/// Every screen together, in screen pixels: (left, top, width, height).
pub fn virtual_screen() -> (i32, i32, i32, i32) {
    // SAFETY: plain reads.
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// An exe's product name and version, from its version resource.
pub fn product_of(path: &std::path::Path) -> Option<(String, String)> {
    let file = HSTRING::from(path.as_os_str());
    // SAFETY: the buffer has the size Windows asked for; VerQueryValueW returns pointers into it,
    // read within the lengths it gives, while the buffer lives.
    unsafe {
        let size = GetFileVersionInfoSizeW(&file, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&file, None, size, data.as_mut_ptr().cast()).ok()?;
        let query = |name: &str| -> Option<Vec<u16>> {
            let (mut pointer, mut length) = (std::ptr::null_mut(), 0u32);
            VerQueryValueW(data.as_ptr().cast(), &HSTRING::from(name), &mut pointer, &mut length).as_bool().then_some(())?;
            (!pointer.is_null()).then(|| std::slice::from_raw_parts(pointer as *const u16, length as usize).to_vec())
        };
        let translation = query("\\VarFileInfo\\Translation")?;
        let (language, codepage) = (*translation.first()?, *translation.get(1)?);
        let text = |key: &str| {
            let units = query(&format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\{key}"))?;
            let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
            Some(String::from_utf16_lossy(&units[..end]))
        };
        Some((text("ProductName")?, text("ProductVersion")?))
    }
}

/// The local time as "14:05:09", for Diagnostics.
pub fn local_time() -> String {
    // SAFETY: no arguments; returns a plain struct.
    let now = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", now.wHour, now.wMinute, now.wSecond)
}
