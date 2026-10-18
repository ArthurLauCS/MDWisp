//! Win32 clipboard: Unicode text in and out, bitmaps in (for pasted images).

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};

struct Open;

impl Open {
    fn new(hwnd: HWND) -> Option<Open> {
        // Another process may hold it for a moment; retry briefly.
        for _ in 0..10 {
            if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

pub fn text(hwnd: HWND) -> Option<String> {
    let _open = Open::new(hwnd)?;
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        let _ = GlobalUnlock(g);
        Some(s.replace("\r\n", "\n"))
    }
}

pub fn set_text(hwnd: HWND, s: &str) -> bool {
    let Some(_open) = Open::new(hwnd) else { return false };
    let w: Vec<u16> = s.replace('\n', "\r\n").encode_utf16().chain([0]).collect();
    unsafe {
        let _ = EmptyClipboard();
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2) else { return false };
        let p = GlobalLock(g) as *mut u16;
        std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
        let _ = GlobalUnlock(g);
        SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(g.0))).is_ok()
    }
}

/// The clipboard bitmap as a DIB (BITMAPINFOHEADER + pixels), if any.
pub fn dib(hwnd: HWND) -> Option<Vec<u8>> {
    let _open = Open::new(hwnd)?;
    unsafe {
        let h = GetClipboardData(CF_DIB.0 as u32).ok()?;
        let g = HGLOBAL(h.0);
        let size = GlobalSize(g);
        let p = GlobalLock(g) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, size).to_vec();
        let _ = GlobalUnlock(g);
        Some(v)
    }
}

pub fn has_image() -> bool {
    unsafe { IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok() }
}
