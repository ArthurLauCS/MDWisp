//! Frameless window chrome laid out the way Windows users expect: the app's
//! own controls on the left, the caption in the middle, and the window
//! buttons hard against the top-right corner.

use super::widgets::{DIRTY_W, dirty_dot};
use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::theme::{self, CLOSE_HOVER, CLOSE_PRESS, RADIUS_SM, S1, S2, S3, S4, TEXT_SM, TEXT_XS, WHITE, mix};
use crate::ui::Hit;
use crate::window::Window;
use crate::{app, win};

pub const ACTION_IDS: [&str; 6] = ["document.new", "document.open", "document.save", "workspace.open", "view.settings", "view.help"];
/// 46 px is the Windows 11 caption button metric; flush to the corner, no gaps.
const WBTN_W: f32 = 46.0;

pub fn paint(w: &mut Window, g: &Gfx, bar: Rect) {
    let th = app().theme;
    g.fill(bar, th.chrome);
    g.fill(Rect::new(bar.x, bar.bottom() - 1.0, bar.w, 1.0), th.accent_line);
    w.ui.region(bar, Hit::Caption);

    // ---- right: caption buttons
    let mut rest = bar;
    rest.h -= 1.0;
    let close = rest.cut_right(WBTN_W);
    let max = rest.cut_right(WBTN_W);
    let min = rest.cut_right(WBTN_W);
    for (r, hit) in [(min, Hit::Minimize), (max, Hit::Maximize), (close, Hit::Close)] {
        w.ui.region(r, hit);
        let h = w.ui.hover_t(hit);
        let pressed = w.ui.pressed(hit);
        let (bg, fg) = if hit == Hit::Close {
            (if pressed { CLOSE_PRESS } else { theme::with_alpha(CLOSE_HOVER, h) }, mix(WHITE, th.text_secondary, h))
        } else {
            (if pressed { th.bg_active } else { theme::with_alpha(th.bg_hover, h) }, mix(th.text_primary, th.text_secondary, h))
        };
        g.fill(r, bg);
        glyph(g, hit, r, fg, win::is_maximized(w.hwnd));
    }

    // ---- left: sidebar toggle + app actions
    let mut lead = rest;
    lead.cut_left(S3);
    let toggle = lead.cut_left(28.0);
    let tr = Rect::new(toggle.x, bar.y + (bar.h - 28.0) / 2.0, 28.0, 28.0);
    button_bg(w, g, tr, Hit::SidebarToggle);
    let tc = mix(th.text_primary, th.text_secondary, w.ui.hover_t(Hit::SidebarToggle));
    rail_icon(g, tr, tc);
    w.ui.region(tr, Hit::SidebarToggle);
    lead.cut_left(S1);

    let style = Style::new(Stack::Ui, TEXT_SM);
    for (i, id) in ACTION_IDS.iter().enumerate() {
        let l = g.layout(&crate::actions::title(id), style, 1000.0);
        let m = metrics(&l);
        let bw = m.w + S3 * 2.0;
        if lead.w < bw {
            break;
        }
        let r = lead.cut_left(bw);
        let r = Rect::new(r.x, bar.y + (bar.h - m.h - S1 * 2.0) / 2.0, bw, m.h + S1 * 2.0);
        let hit = Hit::TitleAction(i as u8);
        button_bg(w, g, r, hit);
        g.text(&l, r.x + S3, r.y + S1, mix(th.text_primary, th.text_secondary, w.ui.hover_t(hit)));
        w.ui.region(r, hit);
        lead.cut_left(S1);
    }

    // ---- centre: which document — the one thing people look at the chrome to find out
    let (left, right) = (lead.x + S4, rest.right() - S4);
    // Centred on the window when that clears the buttons, else in the space between them.
    let start = |total: f32| {
        let x = bar.x + (bar.w - total) / 2.0;
        if x >= left && x + total <= right { x } else { left + ((right - left) - total).max(0.0) / 2.0 }
    };
    match &w.doc {
        Some(d) => {
            let dir = d.meta.as_ref().and_then(|m| m.parent_dir.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let nl = g.layout(&d.name(), Style::new(Stack::Ui, TEXT_SM).weight(700), 600.0);
            let dl = g.layout(&dir, Style::new(Stack::Ui, TEXT_XS), 400.0);
            let (nm, dm) = (metrics(&nl), metrics(&dl));
            // The folder name drops out on narrow windows, like the CSS breakpoint.
            let show_dir = w.width > 900.0 && !dir.is_empty();
            let dot = if d.dirty() { DIRTY_W } else { 0.0 };
            let total = dot + nm.w + if show_dir { S2 + dm.w } else { 0.0 };
            let cx = start(total);
            if total <= right - left {
                let base = bar.y + (bar.h - nm.h) / 2.0;
                if d.dirty() {
                    dirty_dot(g, cx, base + nm.h / 2.0);
                }
                g.text(&nl, cx + dot, base, th.text_primary);
                if show_dir {
                    g.text(&dl, cx + dot + nm.w + S2, base + nm.h - dm.h - 1.0, th.text_tertiary);
                }
            }
        }
        None => {
            let name = g.layout("MDWISP", Style::new(Stack::Ui, TEXT_XS), 1000.0);
            let m = metrics(&name);
            if m.w <= right - left {
                g.text(&name, start(m.w), bar.y + (bar.h - m.h) / 2.0, th.text_tertiary);
            }
        }
    }
}

fn button_bg(w: &Window, g: &Gfx, r: Rect, hit: Hit) {
    let th = app().theme;
    let c = if w.ui.pressed(hit) { th.bg_active } else { theme::with_alpha(th.bg_hover, w.ui.hover_t(hit)) };
    g.fill_round(r, RADIUS_SM, c);
}

/// A miniature of the panel it toggles — the icon states its own meaning.
fn rail_icon(g: &Gfx, r: Rect, c: crate::theme::Color) {
    let c = theme::with_alpha(c, 0.75);
    let b = Rect::new(r.x + (r.w - 12.0) / 2.0, r.y + (r.h - 11.0) / 2.0, 12.0, 11.0);
    g.stroke_round(b, 2.0, c, 1.4);
    g.fill(Rect::new(b.x + 3.4, b.y + 2.8, 1.4, b.h - 5.6), theme::with_alpha(c, 0.6));
}

/// Segoe-style 10×10 caption glyphs, 1 px strokes snapped to pixels.
fn glyph(g: &Gfx, hit: Hit, r: Rect, c: crate::theme::Color, maximized: bool) {
    let x = (r.x + (r.w - 10.0) / 2.0).round();
    let y = (r.y + (r.h - 10.0) / 2.0).round();
    match hit {
        Hit::Minimize => g.line(x, y + 5.5, x + 10.0, y + 5.5, c, 1.0),
        Hit::Maximize if maximized => {
            g.stroke_round(Rect::new(x, y + 2.0, 8.0, 8.0), 0.0, c, 1.0);
            g.line(x + 2.5, y + 2.0, x + 2.5, y + 0.5, c, 1.0);
            g.line(x + 2.0, y + 0.5, x + 10.0, y + 0.5, c, 1.0);
            g.line(x + 9.5, y + 0.5, x + 9.5, y + 8.0, c, 1.0);
            g.line(x + 9.5, y + 7.5, x + 8.0, y + 7.5, c, 1.0);
        }
        Hit::Maximize => g.stroke_round(Rect::new(x, y, 10.0, 10.0), 0.0, c, 1.0),
        _ => {
            g.line(x, y, x + 10.0, y + 10.0, c, 1.0);
            g.line(x + 10.0, y, x, y + 10.0, c, 1.0);
        }
    }
}
