//! The editor pane: path bar on top, the editor, the status line, the toast.

use mdwisp_core::i18n::{t, tf};
use mdwisp_text::ops::count_words;

use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::theme::{self, RADIUS_MD, RADIUS_SM, S1, S2, S3, S4, S6, TEXT_SM, TEXT_XS};
use crate::ui::Hit;
use crate::window::Window;
use crate::app;

pub const TOOLBAR_H: f32 = 38.0;

pub fn status_h() -> f32 {
    TEXT_XS * 1.5 + S1 * 2.0 + 1.0
}

pub fn paint(w: &mut Window, g: &Gfx, mut r: Rect) {
    let th = app().theme;
    let Some(d) = &w.doc else { return };

    // ---- path bar
    let bar = r.cut_top(TOOLBAR_H);
    g.fill(Rect::new(bar.x, bar.bottom() - 1.0, bar.w, 1.0), th.border_soft);
    let mut x = bar.x + S4;
    let mid = bar.y + (bar.h - 1.0) / 2.0;
    let xs = Style::new(Stack::Ui, TEXT_XS);
    if d.dirty() {
        super::widgets::dirty_dot(g, x, mid);
        x += super::widgets::DIRTY_W;
    }
    let state = if d.meta.is_none() { t("尚未保存") } else if d.dirty() { t("有未保存修改") } else { t("已保存") };
    let sl = g.layout(state, xs, 400.0);
    let sm = metrics(&sl);
    let right = bar.right() - S4 - sm.w;
    g.text(&sl, right, mid - sm.h / 2.0, th.text_tertiary);
    let path = match &d.meta {
        Some(m) => format!("{} / {}", m.parent_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), m.stem),
        None => t("未命名 · 首次保存时选择位置").to_string(),
    };
    let mut avail = right - x - S4;
    let loose = d.meta.as_ref().is_some_and(|m| !m.in_folder);
    let loose_l = loose.then(|| g.layout(t("散装文件 · 整理为文档文件夹"), xs, 1000.0));
    let loose_w = loose_l.as_ref().map_or(0.0, |l| metrics(l).w + S2 * 2.0 + S2);
    avail -= loose_w;
    // Long paths keep their end visible: the document name matters more than the folder.
    let pl = g.elide(&path, xs, avail, true);
    let pm = metrics(&pl);
    g.text(&pl, x, mid - pm.h / 2.0, th.text_tertiary);
    if let Some(l) = loose_l {
        let lm = metrics(&l);
        let bx = x + pm.w + S2;
        let br = Rect::new(bx, mid - lm.h / 2.0 - 1.0, lm.w + S2 * 2.0, lm.h + 2.0);
        g.fill_round(br, RADIUS_SM, th.accent_soft);
        let c = theme::mix(th.accent_hover, th.accent, w.ui.hover_t(Hit::Breadcrumb(1)));
        g.text(&l, br.x + S2, br.y + 1.0, c);
        w.ui.region(br, Hit::Breadcrumb(1));
    }

    // ---- status line
    let status = r.cut_bottom(status_h());
    g.fill(Rect::new(status.x, status.y, status.w, 1.0), th.border_soft);
    let Some(d) = &mut w.doc else { return };
    let buf = &d.view.buf;
    let line = buf.line_number(buf.sel.head);
    let words = d.words();
    let sel = buf.sel;
    let count = if sel.empty() { tf("{0} 字", &[&words]) } else { tf("已选 {0} / 共 {1} 字", &[&count_words(&buf.text[sel.from()..sel.to()]), &words]) };
    let mid = status.y + 1.0 + (status.h - 1.0) / 2.0;
    let mut sx = status.x + S4;
    for s in [tf("第 {0} 行", &[&line]), count] {
        let l = g.layout(&s, xs, 1000.0);
        g.text(&l, sx, mid - metrics(&l).h / 2.0, th.text_tertiary);
        sx += metrics(&l).w + S4;
    }
    let s = app().s();
    let mode = if s.read_only { t("只读") } else if s.source_mode { t("源码模式") } else { t("直接编辑") };
    let l = g.layout(mode, xs, 1000.0);
    g.text(&l, status.right() - S4 - metrics(&l).w, mid - metrics(&l).h / 2.0, th.text_tertiary);

    // ---- find bar
    if w.doc.as_ref().is_some_and(|d| d.view.search.is_some()) {
        let h = search_bar_h();
        let bar = r.cut_top(h);
        search_bar(w, g, bar);
    }

    // ---- editor
    if let Some(e) = &w.error {
        let l = g.layout(e, Style::new(Stack::Ui, TEXT_SM), r.w - S4 * 2.0);
        let h = metrics(&l).h + S2 * 2.0;
        let banner = r.cut_top(h);
        g.fill(banner, th.bg_raised);
        g.text(&l, banner.x + S4, banner.y + S2, th.danger);
    }
    // The thumb fades in while the pointer is over the editor, like the CSS overlay scrollbar.
    let shown = w.ui.hover_t(Hit::Editor).max(w.ui.hover_t(Hit::EditorScrollbar)).max(w.ui.value((Hit::EditorScrollbar, 5), 0.0));
    let d = w.doc.as_mut().unwrap();
    d.view.scrollbar_alpha = shown;
    d.view.paint(g, &th, r);
    w.ui.region(r, Hit::Editor);
    let hits = std::mem::take(&mut d.view.widget_hits);
    for (rr, id) in &hits {
        w.ui.region(*rr, Hit::Widget(*id));
    }
    if let Some(d) = &mut w.doc {
        d.view.widget_hits = hits;
    }
    if let Some(sb) = w.doc.as_ref().and_then(|d| d.view.scrollbar) {
        w.ui.region(sb, Hit::EditorScrollbar);
    }
    super::margins(w, g, r);
    table_toolbar(w, g, r);
}

const FIELD_H: f32 = TEXT_SM * 1.5 + S1 * 2.0 + 2.0;

fn search_bar_h() -> f32 {
    let rows = if app().s().read_only { 1.0 } else { 2.0 };
    S2 * 2.0 + rows * (FIELD_H + S1 * 2.0) + 1.0
}

/// CodeMirror's search panel: find row, replace row, option toggles, close.
fn search_bar(w: &mut Window, g: &Gfx, bar: Rect) {
    use crate::shell::widgets::{Btn, button, button_width};
    let th = app().theme;
    g.fill(bar, th.bg_raised);
    g.fill(Rect::new(bar.x, bar.bottom() - 1.0, bar.w, 1.0), th.border_soft);
    let focused = w.focused;
    let read_only = app().s().read_only;
    let Some(d) = &mut w.doc else { return };
    let Some(s) = &mut d.view.search else { return };
    let sm = Style::new(Stack::Ui, TEXT_SM);
    let count = if s.invalid { "✕".to_string() } else { s.matches.len().to_string() };
    let mut regions: Vec<(Rect, Hit)> = vec![];
    let top = bar.y + S2 + S1;
    let mut y = top;
    for row in 0..if read_only { 1 } else { 2 } {
        let mut x = bar.x + S3 + S1;
        let fw = 220.0f32.min(bar.w * 0.35);
        let fr = Rect::new(x, y, fw, FIELD_H);
        let on = s.focus == Some(row);
        g.fill_round(fr, RADIUS_SM, th.bg_inset);
        g.stroke_round(fr, RADIUS_SM, if on { th.accent } else { th.border_strong }, 1.0);
        let (field, placeholder) = if row == 0 { (&mut s.find, t("查找")) } else { (&mut s.replace, t("替换")) };
        field.paint(g, sm, fr.pad(S2, 0.0), th.text_primary, placeholder, on && focused);
        regions.push((fr, Hit::Status(10 + row)));
        x = fr.right() + S2;
        let buttons: Vec<(&str, u8)> = if row == 0 {
            vec![(t("下一个"), 20), (t("上一个"), 21)]
        } else {
            vec![(t("替换"), 22), (t("全部替换"), 23)]
        };
        for (label, id) in buttons {
            let bw = button_width(g, label) - S2;
            let br = Rect::new(x, y, bw, FIELD_H);
            button(&mut w.ui, g, br, Hit::Status(id), label, Btn::Normal, true);
            x += bw + S1 * 2.0;
        }
        if row == 0 {
            for (label, on, id) in [(t("区分大小写"), s.case, 30u8), (t("正则表达式"), s.regex, 31), (t("全字匹配"), s.word, 32)] {
                let bx = Rect::new(x + S1, y + (FIELD_H - 13.0) / 2.0, 13.0, 13.0);
                if on {
                    g.fill(bx, th.accent);
                    g.line(bx.x + 3.0, bx.y + 6.8, bx.x + 5.4, bx.y + 9.2, app().theme.on_accent, 1.6);
                    g.line(bx.x + 5.4, bx.y + 9.2, bx.x + 10.0, bx.y + 4.0, app().theme.on_accent, 1.6);
                } else {
                    g.stroke_round(bx, 0.0, th.border_strong, 1.2);
                }
                let l = g.layout(label, sm, 400.0);
                let lm = metrics(&l);
                g.text(&l, bx.right() + S1, y + (FIELD_H - lm.h) / 2.0, th.text_secondary);
                let hr = Rect::new(bx.x - S1, y, bx.w + S1 * 3.0 + lm.w, FIELD_H);
                regions.push((hr, Hit::Status(id)));
                x = hr.right() + S2;
            }
            let cl = g.layout(&count, Style::new(Stack::Code, TEXT_XS), 200.0);
            g.text(&cl, x + S2, y + (FIELD_H - metrics(&cl).h) / 2.0, if s.invalid { th.danger } else { th.text_tertiary });
        }
        y += FIELD_H + S1 * 2.0;
    }
    let close = g.layout("×", Style::new(Stack::Ui, TEXT_SM), 40.0);
    let cm = metrics(&close);
    let cr = Rect::new(bar.right() - S2 - cm.w - S2 * 2.0, top, cm.w + S2 * 2.0, FIELD_H);
    let hv = w.ui.hover_t(Hit::Status(40));
    g.text(&close, cr.x + S2, cr.y + (cr.h - cm.h) / 2.0, crate::theme::mix(th.text_primary, th.text_secondary, hv));
    regions.push((cr, Hit::Status(40)));
    for (r, h) in regions {
        w.ui.region(r, h);
    }
}

/// Clicks on the find bar.
pub fn search_click(w: &mut Window, hit: Hit) -> bool {
    let Hit::Status(id) = hit else { return false };
    let Some(d) = &mut w.doc else { return false };
    let Some(s) = &mut d.view.search else { return false };
    match id {
        10 | 11 => {
            s.focus = Some(id - 10);
            return true;
        }
        20 => {
            d.view.search_step(1);
        }
        21 => {
            d.view.search_step(-1);
        }
        22 => {
            d.view.search_replace();
            w.after_input();
        }
        23 => {
            let n = d.view.search_replace_all();
            w.after_input();
            w.toast(&tf("已替换 {0} 处匹配", &[&n]));
        }
        30 => s.case = !s.case,
        31 => s.regex = !s.regex,
        32 => s.word = !s.word,
        40 => w.search_close(),
        _ => return false,
    }
    true
}

/// Hit id base of the table toolbar buttons; the spec index is added.
pub const TABLE_BUTTON: u32 = 10_000;
pub const TABLE_BAR: u32 = 19_999;

enum Piece {
    Pos(String),
    Group { label: String, first: bool },
    Button { spec: usize, label: String, enabled: bool },
}

/// Floating table toolbar: shown while the caret is inside a table, an
/// overlay on the editor so nothing in the text moves.
fn table_toolbar(w: &mut Window, g: &Gfx, area: Rect) {
    use mdwisp_table::model::{cell_at, find_table_at, parse_table};
    use mdwisp_table::specs::{GROUPS, Input, SPECS, disabled_reason, has_header};
    let th = app().theme;
    w.shell.table_bar = None;
    if app().s().read_only {
        return;
    }
    let Some(d) = &w.doc else { return };
    let head = d.view.buf.sel.head;
    if !d.view.deco.tables.iter().any(|c| c.start <= head && head <= c.end) {
        return;
    }
    let Some(ctx) = find_table_at(&d.view.buf.text, head) else { return };
    let cells = parse_table(&ctx);
    let cell = cell_at(&ctx, head);
    let input = Input { cells: &cells, aligns: &ctx.aligns, cell, has_header: has_header(&cells) };
    let mut pieces = vec![Piece::Pos(cell.map_or_else(|| t("表格").to_string(), |c| format!("{}·{}", c.row + 1, c.col + 1)))];
    for (gi, (group, label)) in GROUPS.iter().enumerate() {
        pieces.push(Piece::Group { label: t(label).into(), first: gi == 0 });
        for (si, spec) in SPECS.iter().enumerate().filter(|(_, s)| s.group == *group) {
            pieces.push(Piece::Button { spec: si, label: t(spec.label).into(), enabled: disabled_reason(spec.id, &input).is_none() });
        }
    }
    // Measure every piece first: the bar is as wide as its content, centred.
    let xs = Style::new(Stack::Ui, TEXT_XS);
    let width = |p: &Piece| match p {
        Piece::Pos(s) => metrics(&g.layout(s, xs, 200.0)).w + S2 * 2.0,
        Piece::Group { label, first } => metrics(&g.layout(label, xs, 200.0)).w + S1 + if *first { S3 } else { S3 * 2.0 + 1.0 },
        Piece::Button { label, .. } => (metrics(&g.layout(label, xs, 200.0)).w + S2 * 2.0).max(30.0) + 2.0,
    };
    let widths: Vec<f32> = pieces.iter().map(width).collect();
    let total: f32 = widths.iter().sum::<f32>() + S2 * 2.0;
    let bw = total.min(area.w - S6);
    let bh = 24.0 + S1 * 2.0 + 2.0;
    let bar = Rect::new(area.x + (area.w - bw) / 2.0, area.y + S2, bw, bh);
    g.fill_round(bar, crate::theme::RADIUS_LG, th.bg_overlay);
    g.stroke_round(bar, crate::theme::RADIUS_LG, th.border_strong, 1.0);
    w.ui.region(bar, Hit::Widget(TABLE_BAR));
    // Too wide for the pane: it scrolls sideways under the wheel.
    w.shell.table_scroll = w.shell.table_scroll.clamp(0.0, (total - bw).max(0.0));
    let mut x = bar.x + S2 - w.shell.table_scroll;
    let cy = bar.y + bar.h / 2.0;
    g.clip(bar.inset(1.0));
    for (p, wd) in pieces.iter().zip(&widths) {
        match p {
            Piece::Pos(s) => {
                let l = g.layout(s, xs, 200.0);
                g.text(&l, x + S2, cy - metrics(&l).h / 2.0, th.text_tertiary);
            }
            Piece::Group { label, first } => {
                let mut lx = x + S3;
                if !first {
                    // Groups are separated by a hairline, so families of operations read apart.
                    g.fill(Rect::new(x + S3, bar.y + S2, 1.0, bar.h - S2 * 2.0), th.border_soft);
                    lx += S3 + 1.0;
                }
                let l = g.layout(label, xs, 200.0);
                g.text(&l, lx, cy - metrics(&l).h / 2.0, th.text_disabled);
            }
            Piece::Button { spec, label, enabled } => {
                let r = Rect::new(x, cy - 12.0, wd - 2.0, 24.0);
                let hit = Hit::Widget(TABLE_BUTTON + *spec as u32);
                let h = if *enabled { w.ui.hover_t(hit) } else { 0.0 };
                if w.ui.pressed(hit) {
                    g.fill_round(r, RADIUS_SM, th.bg_active);
                } else if h > 0.0 {
                    g.fill_round(r, RADIUS_SM, crate::theme::with_alpha(th.bg_hover, h));
                }
                let l = g.layout(label, xs, 200.0);
                let m = metrics(&l);
                let c = if *enabled { crate::theme::mix(th.text_primary, th.text_secondary, h) } else { th.text_disabled };
                g.text(&l, r.x + (r.w - m.w) / 2.0, cy - m.h / 2.0, c);
                if *enabled && r.right() > bar.x && r.x < bar.right() {
                    w.ui.region(r, hit);
                }
            }
        }
        x += wd;
    }
    g.unclip();
    w.shell.table_bar = Some(bar);
}

/// A toolbar button: run the engine's spec directly (the registry still
/// lists some of these as unimplemented, but the engine handles them).
pub fn table_button(w: &mut Window, spec: usize) {
    let Some(s) = mdwisp_table::specs::SPECS.get(spec) else { return };
    let id = s.id;
    let clip = if mdwisp_table::specs::needs_clipboard(id) { crate::clipboard::text(w.hwnd).unwrap_or_default() } else { String::new() };
    let src = crate::actions::source(w).to_string();
    let head = crate::actions::sel(w).head;
    match mdwisp_table::specs::resolve(id, &src, head, &clip, mdwisp_core::i18n::current_language()) {
        Some(effect) => {
            if let Some(c) = &effect.clipboard {
                crate::clipboard::set_text(w.hwnd, c);
            }
            if let Some((a, b)) = effect.select {
                if let Some(d) = &mut w.doc {
                    d.view.buf.sel = crate::editor::buffer::Sel { anchor: a, head: b };
                }
            } else if let Some(e) = effect.edit {
                crate::actions::apply(w, mdwisp_text::ops::EditResult { text: e.text, cursor: e.cursor, selection: None });
            }
        }
        None => w.toast(t("这个操作在当前单元格不可用")),
    }
}

/// Transient message, bottom right, sliding in like the CSS toast.
pub fn toast(w: &mut Window, g: &Gfx) {
    let Some((text, _)) = &w.toast_text else { return };
    let k = w.ui.value((Hit::Panel, 7), 0.0);
    if k <= 0.001 {
        return;
    }
    let th = app().theme;
    let l = g.layout(text, Style::new(Stack::Ui, TEXT_SM), 480.0);
    let m = metrics(&l);
    let bw = m.w + S3 + S4 + 3.0;
    let bh = (m.h + S2 * 2.0).max(36.0);
    let x = w.width - S6 - bw;
    let y = w.height - S6 - bh;
    g.with_opacity(k.clamp(0.0, 1.0), || {
        let r = Rect::new(x, y, bw, bh);
        g.fill_round(r, RADIUS_MD, th.bg_overlay);
        g.stroke_round(r, RADIUS_MD, th.border_strong, 1.0);
        g.fill(Rect::new(x, y, 3.0, bh), th.accent);
        g.text(&l, x + 3.0 + S3, y + (bh - m.h) / 2.0, th.text_primary);
    });
}
