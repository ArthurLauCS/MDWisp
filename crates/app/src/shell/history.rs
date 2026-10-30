//! The history dock: a rail on the right edge and, when open, the panel of
//! restore points with what each one changed.

use mdwisp_core::history::{DiffKind, DiffSummary, Revision, RevisionKind};
use mdwisp_core::i18n::{t, tf};
use mdwisp_text::history::{absolute_time, format_bytes, format_delta, kind_label, relative_time};

use super::widgets::{BTN_H, Btn, button};
use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::theme::{self, RADIUS_MD, RADIUS_SM, S1, S2, S3, S4, S6, S8, S16, SIDE_PANEL_HEAD_H, SIDE_PANEL_MAX_W, SIDE_PANEL_MIN_W, TEXT_LG, TEXT_SM, TEXT_XS};
use crate::ui::Hit;
use crate::window::Window;
use crate::{app, win};

const ICON: &str = "M3 11a9 9 0 1 1 2 7M3 5v6h6M12 7v5l3 2";

#[derive(Default, PartialEq, Clone, Copy)]
enum Confirm {
    #[default]
    None,
    Restore,
    Clear,
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub stale: bool,
    doc_id: String,
    revs: Vec<Revision>,
    selected: Option<String>,
    text_view: bool,
    diff: Option<(String, DiffSummary)>,
    text: Option<(String, String)>,
    confirm: Confirm,
    forgetting: Option<String>,
    error: Option<String>,
    list_scroll: f32,
    body_scroll: f32,
    list: Rect,
    body: Rect,
}

impl Window {
    pub fn toggle_history(&mut self) {
        self.shell.history.open = !self.shell.history.open;
        self.shell.history.stale = true;
        win::request_frame(self.hwnd);
    }
}

fn refresh(w: &mut Window) {
    let id = w.doc.as_ref().and_then(|d| d.meta.as_ref()).map(|m| m.id.clone()).unwrap_or_default();
    let s = &mut w.shell.history;
    if !s.stale && s.doc_id == id {
        return;
    }
    s.stale = false;
    s.doc_id = id.clone();
    s.revs = if id.is_empty() { vec![] } else { app().history.list(&id) };
    // A forgotten revision must not leave the selection pointing at nothing.
    if !s.selected.as_ref().is_some_and(|sel| s.revs.iter().any(|r| &r.id == sel)) {
        s.selected = s.revs.first().map(|r| r.id.clone());
    }
    s.diff = None;
    s.text = None;
}

/// Rail on the right edge, plus the panel left of it when open. Shrinks `body`.
pub fn paint(w: &mut Window, g: &Gfx, body: &mut Rect) -> Rect {
    let th = app().theme;
    let rail = body.cut_right(S16);
    g.fill(rail, th.bg_raised);
    g.fill(Rect::new(rail.x, rail.y, 1.0, rail.h), th.border_soft);
    let open = w.shell.history.open;
    let hit = Hit::HistoryToggle;
    let h = w.ui.hover_t(hit);
    let label = g.layout(t("历史"), Style::new(Stack::Ui, TEXT_XS), rail.w);
    let lm = metrics(&label);
    let btn = Rect::new(rail.x + 1.0, rail.y, rail.w - 1.0, S3 * 2.0 + S6 + S1 + lm.h);
    let fg = if open {
        g.fill(btn, th.accent_soft);
        th.text_primary
    } else {
        g.fill(btn, theme::with_alpha(th.bg_hover, h));
        theme::mix(th.text_primary, th.text_tertiary, h)
    };
    g.stroke_svg(ICON, btn.x + (btn.w - S6) / 2.0, btn.y + S3, 1.0, fg, 1.5);
    g.text(&label, btn.x + (btn.w - lm.w) / 2.0, btn.y + S3 + S6 + S1, fg);
    w.ui.region(btn, hit);
    if !open {
        return rail;
    }
    refresh(w);
    let width = (app().s().history_width as f32).clamp(SIDE_PANEL_MIN_W, SIDE_PANEL_MAX_W.min(w.width * 0.35).max(SIDE_PANEL_MIN_W));
    // Narrow windows float the panel over the editor instead of squeezing it.
    let overlay = w.width <= 1100.0;
    let r = if overlay { Rect::new(rail.x - width, rail.y, width, rail.h) } else { body.cut_right(width) };
    if overlay {
    }
    panel(w, g, r);
    rail
}

fn panel(w: &mut Window, g: &Gfx, r: Rect) {
    let th = app().theme;
    g.fill(r, th.bg_raised);
    g.fill(Rect::new(r.x, r.y, 1.0, r.h), th.border_soft);
    let mut body = r;
    // Resize handle on the left edge.
    let rh = Rect::new(r.x, r.y, S2, r.h);
    let hv = w.ui.hover_t(Hit::HistoryResize);
    if hv > 0.0 {
        g.fill(Rect::new(rh.x, rh.y, 2.0, rh.h), theme::with_alpha(th.accent, hv));
    }

    // ---- head
    let head = body.cut_top(SIDE_PANEL_HEAD_H);
    g.fill(Rect::new(head.x, head.bottom() - 1.0, head.w, 1.0), th.border_soft);
    let title = g.layout(t("历史版本"), Style::new(Stack::Ui, TEXT_SM).weight(700), head.w);
    let tm = metrics(&title);
    let sub = match &w.doc {
        Some(d) => d.name(),
        None => t("未打开文档").into(),
    };
    let sl = g.layout(&sub, Style::new(Stack::Ui, TEXT_XS), head.w - S4 * 3.0 - 30.0);
    let sm = metrics(&sl);
    let ty = head.y + (head.h - tm.h - S1 - sm.h) / 2.0;
    g.text(&title, head.x + S4, ty, th.text_primary);
    g.text(&sl, head.x + S4, ty + tm.h + S1, th.text_tertiary);
    close_button(w, g, Rect::new(head.right() - S4 - 22.0, head.y + (head.h - 20.0) / 2.0, 22.0, 20.0), "›", Hit::HistoryButton(0));

    // ---- foot
    let n = w.shell.history.revs.len();
    let confirm = w.shell.history.confirm;
    let note = match confirm {
        Confirm::Clear => t("将删除这篇文档的全部还原点，无法撤销。").to_string(),
        Confirm::Restore => t("恢复会覆盖当前内容，原内容会先存为一个还原点。").to_string(),
        Confirm::None => tf("{0} 个还原点", &[&n]),
    };
    let nl = g.layout(&note, Style::new(Stack::Ui, TEXT_XS), body.w - S4 * 2.0);
    let nm = metrics(&nl);
    let inner_w = body.w - S4 * 2.0;
    let has_doc = !w.shell.history.doc_id.is_empty();
    let read_only = app().s().read_only;
    let buttons: Vec<(&str, Btn, bool, u8)> = match confirm {
        Confirm::Clear => vec![(t("取消"), Btn::Normal, true, 4), (t("确认清空"), Btn::Danger, true, 5)],
        Confirm::Restore => vec![(t("取消"), Btn::Normal, true, 4), (t("确认恢复"), Btn::Primary, !read_only, 6)],
        Confirm::None => vec![
            (t("清空历史"), Btn::Normal, n > 0, 1),
            (t("立即记录"), Btn::Normal, has_doc, 2),
            (t("恢复到此版本"), Btn::Primary, w.shell.history.selected.is_some() && !read_only, 3),
        ],
    };
    let mut rows: Vec<Vec<(f32, usize)>> = vec![vec![]];
    let mut used = 0.0;
    for (i, (label, ..)) in buttons.iter().enumerate() {
        let bw = super::widgets::button_width(g, label);
        let row = rows.last_mut().unwrap();
        if !row.is_empty() && used + S2 + bw > inner_w {
            rows.push(vec![(bw, i)]);
            used = bw;
        } else {
            used += if row.is_empty() { bw } else { S2 + bw };
            row.push((bw, i));
        }
    }
    let foot = body.cut_bottom(S3 * 2.0 + nm.h + S2 + rows.len() as f32 * (BTN_H + S2) - S2);
    g.fill(Rect::new(foot.x, foot.y, foot.w, 1.0), th.border_soft);
    g.text(&nl, foot.x + S4, foot.y + S3, th.text_tertiary);
    let mut y = foot.y + S3 + nm.h + S2;
    for row in &rows {
        let natural: f32 = row.iter().map(|(bw, _)| bw).sum::<f32>() + S2 * (row.len() as f32 - 1.0);
        let grow = (inner_w - natural) / row.len() as f32;
        let mut x = foot.x + S4;
        for (bw, i) in row {
            let (label, kind, enabled, id) = &buttons[*i];
            let br = Rect::new(x, y, bw + grow, BTN_H);
            button(&mut w.ui, g, br, Hit::HistoryButton(*id), label, *kind, *enabled);
            x += bw + grow + S2;
        }
        y += BTN_H + S2;
    }

    // ---- body
    let inner = body.pad(S4, S4);
    w.shell.history.body = inner;
    if let Some(e) = &w.shell.history.error {
        let l = g.layout(e, Style::new(Stack::Ui, TEXT_SM), inner.w);
        g.text(&l, inner.x, inner.y, th.danger);
        return;
    }
    if n == 0 {
        let msg = if has_doc { t("这篇文档还没有还原点。点击「立即记录」保存当前内容。") } else { t("首次保存文档后开始记录历史；未命名草稿不会定时保存。") };
        let l = g.layout(msg, Style::new(Stack::Ui, TEXT_SM), inner.w);
        unsafe {
            let _ = l.SetTextAlignment(windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        g.text(&l, inner.x, inner.y + S8, th.text_tertiary);
        return;
    }
    // Revision list: at most 30% of the window tall, scrolling inside.
    let row_h = TEXT_SM * 1.5 + TEXT_XS * 1.5 + S1 + S2 * 2.0 + 2.0;
    let list_h = (n as f32 * (row_h + S1)).min(w.height * 0.3);
    let list = Rect::new(inner.x, inner.y, inner.w, list_h);
    w.shell.history.list = list;
    let s = &mut w.shell.history;
    s.list_scroll = s.list_scroll.clamp(0.0, (n as f32 * (row_h + S1) - list_h).max(0.0));
    let revs = s.revs.clone();
    let scroll = s.list_scroll;
    g.clip(list);
    for (i, rev) in revs.iter().enumerate() {
        let y = list.y + i as f32 * (row_h + S1) - scroll;
        if y + row_h < list.y || y > list.bottom() {
            continue;
        }
        let rr = Rect::new(list.x, y, list.w, row_h);
        let hit = Hit::HistoryRow(i as u32);
        if w.shell.history.forgetting.as_deref() == Some(rev.id.as_str()) {
            g.fill_round(rr, RADIUS_MD, th.bg_inset);
            g.stroke_round(rr, RADIUS_MD, th.border_strong, 1.0);
            let l = g.layout(t("删除这个还原点？"), Style::new(Stack::Ui, TEXT_XS), rr.w);
            let m = metrics(&l);
            g.text(&l, rr.x + S3, rr.y + (rr.h - m.h) / 2.0, th.text_secondary);
            let bh = TEXT_XS * 1.5 + S1 * 2.0 + 2.0;
            let cw = super::widgets::button_width(g, t("取消")) - S2;
            let dw = super::widgets::button_width(g, t("删除")) - S2;
            let by = rr.y + (rr.h - bh) / 2.0;
            button(&mut w.ui, g, Rect::new(rr.right() - S3 - cw, by, cw, bh), Hit::HistoryButton(8), t("取消"), Btn::Normal, true);
            button(&mut w.ui, g, Rect::new(rr.right() - S3 - cw - S2 - dw, by, dw, bh), Hit::HistoryButton(7), t("删除"), Btn::Danger, true);
            continue;
        }
        let on = w.shell.history.selected.as_deref() == Some(rev.id.as_str());
        let h = w.ui.hover_t(hit);
        if on {
            g.fill_round(rr, RADIUS_MD, th.accent_soft);
            g.fill(Rect::new(rr.x, rr.y, 2.0, rr.h), th.accent);
        } else if h > 0.0 {
            g.fill_round(rr, RADIUS_MD, theme::with_alpha(th.bg_hover, h));
        }
        w.ui.region(rr, hit);
        let time = g.layout(&relative_time(rev.at as i64, mdwisp_core::now_ms() as i64), Style::new(Stack::Ui, TEXT_SM), rr.w);
        let tm = metrics(&time);
        g.text(&time, rr.x + S3, rr.y + S2 + 1.0, th.text_primary);
        // Meta line: kind tag · size · line delta.
        let my = rr.y + S2 + 1.0 + tm.h + S1;
        let kind = match rev.kind {
            RevisionKind::Auto => "auto",
            RevisionKind::Manual => "manual",
            RevisionKind::Restore => "restore",
        };
        let tag = g.layout(kind_label(kind), Style::new(Stack::Ui, TEXT_XS), 200.0);
        let tgm = metrics(&tag);
        let tag_r = Rect::new(rr.x + S3, my, tgm.w + S2 * 2.0, tgm.h);
        let (border, fg) = match rev.kind {
            RevisionKind::Auto => (th.border_soft, th.text_tertiary),
            RevisionKind::Manual => (th.accent_line, th.accent),
            RevisionKind::Restore => (th.border_strong, th.link),
        };
        g.fill(tag_r, th.bg_overlay);
        g.stroke_round(tag_r, 0.0, border, 1.0);
        g.text(&tag, tag_r.x + S2, tag_r.y, fg);
        let mut mx = tag_r.right() + S2;
        let bytes = g.layout(&format_bytes(rev.bytes as u64), Style::new(Stack::Code, TEXT_XS), 200.0);
        g.text(&bytes, mx, my, th.text_tertiary);
        mx += metrics(&bytes).w + S2;
        let delta = revs.get(i + 1).map(|prev| rev.lines.unwrap_or(0) as i64 - prev.lines.unwrap_or(0) as i64);
        let (dtext, dc) = match delta {
            None => (t("起始版本").to_string(), th.text_disabled),
            Some(d) if d > 0 => (format_delta(d), th.success),
            Some(d) if d < 0 => (format_delta(d), th.danger),
            Some(d) => (format_delta(d), th.text_disabled),
        };
        let dl = g.layout(&dtext, Style::new(Stack::Code, TEXT_XS), 200.0);
        g.text(&dl, mx, my, dc);
        // The per-row delete is quiet until the row is hovered or selected.
        let x_hit = Hit::HistoryButton(100 + i as u8);
        let show = if on { 1.0 } else { h.max(w.ui.hover_t(x_hit)) };
        if show > 0.0 {
            let xl = g.layout("×", Style::new(Stack::Code, TEXT_SM), 40.0);
            let xm = metrics(&xl);
            let xr = Rect::new(rr.right() - S3 - xm.w - S2 * 2.0, rr.y, xm.w + S2 * 2.0, rr.h);
            let c = theme::mix(th.danger, th.text_disabled, w.ui.hover_t(x_hit));
            g.text(&xl, xr.x + S2, xr.y + (xr.h - xm.h) / 2.0, theme::with_alpha(c, show));
            w.ui.region(xr, x_hit);
        }
    }
    g.unclip();

    // ---- detail
    let mut detail = Rect::new(inner.x, list.bottom() + S6, inner.w, (inner.bottom() - list.bottom() - S6).max(0.0));
    let tabs = detail.cut_top(BTN_H);
    let text_view = w.shell.history.text_view;
    let mut x = tabs.x;
    for (label, on, id) in [(t("变更"), !text_view, 10u8), (t("全文"), text_view, 11)] {
        let bw = super::widgets::button_width(g, label);
        button(&mut w.ui, g, Rect::new(x, tabs.y, bw, tabs.h), Hit::HistoryButton(id), label, if on { Btn::Pressed } else { Btn::Normal }, true);
        x += bw + S2;
    }
    let del = t("删除此版本");
    let dw = super::widgets::button_width(g, del);
    button(&mut w.ui, g, Rect::new(tabs.right() - dw, tabs.y, dw, tabs.h), Hit::HistoryButton(12), del, Btn::Normal, w.shell.history.selected.is_some());

    let Some(sel) = w.shell.history.selected.clone() else { return };
    let idx = revs.iter().position(|r| r.id == sel).unwrap_or(0);
    let prev = revs.get(idx + 1).map(|r| r.id.clone()).unwrap_or_default();
    let caption = g.layout(&absolute_time(revs[idx].at as i64), Style::new(Stack::Ui, TEXT_XS), detail.w);
    detail.cut_top(S2);
    let cap = detail.cut_top(metrics(&caption).h + S2);
    g.text(&caption, cap.x, cap.y, th.text_tertiary);
    g.clip(detail);
    let id = w.shell.history.doc_id.clone();
    let mut y = detail.y - w.shell.history.body_scroll;
    if text_view {
        if w.shell.history.text.as_ref().is_none_or(|(r, _)| *r != sel) {
            let text = app().history.read(&id, &sel).unwrap_or_default();
            w.shell.history.text = Some((sel.clone(), text));
        }
        let text = &w.shell.history.text.as_ref().unwrap().1;
        // Very long revisions show their first part; the full text is a restore away.
        let shown: String = text.chars().take(60_000).collect();
        let l = g.new_layout(&shown, Style::new(Stack::Code, TEXT_SM), detail.w - S3 * 2.0, None);
        unsafe {
            let _ = l.SetLineSpacing(windows::Win32::Graphics::DirectWrite::DWRITE_LINE_SPACING_METHOD_UNIFORM, TEXT_SM * 1.6, TEXT_SM * 1.25);
        }
        let m = metrics(&l);
        let box_r = Rect::new(detail.x, y, detail.w, m.h + S3 * 2.0);
        g.fill_round(box_r, RADIUS_SM, th.bg_inset);
        g.text(&l, detail.x + S3, y + S3, th.text_primary);
        y += box_r.h;
    } else {
        if w.shell.history.diff.as_ref().is_none_or(|(r, _)| *r != sel) {
            let d = app().history.diff(&id, &prev, &sel).unwrap_or(DiffSummary { added: 0, removed: 0, hunks: vec![] });
            w.shell.history.diff = Some((sel.clone(), d));
        }
        let diff = &w.shell.history.diff.as_ref().unwrap().1;
        let c = g.layout(if prev.is_empty() { t("起始版本") } else { t("与上一还原点比较") }, Style::new(Stack::Ui, TEXT_XS), detail.w);
        g.text(&c, detail.x, y, th.text_tertiary);
        y += metrics(&c).h + S2 * 2.0;
        // +N 新增 −N 移除 (not 删除: that key is the Delete button's verb)
        let mut sx = detail.x;
        for (n, label, tone) in [(format!("+{}", diff.added), t("新增"), th.success), (format!("−{}", diff.removed), t("移除"), th.danger)] {
            let nl = g.layout(&n, Style::new(Stack::Code, TEXT_LG), 200.0);
            let nm = metrics(&nl);
            g.text(&nl, sx, y, tone);
            sx += nm.w + S3;
            let ll = g.layout(label, Style::new(Stack::Ui, TEXT_SM), 200.0);
            let lm = metrics(&ll);
            g.text(&ll, sx, y + (nm.h - lm.h) / 2.0, th.text_secondary);
            sx += lm.w + S3;
        }
        y += TEXT_LG * 1.5 + S3;
        if diff.hunks.is_empty() {
            let l = g.layout(t("这一版与上一版内容相同"), Style::new(Stack::Ui, TEXT_SM), detail.w);
            unsafe {
                let _ = l.SetTextAlignment(windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            g.text(&l, detail.x, y + S8, th.text_tertiary);
        }
        for hunk in &diff.hunks {
            y += S3;
            let top = y;
            for line in hunk {
                let style = Style::new(Stack::Code, TEXT_XS);
                let text_x = detail.x + S3 + 40.0 + S3 + 40.0 + S3;
                let l = g.new_layout(if line.text.is_empty() { " " } else { &line.text }, style, (detail.right() - S3 - text_x).max(10.0), None);
                let m = metrics(&l);
                let rh = m.h + S1 * 2.0;
                if y + rh > detail.y && y < detail.bottom() {
                    for (n, x) in [(line.old_line, detail.x + S3), (line.new_line, detail.x + S3 + 40.0 + S3)] {
                        if let Some(n) = n {
                            let nl = g.layout(&n.to_string(), style, 40.0);
                            g.text(&nl, x + 40.0 - metrics(&nl).w, y + S1, th.text_disabled);
                        }
                    }
                    let c = match line.kind {
                        DiffKind::Add => th.text_secondary,
                        DiffKind::Del => th.text_tertiary,
                        DiffKind::Same => th.text_tertiary,
                    };
                    if line.kind == DiffKind::Del {
                        unsafe {
                            let _ = l.SetStrikethrough(true, windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE { startPosition: 0, length: u32::MAX });
                        }
                    }
                    if line.kind == DiffKind::Add {
                        g.fill(Rect::new(detail.x, y, detail.w, rh), theme::with_alpha(th.success, 0.08));
                    }
                    g.text(&l, text_x, y + S1, c);
                }
                y += rh;
                g.fill(Rect::new(detail.x, y - 1.0, detail.w, 1.0), theme::with_alpha(th.border_soft, 0.45));
            }
            g.stroke_round(Rect::new(detail.x, top, detail.w, y - top), RADIUS_SM, th.border_soft, 1.0);
        }
    }
    g.unclip();
    let content = y + w.shell.history.body_scroll - detail.y;
    let max = (content - detail.h).max(0.0);
    w.shell.history.body_scroll = w.shell.history.body_scroll.min(max);
    w.ui.region(rh, Hit::HistoryResize);
}

fn close_button(w: &mut Window, g: &Gfx, r: Rect, label: &str, hit: Hit) {
    let th = app().theme;
    let h = w.ui.hover_t(hit);
    g.fill_round(r, RADIUS_SM, th.bg_inset);
    g.stroke_round(r, RADIUS_SM, theme::mix(th.accent_line, th.border_soft, h), 1.0);
    let l = g.layout(label, Style::new(Stack::Code, TEXT_XS), r.w);
    let m = metrics(&l);
    g.text(&l, r.x + (r.w - m.w) / 2.0, r.y + (r.h - m.h) / 2.0, theme::mix(th.text_primary, th.text_tertiary, h));
    w.ui.region(r, hit);
}

pub fn pointer_down(_w: &mut Window, _hit: Option<Hit>, _x: f32, _y: f32) -> bool {
    false
}

pub fn click(w: &mut Window, hit: Hit) {
    let id = w.shell.history.doc_id.clone();
    match hit {
        Hit::HistoryToggle | Hit::HistoryButton(0) => w.toggle_history(),
        Hit::HistoryRow(i) => {
            let s = &mut w.shell.history;
            s.selected = s.revs.get(i as usize).map(|r| r.id.clone());
            s.confirm = Confirm::None;
            s.body_scroll = 0.0;
        }
        Hit::HistoryButton(1) => w.shell.history.confirm = Confirm::Clear,
        Hit::HistoryButton(2) => w.snapshot(RevisionKind::Manual),
        Hit::HistoryButton(3) => w.shell.history.confirm = Confirm::Restore,
        Hit::HistoryButton(4) => w.shell.history.confirm = Confirm::None,
        Hit::HistoryButton(5) => {
            if let Err(e) = app().history.clear(&id) {
                w.shell.history.error = Some(e);
            }
            w.shell.history.confirm = Confirm::None;
            w.shell.history.stale = true;
        }
        Hit::HistoryButton(6) => {
            w.shell.history.confirm = Confirm::None;
            let Some(sel) = w.shell.history.selected.clone() else { return };
            let Some(text) = app().history.read(&id, &sel) else { return };
            // The current text becomes a restore point first; the editor then
            // takes the old text like any other edit and autosaves it.
            if let Some(d) = &w.doc {
                let _ = app().history.record(&id, &d.view.buf.text, RevisionKind::Restore, 0);
            }
            if let Some(d) = &mut w.doc {
                d.view.buf.set_text(&text);
            }
            w.after_input();
            w.shell.history.stale = true;
        }
        Hit::HistoryButton(7) => {
            if let Some(f) = w.shell.history.forgetting.take() {
                if let Err(e) = app().history.forget(&id, &f) {
                    w.shell.history.error = Some(e);
                }
                w.shell.history.stale = true;
            }
        }
        Hit::HistoryButton(8) => w.shell.history.forgetting = None,
        Hit::HistoryButton(10) => w.shell.history.text_view = false,
        Hit::HistoryButton(11) => w.shell.history.text_view = true,
        Hit::HistoryButton(12) => w.shell.history.forgetting = w.shell.history.selected.clone(),
        Hit::HistoryButton(n) if n >= 100 => {
            w.shell.history.forgetting = w.shell.history.revs.get(n as usize - 100).map(|r| r.id.clone());
        }
        _ => {}
    }
}

pub fn wheel(w: &mut Window, x: f32, y: f32, dy: f32) -> bool {
    let s = &mut w.shell.history;
    if !s.open {
        return false;
    }
    if s.list.contains(x, y) {
        s.list_scroll = (s.list_scroll - dy * 60.0).max(0.0);
        return true;
    }
    if s.body.contains(x, y) {
        s.body_scroll = (s.body_scroll - dy * 60.0).max(0.0);
        return true;
    }
    false
}
