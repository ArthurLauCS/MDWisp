//! Context menu, built from the action registry, with one level of submenu.
//! Also serves plain choice lists (the code-language picker).

use std::time::Instant;

use mdwisp_core::i18n::t;
use mdwisp_text::keymap::pretty_key;

use crate::actions::{self, ACTIONS, GROUPS, Scope};
use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::theme::{self, Dur, Ease, RADIUS_LG, RADIUS_SM, S1, S2, S3, S4, TEXT_SM, TEXT_XS};
use crate::ui::Hit;
use crate::window::Window;
use crate::app;

#[derive(Clone)]
pub enum Entry {
    Item { id: String, title: String, key: String, disabled: bool },
    Sep,
    Sub { title: String, items: Vec<Entry> },
}

pub struct Menu {
    pub x: f32,
    pub y: f32,
    pub entries: Vec<Entry>,
    pub active: Option<usize>,
    /// Open submenu: (entry index, its active row).
    pub sub: Option<(usize, Option<usize>)>,
    pub opened: Instant,
    /// What committing an item does: run an action, or pick a value.
    pub pick: Option<fn(&mut Window, &str)>,
    rects: Vec<Rect>,
    sub_rects: Vec<Rect>,
}

const W: f32 = 260.0;

fn row_h() -> f32 {
    TEXT_SM * 1.5 + S2 * 2.0
}

impl Menu {
    pub fn new(x: f32, y: f32, entries: Vec<Entry>) -> Menu {
        Menu { x, y, entries, active: None, sub: None, opened: Instant::now(), pick: None, rects: vec![], sub_rects: vec![] }
    }
}

/// Which scopes a right-click reaches: a superset of the caret's own, so a
/// right-click on a table cell still offers what people expect.
fn scopes_for(target: Scope) -> &'static [Scope] {
    use Scope::*;
    match target {
        Image => &[Image, App],
        Link => &[Link, App],
        Table => &[Table, App],
        CodeBlock => &[CodeBlock, App],
        Find => &[Find, Document, App],
        _ => &[Document, App],
    }
}

pub fn build(w: &Window, target: Scope) -> Vec<Entry> {
    let allowed = scopes_for(target);
    let mut entries: Vec<Entry> = vec![];
    let mut headings: Vec<Entry> = vec![];
    for (group, _) in GROUPS {
        let items: Vec<Entry> = ACTIONS
            .iter()
            .filter(|a| a.group == group && allowed.contains(&a.scope))
            .map(|a| Entry::Item { id: a.id.into(), title: a.title(), key: if a.key.is_empty() { String::new() } else { pretty_key(a.key) }, disabled: !(a.enabled)(w) })
            .collect();
        let (h, rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|e| matches!(e, Entry::Item { id, .. } if id.starts_with("heading.")));
        headings.extend(h);
        if rest.is_empty() {
            continue;
        }
        if !entries.is_empty() {
            entries.push(Entry::Sep);
        }
        entries.extend(rest);
        // The "convert to" family reads better as a submenu than a dozen siblings.
        if group == "convert" && !headings.is_empty() {
            entries.push(Entry::Sub { title: t("转换为").into(), items: std::mem::take(&mut headings) });
        }
    }
    entries
}

/// Scope of a right-click on the editor at the caret.
pub fn target(w: &Window) -> Scope {
    if actions::link_at(w).is_some() {
        Scope::Link
    } else if w.image_focused() {
        Scope::Image
    } else if actions::in_code(w) {
        Scope::CodeBlock
    } else if actions::in_table(w) {
        Scope::Table
    } else if !actions::sel(w).empty() {
        Scope::Find
    } else {
        Scope::Document
    }
}

pub fn open_context(w: &mut Window, hit: Option<Hit>, x: f32, y: f32) {
    let scope = match hit {
        Some(Hit::Editor) => target(w),
        _ => Scope::App,
    };
    let entries = build(w, scope);
    w.modal = Some(super::Modal::Menu(Menu::new(x, y, entries)));
}

fn size(entries: &[Entry]) -> (f32, f32) {
    let h: f32 = entries.iter().map(|e| if matches!(e, Entry::Sep) { S1 * 2.0 + 1.0 } else { row_h() }).sum();
    (W, h + S1 * 2.0)
}

/// Keep a box inside the window, flipping to the other side of the anchor if needed.
fn place(x: f32, y: f32, w: f32, h: f32, win_w: f32, win_h: f32) -> (f32, f32) {
    let x = if x + w > win_w - 4.0 { (x - w).max(4.0) } else { x };
    let y = if y + h > win_h - 4.0 { (win_h - 4.0 - h).max(4.0) } else { y };
    (x, y)
}

pub fn paint(w: &mut Window, g: &Gfx) {
    let (ww, wh) = (w.width, w.height);
    let Some(super::Modal::Menu(m)) = &mut w.modal else { return };
    let (mw, mh) = size(&m.entries);
    let mh = mh.min(wh * 0.7).min(560.0);
    let (x, y) = place(m.x, m.y, mw, mh, ww, wh);
    let k = (m.opened.elapsed().as_secs_f32() * 1000.0 / Dur::Fast.ms(w.ui.motion)).min(1.0);
    let e = Ease::Out.at(k);
    let r = Rect::new(x, y - 4.0 * (1.0 - e), mw, mh);
    let rects = paint_list(g, r, &m.entries, m.active, e);
    m.rects = rects;
    w.ui.region(Rect::new(0.0, 0.0, ww, wh), Hit::Backdrop);
    w.ui.region(r, Hit::Menu(0));
    for (i, rr) in m.rects.iter().enumerate() {
        w.ui.region(*rr, Hit::MenuItem(i as u32));
    }
    // Submenu: its own floating panel beside its row, avoiding the window edges.
    if let Some((si, sa)) = m.sub {
        if let (Some(Entry::Sub { items, .. }), Some(anchor)) = (m.entries.get(si).cloned(), m.rects.get(si).copied()) {
            let (sw, sh) = size(&items);
            let sx = if r.right() + sw > ww - 4.0 { r.x - sw + 2.0 } else { r.right() - 2.0 };
            let (sx, sy) = place(sx, anchor.y - S1, sw, sh, ww, wh);
            let sr = Rect::new(sx, sy, sw, sh);
            let rects = paint_list(g, sr, &items, sa, 1.0);
            m.sub_rects = rects;
            w.ui.region(sr, Hit::Menu(1));
            for (i, rr) in m.sub_rects.iter().enumerate() {
                w.ui.region(*rr, Hit::MenuItem(1000 + i as u32));
            }
        }
    }
    if k < 1.0 {
        crate::win::request_frame(w.hwnd);
    }
}

fn paint_list(g: &Gfx, r: Rect, entries: &[Entry], active: Option<usize>, opacity: f32) -> Vec<Rect> {
    let th = app().theme;
    let mut rects = vec![];
    g.with_opacity(opacity, || {
        g.shadow(r, RADIUS_LG, 24.0, 10.0, 0.46 * th.shadow);
        g.fill_round(r, RADIUS_LG, th.bg_overlay);
        g.stroke_round(r, RADIUS_LG, th.border_strong, 1.0);
        g.clip(r.inset(1.0));
        let mut y = r.y + S1;
        for (i, e) in entries.iter().enumerate() {
            match e {
                Entry::Sep => {
                    g.fill(Rect::new(r.x + S3, y + S1, r.w - S3 * 2.0, 1.0), th.border_soft);
                    rects.push(Rect::default());
                    y += S1 * 2.0 + 1.0;
                }
                _ => {
                    let (title, key, disabled) = match e {
                        Entry::Item { title, key, disabled, .. } => (title.as_str(), key.as_str(), *disabled),
                        Entry::Sub { title, .. } => (title.as_str(), "", false),
                        Entry::Sep => unreachable!(),
                    };
                    let row = Rect::new(r.x + S1, y, r.w - S1 * 2.0, row_h());
                    let on = active == Some(i) && !disabled;
                    if on {
                        g.fill_round(row, RADIUS_SM, th.accent_soft);
                    }
                    let fg = if disabled { th.text_disabled } else if on { th.text_primary } else { th.text_secondary };
                    let mut right = row.right() - S3;
                    if matches!(e, Entry::Sub { .. }) {
                        let cy = row.y + row.h / 2.0;
                        let c = theme::with_alpha(fg, 0.5);
                        g.line(right - 6.0, cy - 3.5, right - 2.5, cy, c, 1.4);
                        g.line(right - 2.5, cy, right - 6.0, cy + 3.5, c, 1.4);
                        right -= 12.0;
                    } else if !key.is_empty() {
                        let kl = g.layout(key, Style::new(Stack::Code, TEXT_XS), 400.0);
                        let km = metrics(&kl);
                        g.text(&kl, right - km.w, row.y + (row.h - km.h) / 2.0, if on { th.text_secondary } else { th.text_tertiary });
                        right -= km.w + S4;
                    }
                    let l = g.layout(title, Style::new(Stack::Ui, TEXT_SM), (right - row.x - S3).max(10.0));
                    let lm = metrics(&l);
                    g.clip(Rect::new(row.x, row.y, right - row.x, row.h));
                    g.text(&l, row.x + S3, row.y + (row.h - lm.h) / 2.0, fg);
                    g.unclip();
                    rects.push(if disabled { Rect::default() } else { row });
                    y += row_h();
                }
            }
        }
        g.unclip();
    });
    rects
}

/// Hover tracking: the row under the pointer becomes active; a submenu row opens its submenu.
pub fn hover(m: &mut Menu, hit: Option<Hit>) {
    match hit {
        Some(Hit::MenuItem(i)) if i >= 1000 => {
            if let Some((si, _)) = m.sub {
                m.sub = Some((si, Some(i as usize - 1000)));
            }
        }
        Some(Hit::MenuItem(i)) => {
            let i = i as usize;
            m.active = Some(i);
            if matches!(m.entries.get(i), Some(Entry::Sub { .. })) {
                if m.sub.map(|s| s.0) != Some(i) {
                    m.sub = Some((i, None));
                }
            } else if !matches!(hit, Some(Hit::Menu(1))) {
                m.sub = None;
            }
        }
        _ => {}
    }
}

/// The id under `hit`, if it is a runnable item.
pub fn item_at(m: &Menu, hit: Hit) -> Option<String> {
    let Hit::MenuItem(i) = hit else { return None };
    let entry = if i >= 1000 {
        let (si, _) = m.sub?;
        match m.entries.get(si)? {
            Entry::Sub { items, .. } => items.get(i as usize - 1000)?.clone(),
            _ => return None,
        }
    } else {
        m.entries.get(i as usize)?.clone()
    };
    match entry {
        Entry::Item { id, disabled: false, .. } => Some(id),
        _ => None,
    }
}

/// Keyboard: arrows move, Right/Left enter and leave the submenu, Enter runs.
pub fn key(m: &mut Menu, vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> Option<Option<String>> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let nav = |entries: &[Entry]| -> Vec<usize> {
        entries.iter().enumerate().filter(|(_, e)| matches!(e, Entry::Sub { .. } | Entry::Item { disabled: false, .. })).map(|(i, _)| i).collect()
    };
    let step = |list: &[usize], cur: Option<usize>, d: i32| -> Option<usize> {
        if list.is_empty() {
            return None;
        }
        let pos = cur.and_then(|c| list.iter().position(|x| *x == c));
        let n = list.len() as i32;
        let next = match pos {
            Some(p) => (p as i32 + d).rem_euclid(n),
            None => if d > 0 { 0 } else { n - 1 },
        };
        Some(list[next as usize])
    };
    if let Some((si, sa)) = m.sub.filter(|s| s.1.is_some()) {
        let Some(Entry::Sub { items, .. }) = m.entries.get(si).cloned() else { return None };
        let list = nav(&items);
        match vk {
            VK_DOWN => m.sub = Some((si, step(&list, sa, 1))),
            VK_UP => m.sub = Some((si, step(&list, sa, -1))),
            VK_LEFT => m.sub = Some((si, None)),
            VK_RETURN => {
                if let Some(Entry::Item { id, .. }) = sa.and_then(|a| items.get(a)) {
                    return Some(Some(id.clone()));
                }
            }
            VK_ESCAPE => return Some(None),
            _ => {}
        }
        return None;
    }
    let list = nav(&m.entries);
    match vk {
        VK_DOWN => m.active = step(&list, m.active, 1),
        VK_UP => m.active = step(&list, m.active, -1),
        VK_RIGHT | VK_RETURN if matches!(m.active.and_then(|a| m.entries.get(a)), Some(Entry::Sub { .. })) => {
            let a = m.active.unwrap();
            if let Some(Entry::Sub { items, .. }) = m.entries.get(a) {
                m.sub = Some((a, nav(items).first().copied()));
            }
        }
        VK_RETURN => {
            if let Some(Entry::Item { id, .. }) = m.active.and_then(|a| m.entries.get(a)) {
                return Some(Some(id.clone()));
            }
        }
        VK_ESCAPE => return Some(None),
        _ => {}
    }
    m.sub = m.sub.filter(|(si, _)| Some(*si) == m.active);
    None
}
