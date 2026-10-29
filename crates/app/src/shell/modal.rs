//! Overlays that own the keyboard while open: the context menu, the command
//! palette and the panels. Only one is ever on screen — a stack would let a
//! palette end up behind a settings sheet with no obvious way back.

use std::time::Instant;

use mdwisp_core::i18n::t;
use mdwisp_text::keymap::pretty_key;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::menu::{self, Menu};
use super::panels::{self, Panel};
use super::widgets::Field;
use crate::actions::{self, ACTIONS};
use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::keys::Mods;
use crate::theme::{self, Dur, Ease, RADIUS_MD, RADIUS_SM, RADIUS_XL, S2, S3, S4, S6, TEXT_BASE, TEXT_LG, TEXT_SM, TEXT_XS};
use crate::ui::Hit;
use crate::window::Window;
use crate::{app, win};

pub enum Modal {
    Menu(Menu),
    Palette(Palette),
    Panel(Panel),
}

pub struct Palette {
    pub field: Field,
    pub active: usize,
    pub scroll: f32,
    pub opened: Instant,
    results: Vec<(&'static str, String, Vec<usize>)>,
    query_seen: Option<String>,
}

impl Modal {
    pub fn palette(_w: &Window) -> Modal {
        Modal::Palette(Palette { field: Field::default(), active: 0, scroll: 0.0, opened: Instant::now(), results: vec![], query_seen: None })
    }
    pub fn settings() -> Modal {
        Modal::Panel(Panel::settings(false))
    }
    pub fn shortcuts() -> Modal {
        Modal::Panel(Panel::settings(true))
    }
    pub fn help() -> Modal {
        Modal::Panel(Panel::help())
    }
    pub fn export(w: &Window) -> Modal {
        Modal::Panel(Panel::export(w))
    }
    pub fn organize(w: &Window) -> Modal {
        Modal::Panel(Panel::organize(w))
    }
    pub fn image(w: &Window) -> Modal {
        Modal::Panel(Panel::image(w))
    }

    fn same_kind(&self, other: &Modal) -> bool {
        match (self, other) {
            (Modal::Palette(_), Modal::Palette(_)) | (Modal::Menu(_), Modal::Menu(_)) => true,
            (Modal::Panel(a), Modal::Panel(b)) => a.kind_id() == b.kind_id(),
            _ => false,
        }
    }

    /// Open `m`, or close it when the same overlay is already up.
    pub fn toggle(w: &mut Window, m: Modal) {
        if w.modal.as_ref().is_some_and(|cur| cur.same_kind(&m)) {
            w.modal = None;
        } else {
            w.modal = Some(m);
        }
        win::request_frame(w.hwnd);
    }

    pub fn text_input_at(&self, hit: Hit) -> bool {
        match self {
            Modal::Panel(p) => p.text_input_at(hit),
            _ => false,
        }
    }

    pub fn caret(&self) -> Option<(f32, f32, f32)> {
        match self {
            Modal::Palette(p) => p.field.caret_box,
            Modal::Panel(p) => p.caret(),
            Modal::Menu(_) => None,
        }
    }
}

// ---- fuzzy search -------------------------------------------------------------

/// Subsequence match with boundary and streak bonuses. Returns matched char
/// indexes so the title can show *why* a row matched.
fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, vec![]));
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let tchars: Vec<char> = text.to_lowercase().chars().collect();
    let (mut qi, mut score, mut streak) = (0, 0, 0);
    let mut hits = vec![];
    for (ti, c) in tchars.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if *c == q[qi] {
            hits.push(ti);
            streak += 1;
            score += 1 + streak * 2;
            if ti == 0 || " -_/.".contains(tchars[ti - 1]) {
                score += 6;
            }
            qi += 1;
        } else {
            streak = 0;
        }
    }
    (qi == q.len()).then_some((score, hits))
}

fn rank(w: &Window, query: &str) -> Vec<(&'static str, String, Vec<usize>)> {
    let mut scored: Vec<(f32, bool, &'static str, String, Vec<usize>)> = vec![];
    for a in ACTIONS.iter().filter(|a| actions::in_scope(w, a) && (a.enabled)(w)) {
        let title = a.title();
        let hit = fuzzy(query, &title).map(|(s, h)| (s as f32, h)).or_else(|| a.keywords.iter().filter_map(|k| fuzzy(query, k)).map(|(s, _)| s as f32 * 0.6).reduce(f32::max).map(|s| (s, vec![])));
        if let Some((s, h)) = hit {
            scored.push((s, !a.key.is_empty(), a.id, title, h));
        }
    }
    // A bound action is more likely to be what you meant than an unbound one.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    scored.into_iter().take(40).map(|(_, _, id, title, h)| (id, title, h)).collect()
}

// ---- painting -----------------------------------------------------------------

/// Dim the window behind an overlay; the fade follows the overlay's opening.
fn scrim(w: &mut Window, g: &Gfx, k: f32) {
    let th = app().theme;
    let r = w.bounds();
    g.fill(r, theme::with_alpha(th.bg_base, 0.55 * k));
    w.ui.region(r, Hit::Backdrop);
}

pub fn paint(w: &mut Window, g: &Gfx) {
    match &w.modal {
        None => {}
        Some(Modal::Menu(_)) => menu::paint(w, g),
        Some(Modal::Palette(_)) => paint_palette(w, g),
        Some(Modal::Panel(_)) => panels::paint(w, g),
    }
}

/// Opening progress, eased like `palette-in` / `panel-in` (spring).
pub fn opening(opened: Instant, w: &Window) -> (f32, f32) {
    let k = (opened.elapsed().as_secs_f32() * 1000.0 / Dur::Base.ms(w.ui.motion)).min(1.0);
    if k < 1.0 {
        win::request_frame(w.hwnd);
    }
    (k, Ease::Out.at(k))
}

pub fn panel_scrim(w: &mut Window, g: &Gfx, opened: Instant) -> (f32, f32) {
    let (k, e) = opening(opened, w);
    let fade = (k * 1.6).min(1.0);
    scrim(w, g, fade);
    (fade, e)
}

fn paint_palette(w: &mut Window, g: &Gfx) {
    let th = app().theme;
    let opened = match &w.modal {
        Some(Modal::Palette(p)) => p.opened,
        _ => return,
    };
    let (fade, _) = panel_scrim(w, g, opened);
    // Results are ranked when the query changes, not every frame.
    let query = match &w.modal {
        Some(Modal::Palette(p)) => p.field.text.clone(),
        _ => return,
    };
    let stale = matches!(&w.modal, Some(Modal::Palette(p)) if p.query_seen.as_deref() != Some(query.as_str()));
    if stale {
        let results = rank(w, &query);
        if let Some(Modal::Palette(p)) = &mut w.modal {
            p.results = results;
            p.query_seen = Some(query);
            p.active = 0;
            p.scroll = 0.0;
        }
    }
    let (ww, wh) = (w.width, w.height);
    let focused = w.focused;
    let Some(Modal::Palette(p)) = &mut w.modal else { return };
    let pw = (ww - 48.0).min(640.0);
    let row = TEXT_BASE * 1.5 + S2 * 2.0;
    let field_h = TEXT_LG * 1.5 + S4 + S3 + 1.0;
    let list_h = if p.results.is_empty() { 80.0 } else { p.results.len() as f32 * row + S2 * 2.0 };
    let ph = (field_h + list_h + 1.0).min(wh * 0.6);
    let x = (ww - pw) / 2.0;
    let y = wh * 0.18;
    let r = Rect::new(x, y, pw, ph);
    let mut rows_hit = vec![];
    g.with_opacity(fade, || {
        g.fill_round(r, RADIUS_XL, th.bg_overlay);
        g.stroke_round(r, RADIUS_XL, th.border_strong, 1.0);
        // Field.
        let fr = Rect::new(r.x + S4, r.y + S4, r.w - S4 * 2.0, TEXT_LG * 1.5);
        let hint = g.layout("Esc", Style::new(Stack::Code, TEXT_XS), 100.0);
        let hm = metrics(&hint);
        let hb = Rect::new(fr.right() - hm.w - S2 * 2.0, fr.y + (fr.h - hm.h - 4.0) / 2.0, hm.w + S2 * 2.0, hm.h + 4.0);
        g.fill_round(hb, RADIUS_SM, th.bg_inset);
        g.stroke_round(hb, RADIUS_SM, th.border_soft, 1.0);
        g.text(&hint, hb.x + S2, hb.y + 2.0, th.text_tertiary);
        p.field.paint(g, Style::new(Stack::Ui, TEXT_LG), Rect::new(fr.x, fr.y, hb.x - fr.x - S3, fr.h), th.text_primary, t("输入命令，或搜索功能…"), focused);
        g.fill(Rect::new(r.x, r.y + field_h - 1.0, r.w, 1.0), th.border_soft);
        // List.
        let list = Rect::new(r.x + S2, r.y + field_h + S2, r.w - S2 * 2.0, ph - field_h - S2 * 2.0);
        g.clip(list);
        if p.results.is_empty() {
            let l = g.layout(t("没有匹配的命令"), Style::new(Stack::Ui, TEXT_SM), list.w);
            let m = metrics(&l);
            g.text(&l, list.x + (list.w - m.w) / 2.0, list.y + S6, th.text_tertiary);
        }
        // Keep the active row visible.
        let top = p.active as f32 * row;
        if top < p.scroll {
            p.scroll = top;
        } else if top + row > p.scroll + list.h {
            p.scroll = top + row - list.h;
        }
        for (i, (id, title, hits)) in p.results.iter().enumerate() {
            let ry = list.y + i as f32 * row - p.scroll;
            if ry + row < list.y || ry > list.bottom() {
                continue;
            }
            let rr = Rect::new(list.x, ry, list.w, row);
            let on = i == p.active;
            if on {
                g.fill_round(rr, RADIUS_MD, th.accent_soft);
                g.fill(Rect::new(rr.x, rr.y, 2.0, rr.h), th.text_primary);
            }
            let key = actions::find(id).map(|a| a.key).unwrap_or("");
            let mut right = rr.right() - S3;
            if !key.is_empty() {
                let kl = g.layout(&pretty_key(key), Style::new(Stack::Code, TEXT_XS), 300.0);
                let km = metrics(&kl);
                let kb = Rect::new(right - km.w - S2 * 2.0, ry + (row - km.h - 4.0) / 2.0, km.w + S2 * 2.0, km.h + 4.0);
                g.fill_round(kb, RADIUS_SM, th.bg_inset);
                g.stroke_round(kb, RADIUS_SM, if on { th.accent_line } else { th.border_soft }, 1.0);
                g.text(&kl, kb.x + S2, kb.y + 2.0, if on { th.text_secondary } else { th.text_tertiary });
                right = kb.x - S4;
            }
            let l = g.layout(title, Style::new(Stack::Ui, TEXT_BASE), (right - rr.x - S3).max(10.0));
            // Matched characters in the accent, semi-bold.
            let mut u16pos = 0u32;
            for (ci, ch) in title.chars().enumerate() {
                let n = ch.len_utf16() as u32;
                if hits.contains(&ci) {
                    let range = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE { startPosition: u16pos, length: n };
                    unsafe {
                        let _ = l.SetDrawingEffect(&g.solid(th.accent), range);
                        let _ = l.SetFontWeight(windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_SEMI_BOLD, range);
                    }
                }
                u16pos += n;
            }
            let m = metrics(&l);
            g.text(&l, rr.x + S3, ry + (row - m.h) / 2.0, if on { th.text_primary } else { th.text_secondary });
            rows_hit.push((rr, i));
        }
        g.unclip();
    });
    w.ui.region(r, Hit::Palette);
    for (rr, i) in rows_hit {
        w.ui.region(rr, Hit::PaletteRow(i as u32));
    }
}

// ---- input ----------------------------------------------------------------------

pub fn key(w: &mut Window, vk: VIRTUAL_KEY, m: Mods) -> bool {
    let hwnd = w.hwnd;
    match &mut w.modal {
        Some(Modal::Menu(menu)) => {
            if let Some(result) = menu::key(menu, vk) {
                let pick = menu.pick;
                w.modal = None;
                if let Some(id) = result {
                    commit(w, &id, pick);
                }
            }
            true
        }
        Some(Modal::Palette(p)) => {
            match vk {
                VK_ESCAPE => w.modal = None,
                VK_DOWN => p.active = (p.active + 1).min(p.results.len().saturating_sub(1)),
                VK_UP => p.active = p.active.saturating_sub(1),
                VK_RETURN => {
                    let id = p.results.get(p.active).map(|r| r.0);
                    w.modal = None;
                    if let Some(id) = id {
                        actions::run(w, id);
                    }
                }
                _ => {
                    // App shortcuts still work with the palette open (e.g. Ctrl+P closes it).
                    if !p.field.key(vk, m, hwnd) {
                        return false;
                    }
                }
            }
            true
        }
        Some(Modal::Panel(_)) => panels::key(w, vk, m),
        None => false,
    }
}

fn commit(w: &mut Window, id: &str, pick: Option<fn(&mut Window, &str)>) {
    match pick {
        Some(f) => f(w, id),
        None => actions::run(w, id),
    }
}

pub fn text(w: &mut Window, s: &str) {
    match &mut w.modal {
        Some(Modal::Palette(p)) => p.field.insert(s),
        Some(Modal::Panel(_)) => panels::text(w, s),
        _ => {}
    }
}

pub fn pointer_down(w: &mut Window, hit: Option<Hit>, x: f32, y: f32, button: u8) -> bool {
    match &mut w.modal {
        Some(Modal::Menu(_)) => {
            if !matches!(hit, Some(Hit::MenuItem(_) | Hit::Menu(_))) {
                w.modal = None;
            }
            true
        }
        Some(Modal::Palette(_)) => {
            if hit == Some(Hit::Backdrop) {
                w.modal = None;
            }
            true
        }
        Some(Modal::Panel(_)) => panels::pointer_down(w, hit, x, y, button),
        None => false,
    }
}

pub fn click(w: &mut Window, hit: Hit) {
    match &mut w.modal {
        Some(Modal::Menu(m)) => {
            if let Some(id) = menu::item_at(m, hit) {
                let pick = m.pick;
                w.modal = None;
                commit(w, &id, pick);
            }
        }
        Some(Modal::Palette(p)) => {
            if let Hit::PaletteRow(i) = hit {
                let id = p.results.get(i as usize).map(|r| r.0);
                w.modal = None;
                if let Some(id) = id {
                    actions::run(w, id);
                }
            }
        }
        Some(Modal::Panel(_)) => panels::click(w, hit),
        None => {}
    }
}

pub fn drag(w: &mut Window, x: f32, y: f32) -> bool {
    let hit = w.ui.hit_at(x, y);
    match &mut w.modal {
        Some(Modal::Menu(m)) => {
            menu::hover(m, hit);
            true
        }
        Some(Modal::Palette(p)) => {
            if let Some(Hit::PaletteRow(i)) = hit {
                p.active = i as usize;
            }
            false
        }
        Some(Modal::Panel(_)) => panels::drag(w, x, y),
        None => false,
    }
}

pub fn drag_end(w: &mut Window) {
    if let Some(Modal::Panel(_)) = &w.modal {
        panels::drag_end(w);
    }
}

pub fn wheel(w: &mut Window, dy: f32) -> bool {
    match &mut w.modal {
        Some(Modal::Palette(p)) => {
            p.scroll = (p.scroll - dy * 60.0).max(0.0);
            true
        }
        Some(Modal::Panel(_)) => panels::wheel(w, dy),
        _ => true,
    }
}
