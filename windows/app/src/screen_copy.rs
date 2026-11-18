//! Copy Text from Screen (the Mac `ScreenText`): the picture of the selected area stays in memory,
//! Windows' own text recognition reads it on this PC, and only the text goes on the clipboard.
//! The only place that captures the screen or writes the clipboard (checked by the audit).

use std::time::Duration;
use typethru_core::notes::{Piece, layout};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetDC, GetDIBits, HALFTONE, ReleaseDC, SRCCOPY, SelectObject, SetStretchBltMode, StretchBlt,
};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

/// An area of the screen, in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// Reads the text in `area`: empty when there is none. Runs on a worker thread.
pub fn read(area: Area) -> Result<String, String> {
    // SAFETY: initializes Windows Runtime for this worker thread; a second call is harmless.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| no_engine())?;
    // DECISION: small text on a 100% scale screen reads much better at twice the size, as on the Mac.
    let scale = crate::native::scale_at(area.left + area.width / 2, area.top + area.height / 2);
    let largest = OcrEngine::MaxImageDimension().unwrap_or(2600) as i32;
    let factor = if scale < 1.5 && area.width.max(area.height) * 2 <= largest { 2 } else { 1 };
    let (width, height) = (area.width * factor, area.height * factor);
    let pixels = capture(area, width, height).ok_or("Windows did not allow a picture of the screen.")?;
    let fail = |error: windows::core::Error| format!("Text recognition failed: {}", error.message());
    let writer = DataWriter::new().map_err(fail)?;
    writer.WriteBytes(&pixels).map_err(fail)?;
    drop(pixels);
    let bitmap =
        SoftwareBitmap::CreateCopyFromBuffer(&writer.DetachBuffer().map_err(fail)?, BitmapPixelFormat::Bgra8, width, height).map_err(fail)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(fail)?.join().map_err(fail)?;
    let mut pieces = Vec::new();
    for line in result.Lines().map_err(fail)? {
        let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, 0.0f64, 0.0f64);
        for word in line.Words().map_err(fail)? {
            let Ok(r) = word.BoundingRect() else { continue };
            let (x, y, w, h) = (f64::from(r.X), f64::from(r.Y), f64::from(r.Width), f64::from(r.Height));
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + w);
            bottom = bottom.max(y + h);
        }
        if right > left {
            let text = line.Text().map_err(fail)?.to_string_lossy();
            pieces.push(Piece { text, x: left, y: top, width: right - left, height: bottom - top });
        }
    }
    Ok(layout(&pieces))
}

fn no_engine() -> String {
    "Windows has no text recognition for your languages. Add one in Settings › Time & language › Language & region, under the language's options."
        .to_owned()
}

/// The area as top-down BGRA pixels, `width` × `height` (larger than the area when enlarged).
fn capture(area: Area, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut pixels = vec![0u8; usize::try_from(width).ok()? * usize::try_from(height).ok()? * 4];
    // SAFETY: every GDI object made here is selected out and deleted before returning; GetDIBits
    // writes at most width × height 32-bit pixels, the size of `pixels`.
    let copied = unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, bitmap.into());
        let drawn = if width == area.width {
            BitBlt(memory, 0, 0, width, height, Some(screen), area.left, area.top, SRCCOPY | CAPTUREBLT).is_ok()
        } else {
            SetStretchBltMode(memory, HALFTONE);
            StretchBlt(memory, 0, 0, width, height, Some(screen), area.left, area.top, area.width, area.height, SRCCOPY | CAPTUREBLT).as_bool()
        };
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        SelectObject(memory, previous);
        let lines = if drawn { GetDIBits(memory, bitmap, 0, height as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        lines == height
    };
    // The screen has no transparency; make every pixel opaque for text recognition.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    copied.then_some(pixels)
}

/// Puts `text` on the clipboard, owned by `owner` (a TypeThru window). Another app may hold the
/// clipboard for a moment.
pub fn set_clipboard(owner: isize, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
    for _ in 0..6 {
        // SAFETY: the clipboard is opened and closed here; the memory holds `units` exactly and
        // belongs to the clipboard once SetClipboardData succeeds, or is freed otherwise.
        unsafe {
            if OpenClipboard(Some(HWND(owner as *mut std::ffi::c_void))).is_err() {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            let mut done = false;
            if EmptyClipboard().is_ok()
                && let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2)
            {
                let target = GlobalLock(memory) as *mut u16;
                if !target.is_null() {
                    std::ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
                    let _ = GlobalUnlock(memory);
                    done = SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(memory.0))).is_ok();
                }
                if !done {
                    let _ = GlobalFree(Some(memory));
                }
            }
            let _ = CloseClipboard();
            return done;
        }
    }
    false
}
