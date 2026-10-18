//! Watch one document file for changes made by other programs.
//!
//! ReadDirectoryChangesW on the file's folder, filtered to its name. Writers
//! often save in several steps, so a change is reported only after the file
//! has been quiet for 150 ms — the renderer never reads half a file.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::IO::CancelIoEx;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::PCWSTR;

/// Posted to the window when the watched file changed on disk.
pub const WM_APP_DISK: u32 = WM_APP + 4;

pub struct Watcher {
    dir: HANDLE,
    stop: Arc<AtomicBool>,
}

unsafe impl Send for Watcher {}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        unsafe {
            let _ = CancelIoEx(self.dir, None);
            let _ = CloseHandle(self.dir);
        }
    }
}

pub fn watch(file: &Path, hwnd: HWND) -> Option<Watcher> {
    let dir_path = file.parent()?.to_path_buf();
    let name: Vec<u16> = file.file_name()?.encode_wide().collect();
    let wdir: Vec<u16> = dir_path.as_os_str().encode_wide().chain([0]).collect();
    let dir = unsafe {
        CreateFileW(PCWSTR(wdir.as_ptr()), FILE_LIST_DIRECTORY.0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, None, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None).ok()?
    };
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let raw = dir.0 as isize;
    let hwnd = hwnd.0 as isize;
    std::thread::spawn(move || {
        let dir = HANDLE(raw as _);
        let mut buf = vec![0u32; 16 * 1024];
        while !stop2.load(Ordering::Relaxed) {
            let mut got = 0u32;
            let ok = unsafe {
                ReadDirectoryChangesW(dir, buf.as_mut_ptr() as _, (buf.len() * 4) as u32, false, FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE, Some(&mut got), None, None)
            };
            if ok.is_err() || stop2.load(Ordering::Relaxed) {
                return;
            }
            if got == 0 || !mentions(&buf, &name) {
                continue;
            }
            // Debounce: wait until the writer has been quiet for a moment.
            std::thread::sleep(Duration::from_millis(150));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_DISK, WPARAM(0), LPARAM(0));
            }
        }
    });
    Some(Watcher { dir, stop })
}

fn mentions(buf: &[u32], name: &[u16]) -> bool {
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
    let mut off = 0usize;
    loop {
        let info = unsafe { &*(bytes.as_ptr().add(off) as *const FILE_NOTIFY_INFORMATION) };
        let len = info.FileNameLength as usize / 2;
        let fname = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), len) };
        if fname.len() == name.len() && fname.iter().zip(name).all(|(a, b)| lower(*a) == lower(*b)) {
            return true;
        }
        if info.NextEntryOffset == 0 {
            return false;
        }
        off += info.NextEntryOffset as usize;
    }
}

fn lower(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) { c + 32 } else { c }
}
