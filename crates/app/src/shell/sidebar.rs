//! The sidebar: files or outline on top, the document's images below, a
//! draggable split between them and a resize handle on the right edge.

use std::collections::HashSet;
use std::path::PathBuf;

use mdwisp_core::assets::AssetRef;
use mdwisp_core::i18n::t;
use mdwisp_core::workspace::{NodeKind, TreeNode};
use mdwisp_text::outline::{Heading, extract_headings, heading_at};

use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::theme::{self, NAV_ROW_H, RADIUS_MD, RADIUS_SM, S1, S2, S3, S4, S6, SIDE_PANEL_HEAD_H, TEXT_SM, TEXT_XS};
use crate::ui::Hit;
use crate::window::Window;
use crate::{app, dialog};

/// Closer than this to an edge, the part being squeezed hides instead of shrinking.
const SNAP: f32 = 64.0;

#[derive(Default)]
pub struct State {
    pub tree_scroll: f32,
    pub outline_scroll: f32,
    pub assets_scroll: f32,
    /// Folders the user toggled away from their default (top level open).
    toggled: HashSet<PathBuf>,
    collapsed: HashSet<String>,
    rows: Vec<Row>,
    filter: u8,
    assets: Option<(u64, PathBuf, Vec<AssetRef>)>,
    headings: Option<(u64, Vec<Heading>)>,
    /// Height of nav + assets area, for the split drag.
    pub split_h: f32,
    nav: Rect,
    asset_list: Rect,
    last_active: Option<String>,
}

#[derive(Clone)]
enum Row {
    Dir(PathBuf),
    File(PathBuf),
    Heading { offset: usize, id: String },
    Asset(PathBuf),
}

fn open_dir(s: &State, path: &PathBuf, depth: usize) -> bool {
    (depth < 1) != s.toggled.contains(path)
}

pub fn paint(w: &mut Window, g: &Gfx, r: Rect) {
    let th = app().theme;
    let s = app().s();
    g.fill(r, th.bg_raised);
    g.fill(Rect::new(r.right() - 1.0, r.y, 1.0, r.h), th.border_soft);
    let mut body = Rect::new(r.x, r.y, r.w - 1.0, r.h);
    w.shell.sidebar.rows.clear();

    // ---- head: tabs + subtitle
    let head = body.cut_top(SIDE_PANEL_HEAD_H);
    g.fill(Rect::new(head.x, head.bottom() - 1.0, head.w, 1.0), th.border_soft);
    let tabs = Rect::new(head.x + S4, head.y + S4, head.w - S4 * 2.0, TEXT_SM * 1.5 + S1 * 2.0 + S1 * 2.0);
    let inner = tabs.inset(S1);
    let tw = (inner.w - S1) / 2.0;
    for (i, id) in ["view.showFiles", "view.showOutline"].iter().enumerate() {
        let cell = Rect::new(inner.x + (tw + S1) * i as f32, inner.y, tw, inner.h);
        let on = (i == 1) == s.outline_visible;
        let hit = Hit::SidebarTab(i as u8);
        if on {
            g.fill(Rect::new(cell.x, cell.bottom() - 2.0, cell.w, 2.0), th.text_primary);
        }
        let l = g.layout(&crate::actions::title(id), Style::new(Stack::Ui, TEXT_SM).weight(if on { 700 } else { 400 }), cell.w);
        let m = metrics(&l);
        let c = if on { th.text_primary } else { theme::mix(th.text_primary, th.text_tertiary, w.ui.hover_t(hit)) };
        g.text(&l, cell.x + (cell.w - m.w) / 2.0, cell.y + (cell.h - m.h) / 2.0, c);
        w.ui.region(cell, hit);
    }
    let doc_path = w.doc.as_ref().and_then(|d| d.path()).map(|p| p.to_path_buf());
    let subtitle = if s.outline_visible {
        match &w.doc {
            Some(d) => d.name(),
            None => t("未打开文档").into(),
        }
    } else {
        w.workspace.current().map_or_else(|| t("未打开目录").to_string(), |ws| ws.name.clone())
    };
    let sl = g.layout(&subtitle, Style::new(Stack::Ui, TEXT_XS), head.w - S4 * 2.0);
    let gap = head.bottom() - 1.0 - tabs.bottom();
    g.clip(Rect::new(head.x + S4, tabs.bottom(), head.w - S4 * 2.0, gap));
    g.text(&sl, head.x + S4, tabs.bottom() + (gap - metrics(&sl).h) / 2.0, th.text_tertiary);
    g.unclip();

    // ---- split between navigation and images
    let has_assets = doc_path.is_some();
    w.shell.sidebar.split_h = body.h;
    let ratio = s.assets_ratio as f32;
    let collapse = if !has_assets { "" } else if ratio * body.h > body.h - SNAP { "nav" } else if ratio * body.h < SNAP { "assets" } else { "" };
    let (nav, assets) = if !has_assets {
        let note_h = draft_note_h(g, body.w);
        let note = body.cut_bottom(note_h);
        draft_note(g, note);
        (body, None)
    } else {
        match collapse {
            "nav" => (Rect::new(body.x, body.y, body.w, 0.0), Some(body)),
            "assets" => (body, Some(Rect::new(body.x, body.bottom(), body.w, 0.0))),
            _ => {
                let a = body.cut_bottom((body.h * ratio).round());
                (body, Some(a))
            }
        }
    };
    w.shell.sidebar.nav = nav;
    if nav.h > 0.0 {
        if s.outline_visible {
            outline(w, g, nav);
        } else {
            tree(w, g, nav);
        }
    }
    if let (Some(a), Some(p)) = (assets, doc_path) {
        if a.h > 0.0 {
            assets_panel(w, g, a, &p);
        }
        // The split handle overlaps the images' top border; it stays at the edge once a part is hidden.
        let y = if collapse == "assets" { a.y - S2 } else { a.y };
        let handle = Rect::new(a.x, y, a.w, S2);
        let hv = w.ui.hover_t(Hit::SidebarSplit);
        if hv > 0.0 {
            let ly = if collapse == "assets" { handle.bottom() - 2.0 } else { handle.y };
            g.fill(Rect::new(handle.x, ly, handle.w, 2.0), theme::with_alpha(th.accent, hv));
        }
        w.ui.region(handle, Hit::SidebarSplit);
    }

    // ---- resize handle
    let rh = Rect::new(r.right() - S2, r.y, S2, r.h);
    let hv = w.ui.hover_t(Hit::SidebarResize);
    if hv > 0.0 {
        g.fill(Rect::new(rh.right() - 2.0, rh.y, 2.0, rh.h), theme::with_alpha(th.accent, hv));
    }
    w.ui.region(rh, Hit::SidebarResize);
}

fn draft_note_h(g: &Gfx, w: f32) -> f32 {
    let l = g.layout(t("文字可直接输入。保存文档后即可插入本地图片，并启用自动保存与历史记录。"), note_style(), w - S4 * 2.0);
    metrics(&l).h + S4 * 2.0 + 1.0
}

fn note_style() -> Style {
    Style::new(Stack::Ui, TEXT_XS)
}

fn draft_note(g: &Gfx, r: Rect) {
    let th = app().theme;
    g.fill(Rect::new(r.x, r.y, r.w, 1.0), th.border_soft);
    let l = g.layout(t("文字可直接输入。保存文档后即可插入本地图片，并启用自动保存与历史记录。"), note_style(), r.w - S4 * 2.0);
    line_height(&l, TEXT_XS * 1.8);
    g.text(&l, r.x + S4, r.y + 1.0 + S4, th.text_tertiary);
}

fn line_height(l: &windows::Win32::Graphics::DirectWrite::IDWriteTextLayout, lh: f32) {
    unsafe {
        let _ = l.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, lh, lh * 0.8);
    }
}

/// A nav row: hover fill, active accent fill with the thin rail.
fn nav_row(w: &mut Window, g: &Gfx, r: Rect, hit: Hit, active: bool) -> crate::theme::Color {
    let th = app().theme;
    let h = w.ui.hover_t(hit);
    if active {
        g.fill_round(r, RADIUS_SM, th.accent_soft);
        g.fill(Rect::new(r.x, r.y, 2.0, r.h), th.text_primary);
    } else if h > 0.0 {
        g.fill_round(r, RADIUS_SM, theme::with_alpha(th.bg_hover, h));
    }
    w.ui.region(r, hit);
    if active { th.text_primary } else { theme::mix(th.text_primary, th.text_secondary, h) }
}

/// The tree chevron: pointing right when closed, turned 90° down when open.
fn chevron(g: &Gfx, cx: f32, cy: f32, open: f32, c: crate::theme::Color) {
    let (sin, cos) = (90.0f32 * open).to_radians().sin_cos();
    let p = |x: f32, y: f32| (cx + x * cos - y * sin, cy + x * sin + y * cos);
    let c = theme::with_alpha(c, 0.55);
    let (x0, y0) = p(-1.5, -3.0);
    let (x1, y1) = p(1.5, 0.0);
    let (x2, y2) = p(-1.5, 3.0);
    g.line(x0, y0, x1, y1, c, 1.5);
    g.line(x1, y1, x2, y2, c, 1.5);
}

fn scroll_list(g: &Gfx, r: Rect, scroll: &mut f32, content_h: f32) -> f32 {
    *scroll = scroll.clamp(0.0, (content_h - r.h).max(0.0));
    let _ = g;
    *scroll
}

fn tree(w: &mut Window, g: &Gfx, r: Rect) {
    let th = app().theme;
    let Some(info) = w.workspace.current().cloned() else {
        let l = g.layout(&format!("{}\n{}", t("还没有打开目录。"), t("用上方「打开目录」选一个装满 markdown 的文件夹。")), Style::new(Stack::Ui, TEXT_SM), r.w - S4 * 2.0);
        line_height(&l, TEXT_SM * 1.7);
        g.text(&l, r.x + S4, r.y + S6, th.text_tertiary);
        return;
    };
    let mut flat: Vec<(TreeNode, usize)> = vec![];
    fn walk(n: &TreeNode, depth: usize, s: &State, out: &mut Vec<(TreeNode, usize)>) {
        for c in n.children.iter().flatten() {
            if c.is_asset_dir {
                continue;
            }
            out.push((TreeNode { children: None, ..c.clone() }, depth));
            if c.kind == NodeKind::Dir && open_dir(s, &c.path, depth) {
                walk(c, depth + 1, s, out);
            }
        }
    }
    walk(&info.tree, 0, &w.shell.sidebar, &mut flat);
    let content = flat.len() as f32 * NAV_ROW_H + S2 + S6;
    let list = r;
    let scroll = scroll_list(g, list, &mut w.shell.sidebar.tree_scroll, content);
    let active = w.doc.as_ref().and_then(|d| d.path()).map(|p| p.to_path_buf());
    g.clip(list);
    for (i, (node, depth)) in flat.iter().enumerate() {
        let y = list.y + S2 + i as f32 * NAV_ROW_H - scroll;
        if y + NAV_ROW_H < list.y || y > list.bottom() {
            continue;
        }
        let row = Rect::new(list.x + S2, y, list.w - S2 * 2.0, NAV_ROW_H);
        let idx = w.shell.sidebar.rows.len() as u32;
        let is_dir = node.kind == NodeKind::Dir;
        let on = !is_dir && active.as_ref() == Some(&node.path);
        let c = nav_row(w, g, row, Hit::TreeRow(idx), on);
        // Same columns as the outline: a chevron slot, then every label at one x per depth.
        let cx = row.x + S2 + *depth as f32 * S3;
        let x = cx + S6;
        let label = if is_dir { node.name.clone() } else { strip_md(&node.name) };
        if is_dir {
            let open = open_dir(&w.shell.sidebar, &node.path, *depth);
            let k = w.ui.value((Hit::TreeRow(idx), 4), if open { 1.0 } else { 0.0 });
            chevron(g, cx + S6 / 2.0 + 1.0, row.y + row.h / 2.0, k, c);
            w.shell.sidebar.rows.push(Row::Dir(node.path.clone()));
        } else {
            w.shell.sidebar.rows.push(Row::File(node.path.clone()));
        }
        let l = g.layout(&label, Style::new(Stack::Ui, TEXT_SM), (row.right() - S2 - x).max(1.0));
        unsafe {
            let _ = l.SetWordWrapping(windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        let m = metrics(&l);
        g.clip(Rect::new(x, row.y, row.right() - S2 - x, row.h));
        g.text(&l, x, row.y + (row.h - m.h) / 2.0, c);
        g.unclip();
    }
    g.unclip();
}

fn strip_md(name: &str) -> String {
    let lower = name.to_lowercase();
    for e in [".md", ".markdown", ".mdx"] {
        if lower.ends_with(e) {
            return name[..name.len() - e.len()].to_string();
        }
    }
    name.to_string()
}

fn outline(w: &mut Window, g: &Gfx, r: Rect) {
    let th = app().theme;
    let Some(d) = &w.doc else { return };
    let version = d.view.buf.version;
    if w.shell.sidebar.headings.as_ref().is_none_or(|(v, _)| *v != version) {
        w.shell.sidebar.headings = Some((version, extract_headings(&d.view.buf.text)));
    }
    let headings = w.shell.sidebar.headings.as_ref().unwrap().1.clone();
    if headings.is_empty() {
        let l = g.layout(t("这份文档还没有标题。添加标题后会在这里按层级显示。"), Style::new(Stack::Ui, TEXT_SM), r.w - S4 * 2.0);
        line_height(&l, TEXT_SM * 1.7);
        g.text(&l, r.x + S4, r.y + S6, th.text_tertiary);
        return;
    }
    let cursor = d.view.buf.sel.head;
    let active = heading_at(&headings, cursor).map(|h| h.id.clone());
    // Collapsed parents of the heading under the caret open up so it stays visible.
    if active != w.shell.sidebar.last_active {
        if let Some(a) = &active {
            let i = headings.iter().position(|h| &h.id == a).unwrap_or(0);
            let mut depth = headings[i].depth;
            for h in headings[..i].iter().rev() {
                if depth == 0 {
                    break;
                }
                if h.depth < depth {
                    w.shell.sidebar.collapsed.remove(&h.id);
                    depth = h.depth;
                }
            }
        }
    }
    let mut visible: Vec<(usize, bool)> = vec![];
    let mut hide_below: Option<usize> = None;
    for (i, h) in headings.iter().enumerate() {
        if let Some(dp) = hide_below {
            if h.depth > dp {
                continue;
            }
            hide_below = None;
        }
        let has_children = headings.get(i + 1).is_some_and(|n| n.depth > h.depth);
        visible.push((i, has_children));
        if has_children && w.shell.sidebar.collapsed.contains(&h.id) {
            hide_below = Some(h.depth);
        }
    }
    let content = visible.len() as f32 * NAV_ROW_H + S2 + S6;
    if active != w.shell.sidebar.last_active {
        if let Some(pos) = visible.iter().position(|(i, _)| Some(&headings[*i].id) == active.as_ref()) {
            let y = pos as f32 * NAV_ROW_H;
            let sc = &mut w.shell.sidebar.outline_scroll;
            if y < *sc {
                *sc = y;
            } else if y + NAV_ROW_H > *sc + r.h - S6 {
                *sc = y + NAV_ROW_H - r.h + S6;
            }
        }
        w.shell.sidebar.last_active = active.clone();
    }
    let scroll = scroll_list(g, r, &mut w.shell.sidebar.outline_scroll, content);
    g.clip(r);
    for (n, (i, has_children)) in visible.iter().enumerate() {
        let h = &headings[*i];
        let y = r.y + S2 + n as f32 * NAV_ROW_H - scroll;
        if y + NAV_ROW_H < r.y || y > r.bottom() {
            continue;
        }
        let row = Rect::new(r.x + S2, y, r.w - S2 * 2.0, NAV_ROW_H);
        let idx = w.shell.sidebar.rows.len() as u32;
        let on = active.as_ref() == Some(&h.id);
        let c = nav_row(w, g, row, Hit::OutlineRow(idx), on);
        let x = row.x + S2 + h.depth as f32 * S3;
        if *has_children {
            let open = !w.shell.sidebar.collapsed.contains(&h.id);
            let k = w.ui.value((Hit::OutlineRow(idx), 4), if open { 1.0 } else { 0.0 });
            chevron(g, x + S6 / 2.0 + 1.0, row.y + row.h / 2.0, k, c);
        }
        let text_x = x + S6;
        let level = g.layout(&format!("H{}", h.level), Style::new(Stack::Code, TEXT_XS), 60.0);
        let lm = metrics(&level);
        g.text(&level, row.right() - S2 - lm.w, row.y + (row.h - lm.h) / 2.0, th.text_tertiary);
        let label = if h.text.is_empty() { t("未命名标题").to_string() } else { h.text.clone() };
        let l = g.layout(&label, Style::new(Stack::Ui, TEXT_SM), 1.0e4);
        unsafe {
            let _ = l.SetWordWrapping(windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        let m = metrics(&l);
        g.clip(Rect::new(text_x, row.y, row.right() - S2 - lm.w - S2 - text_x, row.h));
        g.text(&l, text_x, row.y + (row.h - m.h) / 2.0, c);
        g.unclip();
        w.shell.sidebar.rows.push(Row::Heading { offset: h.offset, id: h.id.clone() });
        if *has_children {
            // The chevron is its own target: it folds without jumping.
            let tr = Rect::new(x, row.y, S6, row.h);
            w.ui.region(tr, Hit::OutlineRow(idx | 0x8000_0000));
        }
    }
    g.unclip();
}

fn assets_panel(w: &mut Window, g: &Gfx, r: Rect, doc: &PathBuf) {
    let th = app().theme;
    g.fill(Rect::new(r.x, r.y, r.w, 1.0), th.border_soft);
    let version = w.doc.as_ref().map_or(0, |d| d.view.buf.version);
    let stale = w.shell.sidebar.assets.as_ref().is_none_or(|(v, p, _)| *v != version || p != doc);
    if stale {
        let text = w.doc.as_ref().map_or(String::new(), |d| d.view.buf.text.clone());
        w.shell.sidebar.assets = Some((version, doc.clone(), mdwisp_core::assets::refs(doc, &text)));
    }
    let refs = w.shell.sidebar.assets.as_ref().unwrap().2.clone();
    // Head.
    let title = g.layout(t("本文图片"), Style::new(Stack::Ui, TEXT_XS).weight(700), 400.0);
    let tm = metrics(&title);
    let hy = r.y + 1.0 + S4;
    g.text(&title, r.x + S4, hy, th.text_tertiary);
    let count = g.layout(&refs.len().to_string(), Style::new(Stack::Ui, TEXT_XS), 100.0);
    g.text(&count, r.x + S4 + tm.w + S2, hy, th.text_disabled);
    // Filters.
    let fy = hy + tm.h + S2;
    let filters = Rect::new(r.x + S4, fy, r.w - S4 * 2.0, TEXT_XS * 1.5 + S1 * 2.0 + 4.0);
    g.fill_round(filters, RADIUS_MD, th.bg_inset);
    let inner = filters.inset(2.0);
    // Each segment is its label plus an even share of the room left, so no label wraps.
    let labels = [t("全部"), t("缺失"), t("文件夹外")].map(|s| g.layout(s, Style::new(Stack::Ui, TEXT_XS), 1.0e4));
    let spare = (inner.w - 4.0 - labels.iter().map(|l| metrics(l).w).sum::<f32>()) / 3.0;
    let mut cx = inner.x;
    for (i, l) in labels.iter().enumerate() {
        let m = metrics(l);
        let cell = Rect::new(cx, inner.y, m.w + spare, inner.h);
        cx = cell.right() + 2.0;
        let hit = Hit::AssetFilter(i as u8);
        let on = w.shell.sidebar.filter == i as u8;
        if on {
            g.fill_round(cell, RADIUS_SM, th.bg_overlay);
        }
        let c = if on { th.text_primary } else { theme::mix(th.text_primary, th.text_tertiary, w.ui.hover_t(hit)) };
        g.text(l, cell.x + (cell.w - m.w) / 2.0, cell.y + (cell.h - m.h) / 2.0, c);
        w.ui.region(cell, hit);
    }
    // List.
    let list = Rect::new(r.x, filters.bottom() + S2, r.w, (r.bottom() - filters.bottom() - S2).max(0.0));
    w.shell.sidebar.asset_list = list;
    let shown: Vec<&AssetRef> = refs
        .iter()
        .filter(|a| match w.shell.sidebar.filter {
            1 => !a.exists,
            2 => !a.inside_doc_folder,
            _ => true,
        })
        .collect();
    if shown.is_empty() {
        let msg = match w.shell.sidebar.filter {
            1 => t("没有缺失的图片，所有引用都能找到文件。"),
            2 => t("没有引用文档文件夹以外的图片。"),
            _ => t("这篇文档还没有图片。把图片拖进编辑器，或按 Ctrl+Shift+I 插入。"),
        };
        let l = g.layout(msg, Style::new(Stack::Ui, TEXT_SM), list.w - S4 * 2.0);
        line_height(&l, TEXT_SM * 1.7);
        g.clip(list);
        g.text(&l, list.x + S4, list.y + S4, th.text_tertiary);
        g.unclip();
        return;
    }
    let row_h = 36.0 + S2 * 2.0;
    let content = shown.len() as f32 * row_h + S6;
    let scroll = scroll_list(g, list, &mut w.shell.sidebar.assets_scroll, content);
    let dir = w.doc.as_ref().and_then(|d| d.view.doc_dir.clone());
    g.clip(list);
    for (i, a) in shown.iter().enumerate() {
        let y = list.y + i as f32 * row_h - scroll;
        if y + row_h < list.y || y > list.bottom() {
            continue;
        }
        let row = Rect::new(list.x + S2, y, list.w - S2 * 2.0, row_h);
        let idx = w.shell.sidebar.rows.len() as u32;
        let hit = Hit::AssetRow(idx);
        let h = w.ui.hover_t(hit);
        if h > 0.0 {
            g.fill_round(row, RADIUS_SM, theme::with_alpha(th.bg_hover, h));
        }
        w.ui.region(row, hit);
        let thumb = Rect::new(row.x + S2, row.y + S2, 36.0, 36.0);
        g.fill_round(thumb, RADIUS_SM, th.bg_inset);
        if a.exists {
            if let Some(d) = &w.doc {
                d.view.images.size(dir.as_deref(), &a.abs_path.to_string_lossy());
                d.view.images.draw_cover(g, &a.abs_path, thumb);
            }
        } else {
            let l = g.layout("⚠", Style::new(Stack::Ui, TEXT_SM), 36.0);
            let m = metrics(&l);
            g.text(&l, thumb.x + (36.0 - m.w) / 2.0, thumb.y + (36.0 - m.h) / 2.0, th.warning);
        }
        g.stroke_round(thumb, RADIUS_SM, th.border_soft, 1.0);
        let bx = thumb.right() + S3;
        let bw = (row.right() - S2 - bx).max(1.0);
        let decoded = mdwisp_core::paths::decode_uri(&a.rel_path);
        let name = decoded.rsplit('/').next().unwrap_or(&decoded).to_string();
        let nl = g.elide(&name, Style::new(Stack::Ui, TEXT_SM), bw, false);
        // Long paths keep their end visible.
        let pl = g.elide(&a.rel_path, Style::new(Stack::Code, TEXT_XS), bw, true);
        let (nm, pm) = (metrics(&nl), metrics(&pl));
        let ty = row.y + (row.h - nm.h - pm.h - 1.0) / 2.0;
        g.text(&nl, bx, ty, if a.exists { th.text_primary } else { th.text_secondary });
        g.text(&pl, bx, ty + nm.h + 1.0, th.text_tertiary);
        w.shell.sidebar.rows.push(Row::Asset(a.abs_path.clone()));
    }
    g.unclip();
}

pub fn pointer_down(_w: &mut Window, _hit: Option<Hit>, _x: f32, _y: f32) -> bool {
    false
}

pub fn click(w: &mut Window, hit: Hit) {
    match hit {
        Hit::SidebarTab(0) => crate::actions::run(w, "view.showFiles"),
        Hit::SidebarTab(1) => crate::actions::run(w, "view.showOutline"),
        Hit::AssetFilter(i) => w.shell.sidebar.filter = i,
        Hit::OutlineRow(i) if i & 0x8000_0000 != 0 => {
            if let Some(Row::Heading { id, .. }) = w.shell.sidebar.rows.get((i & 0x7fff_ffff) as usize).cloned() {
                let set = &mut w.shell.sidebar.collapsed;
                let open = set.remove(&id);
                if !open {
                    set.insert(id);
                }
                let key = (Hit::OutlineRow(i & 0x7fff_ffff), 4);
                w.ui.animate(key, if open { 1.0 } else { 0.0 }, crate::theme::Dur::Fast, crate::theme::Ease::Out);
            }
        }
        Hit::TreeRow(i) | Hit::OutlineRow(i) | Hit::AssetRow(i) => match w.shell.sidebar.rows.get(i as usize).cloned() {
            Some(Row::Dir(p)) => {
                let set = &mut w.shell.sidebar.toggled;
                if !set.remove(&p) {
                    set.insert(p);
                }
                let k = w.ui.value((hit, 4), 0.0);
                w.ui.animate((hit, 4), if k > 0.5 { 0.0 } else { 1.0 }, crate::theme::Dur::Fast, crate::theme::Ease::Out);
            }
            Some(Row::File(p)) => {
                if w.doc.as_ref().and_then(|d| d.path()) != Some(p.as_path()) {
                    w.open_in_new_window(Some(p), String::new());
                }
            }
            Some(Row::Heading { offset, .. }) => {
                if let Some(d) = &mut w.doc {
                    let rest = &d.view.buf.text[offset..];
                    let prefix = rest.len() - rest.trim_start_matches('#').trim_start_matches([' ', '\t']).len();
                    let at = offset + if rest.starts_with('#') { prefix } else { 0 };
                    d.view.move_to(at, false);
                    d.view.scroll_to_line_top(at);
                    d.view.focused = true;
                }
            }
            Some(Row::Asset(p)) => dialog::reveal(&p),
            None => {}
        },
        _ => {}
    }
}

pub fn wheel(w: &mut Window, x: f32, y: f32, dy: f32) -> bool {
    let s = &mut w.shell.sidebar;
    if s.nav.contains(x, y) {
        if app().s().outline_visible {
            s.outline_scroll = (s.outline_scroll - dy * 60.0).max(0.0);
        } else {
            s.tree_scroll = (s.tree_scroll - dy * 60.0).max(0.0);
        }
        return true;
    }
    if s.asset_list.contains(x, y) {
        s.assets_scroll = (s.assets_scroll - dy * 60.0).max(0.0);
        return true;
    }
    false
}
