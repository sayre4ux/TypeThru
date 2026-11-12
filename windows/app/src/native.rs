//! Direct Windows calls, kept in one place so the unsafe code is easy to review. DECISION: no
//! keyboard hooks and no raw input (the privacy contract); shortcuts use RegisterHotKey, which
//! tells TypeThru only that its own shortcut was pressed.

use std::cell::RefCell;
use std::ffi::c_void;
use typethru_core::typing::Shortcut;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId, ReleaseMutex, WaitForSingleObject};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, HOT_KEY_MODIFIERS, MAPVK_VK_TO_CHAR, MOD_NOREPEAT, MapVirtualKeyExW, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, HWND_MESSAGE, MB_ICONINFORMATION,
    MB_OK, MessageBoxW, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_HOTKEY, WNDCLASSW,
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

pub fn message(title: &str, text: &str) {
    // SAFETY: both strings live until MessageBoxW returns.
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION);
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
