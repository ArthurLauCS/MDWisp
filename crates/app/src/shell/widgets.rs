//! Small controls shared by the overlays: a single-line text field and the
//! button / switch / segmented-control painters.

use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectWrite::DWRITE_HIT_TEST_METRICS;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::core::BOOL;

use crate::gfx::{Gfx, Rect, Style, metrics};
use crate::keys::Mods;
use crate::theme::{self, Color};
use crate::ui::{Hit, Ui};
use crate::{app, clipboard};

/// A one-line editable string. Offsets are UTF-8 bytes on char boundaries.
#[derive(Default, Clone)]
pub struct Field {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
    pub scroll: f32,
    pub blink: Option<Instant>,
    /// Where the caret was drawn, for the IME window.
    pub caret_box: Option<(f32, f32, f32)>,
}

impl Field {
    pub fn new(text: &str) -> Field {
        Field { text: text.into(), caret: text.len(), anchor: text.len(), ..Default::default() }
    }

    fn sel(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn insert(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        let (a, b) = self.sel();
        self.text.replace_range(a..b, &s);
        self.caret = a + s.len();
        self.anchor = self.caret;
        self.blink = Some(Instant::now());
    }

    fn prev(&self, p: usize) -> usize {
        self.text[..p].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    fn next(&self, p: usize) -> usize {
        self.text[p..].chars().next().map_or(p, |c| p + c.len_utf8())
    }

    fn word(&self, p: usize, dir: i32) -> usize {
        let b = crate::editor::buffer::Buffer::new(self.text.clone());
        b.word(p, dir)
    }

    /// Editing keys; false for keys the field does not use (Enter, Escape, Up/Down).
    pub fn key(&mut self, vk: VIRTUAL_KEY, m: Mods, hwnd: HWND) -> bool {
        let (a, b) = self.sel();
        let moved = |f: &mut Field, p: usize| {
            f.caret = p;
            if !m.shift {
                f.anchor = p;
            }
        };
        match vk {
            VK_LEFT => {
                let p = if m.ctrl { self.word(self.caret, -1) } else if a != b && !m.shift { a } else { self.prev(self.caret) };
                moved(self, p);
            }
            VK_RIGHT => {
                let p = if m.ctrl { self.word(self.caret, 1) } else if a != b && !m.shift { b } else { self.next(self.caret) };
                moved(self, p);
            }
            VK_HOME => moved(self, 0),
            VK_END => moved(self, self.text.len()),
            VK_BACK => {
                if a == b && a > 0 {
                    let p = if m.ctrl { self.word(a, -1) } else { self.prev(a) };
                    self.text.replace_range(p..a, "");
                    self.caret = p;
                } else {
                    self.text.replace_range(a..b, "");
                    self.caret = a;
                }
                self.anchor = self.caret;
            }
            VK_DELETE => {
                if a == b {
                    let p = if m.ctrl { self.word(a, 1) } else { self.next(a) };
                    self.text.replace_range(a..p, "");
                } else {
                    self.text.replace_range(a..b, "");
                }
                self.caret = a;
                self.anchor = a;
            }
            k if m.ctrl && k.0 == b'A' as u16 => {
                self.anchor = 0;
                self.caret = self.text.len();
            }
            k if m.ctrl && (k.0 == b'C' as u16 || k.0 == b'X' as u16) => {
                if a != b {
                    clipboard::set_text(hwnd, &self.text[a..b]);
                    if k.0 == b'X' as u16 {
                        self.insert("");
                    }
                }
            }
            k if m.ctrl && k.0 == b'V' as u16 => {
                if let Some(s) = clipboard::text(hwnd) {
                    self.insert(&s);
                }
            }
            _ => return false,
        }
        self.blink = Some(Instant::now());
        true
    }

    /// Place the caret from a click at `x` inside the field box.
    pub fn click(&mut self, g: &Gfx, style: Style, r: Rect, x: f32, extend: bool) {
        let l = g.layout(&self.text, style, 1.0e5);
        let mut trailing = BOOL(0);
        let mut inside = BOOL(0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        unsafe { l.HitTestPoint(x - r.x + self.scroll, 1.0, &mut trailing, &mut inside, &mut m).unwrap() };
        let d = m.textPosition + if trailing.as_bool() { m.length } else { 0 };
        let mut n = 0u32;
        let mut pos = self.text.len();
        for (i, c) in self.text.char_indices() {
            if n >= d {
                pos = i;
                break;
            }
            n += c.len_utf16() as u32;
        }
        self.caret = pos;
        if !extend {
            self.anchor = pos;
        }
        self.blink = Some(Instant::now());
    }

    /// Draw text, selection and caret inside `r`; the caret only when focused.
    pub fn paint(&mut self, g: &Gfx, style: Style, r: Rect, color: Color, placeholder: &str, focused: bool) {
        let th = app().theme;
        let l = g.layout(&self.text, style, 1.0e5);
        let m = metrics(&l);
        let y = r.y + (r.h - m.h.max(style.size * 1.3)) / 2.0;
        let caret16 = self.text[..self.caret].encode_utf16().count() as u32;
        let (mut cx, mut cy) = (0.0, 0.0);
        let mut hm = DWRITE_HIT_TEST_METRICS::default();
        unsafe {
            let _ = l.HitTestTextPosition(caret16, false, &mut cx, &mut cy, &mut hm);
        }
        // Keep the caret inside the box.
        if cx - self.scroll > r.w - 4.0 {
            self.scroll = cx - r.w + 4.0;
        }
        if cx - self.scroll < 0.0 {
            self.scroll = cx.max(0.0);
        }
        g.clip(r);
        let (a, b) = self.sel();
        if a != b {
            let a16 = self.text[..a].encode_utf16().count() as u32;
            let b16 = self.text[..b].encode_utf16().count() as u32;
            for rr in crate::editor::view::range_rects(&l, a16, b16) {
                g.fill(Rect::new(r.x + rr.x - self.scroll, y + rr.y, rr.w, rr.h), th.accent_soft);
            }
        }
        if self.text.is_empty() {
            let p = g.layout(placeholder, style, 1.0e5);
            g.text(&p, r.x, y, th.text_tertiary);
        } else {
            g.text(&l, r.x - self.scroll, y, color);
        }
        let h = hm.height.max(style.size * 1.3);
        self.caret_box = Some((r.x + cx - self.scroll, y, h));
        if focused {
            let on = self.blink.is_none_or(|b| (b.elapsed().as_millis() / 530) % 2 == 0);
            if on {
                g.fill(Rect::new(r.x + cx - self.scroll, y, 1.5, h), th.accent);
            }
        }
        g.unclip();
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Normal,
    Primary,
    Danger,
    /// A toggle that is on (`aria-pressed`).
    Pressed,
}

/// `.btn`: outlined, accent border on hover; filled for the primary action.
pub fn button(ui: &mut Ui, g: &Gfx, r: Rect, hit: Hit, label: &str, kind: Btn, enabled: bool) {
    let th = app().theme;
    let h = if enabled { ui.hover_t(hit) } else { 0.0 };
    let (bg, border, fg) = match kind {
        _ if !enabled => (Color { a: 0.0, ..th.bg_base }, th.border_soft, th.text_disabled),
        Btn::Primary => {
            // Paper: the primary action is solid ink, not a coloured button.
            let bg = if ui.pressed(hit) { th.text_secondary } else { theme::mix(th.text_primary, th.text_secondary, h) };
            (bg, bg, th.bg_base)
        }
        Btn::Danger => (theme::with_alpha(theme::mix(th.danger, Color { a: 0.0, ..th.danger }, 0.16), h), th.danger, theme::mix(th.text_primary, th.danger, h)),
        Btn::Pressed => (Color { a: 0.0, ..th.bg_base }, th.accent_line, th.accent),
        Btn::Normal => (Color { a: 0.0, ..th.bg_base }, theme::mix(th.text_secondary, th.border_strong, h), theme::mix(th.text_primary, th.text_secondary, h)),
    };
    g.fill_round(r, crate::theme::RADIUS_MD, bg);
    g.stroke_round(r, crate::theme::RADIUS_MD, border, 1.0);
    let style = Style::new(crate::gfx::Stack::Ui, crate::theme::TEXT_SM).weight(400);
    let l = g.layout(label, style, r.w);
    let m = metrics(&l);
    g.text(&l, r.x + (r.w - m.w) / 2.0, r.y + (r.h - m.h) / 2.0, fg);
    if enabled {
        ui.region(r, hit);
    }
}

pub const BTN_H: f32 = crate::theme::TEXT_SM * 1.5 + crate::theme::S2 * 2.0 + 2.0;

pub fn button_width(g: &Gfx, label: &str) -> f32 {
    metrics(&g.layout(label, Style::new(crate::gfx::Stack::Ui, crate::theme::TEXT_SM), 1.0e4)).w + crate::theme::S4 * 2.0 + 2.0
}

/// Unsaved changes: the same accent dot in the title bar and the path bar.
/// `DIRTY_W` is the dot plus the gap before the text.
pub const DIRTY_W: f32 = 6.0 + crate::theme::S2;

pub fn dirty_dot(g: &Gfx, x: f32, cy: f32) {
    g.fill_round(Rect::new(x, cy - 3.0, 6.0, 6.0), 3.0, app().theme.accent);
}
