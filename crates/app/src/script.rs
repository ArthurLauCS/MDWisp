//! Scripted runs for visual and workflow regression: `MDWISP_SCRIPT=<file>`
//! feeds the window one command per line, through the same entry points as
//! real input, and `shot` saves the app's own frame (never the screen).
//!
//! ```text
//! size 1280 840        client size in DIPs
//! wait 300             milliseconds
//! scroll 1200          editor scroll offset
//! goto 表格            caret to the first match, scrolled into view
//! key Ctrl+End         a chord as written in the action registry
//! type 你好            text input
//! click 400 300        left click (rclick: right click)
//! move 400 300         pointer move
//! shot out/a.png       capture the next frame
//! quit                 close without asking to save
//! ```
//!
//! Run scripts against copies of documents in a temp folder: autosave is on,
//! and a script that types into a repo file (the bundled welcome document)
//! writes the change to disk.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::keys;
use crate::window::Window;

pub struct Script {
    lines: Vec<String>,
    next: usize,
    until: Instant,
}

impl Script {
    pub fn from_env() -> Option<Script> {
        let path = std::env::var("MDWISP_SCRIPT").ok()?;
        let text = std::fs::read_to_string(path).ok()?;
        Some(Script { lines: text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(Into::into).collect(), next: 0, until: Instant::now() + Duration::from_millis(300) })
    }
}

/// Run commands until one needs time to pass. Called from a timer.
pub fn step(w: &mut Window) {
    loop {
        let Some(s) = &mut w.script else { return };
        if Instant::now() < s.until || w.shot.is_some() {
            return;
        }
        let Some(line) = s.lines.get(s.next).cloned() else {
            w.script = None;
            return;
        };
        s.next += 1;
        let (cmd, arg) = line.split_once(' ').unwrap_or((&line, ""));
        let nums: Vec<f32> = arg.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        match cmd {
            "wait" => {
                s.until = Instant::now() + Duration::from_millis(nums.first().copied().unwrap_or(100.0) as u64);
                return;
            }
            "size" => unsafe {
                let sc = crate::win::dpi_of(w.hwnd) / 96.0;
                let mut r = windows::Win32::Foundation::RECT::default();
                let _ = GetWindowRect(w.hwnd, &mut r);
                let _ = SetWindowPos(w.hwnd, None, r.left, r.top, (nums[0] * sc) as i32, (nums[1] * sc) as i32, SWP_NOZORDER | SWP_NOACTIVATE);
            },
            "scroll" => {
                if let Some(d) = &mut w.doc {
                    d.view.scroll_to(nums[0], false);
                }
            }
            "goto" => {
                if let Some(d) = &mut w.doc {
                    if let Some(at) = d.view.buf.text.find(arg) {
                        d.view.move_to(at, false);
                        d.view.scroll_into_view(at, true);
                    }
                }
            }
            "key" => {
                let (m, name) = keys::parse(arg);
                if let Some(vk) = (1..255u16).map(VIRTUAL_KEY).find(|vk| keys::key_name(*vk).is_some_and(|n| keys::normalise(n) == keys::normalise(&name))) {
                    w.key_down(vk, m);
                }
            }
            "type" => {
                for c in arg.encode_utf16() {
                    w.char_input(c);
                }
            }
            "click" | "rclick" => {
                let b = if cmd == "click" { 0 } else { 1 };
                w.pointer_move(nums[0], nums[1], false);
                w.pointer_down(nums[0], nums[1], b, false);
                w.pointer_up(nums[0], nums[1], b);
            }
            "move" => w.pointer_move(nums[0], nums[1], false),
            "shot" => {
                w.shot = Some(PathBuf::from(arg));
            }
            // Scripted runs close without the save question; unsaved test edits are dropped.
            "quit" => unsafe {
                w.force_close = true;
                let _ = PostMessageW(Some(w.hwnd), WM_CLOSE, Default::default(), Default::default());
            },
            _ => {}
        }
        crate::win::request_frame(w.hwnd);
    }
}
