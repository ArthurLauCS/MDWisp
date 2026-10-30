//! The editor surface: lays out decorated lines, paints them, and turns
//! pointer and keyboard input into buffer edits.

use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use windows::core::BOOL;
use windows::Win32::Graphics::DirectWrite::*;

use super::buffer::{Buffer, Kind, Sel};
use super::deco::{self, Code, Deco, Focus};
use super::images::Images;
use super::layout::{self, Bg, Cache, Line, Look, Obj, disp_to_src, src_to_disp_pos};
use super::table::{self, TableView};
use crate::gfx::{Gfx, Rect};
use crate::theme::{S1, S2, S4, Theme, with_alpha};

pub const PAD_TOP: f32 = 48.0;

pub struct View {
    pub buf: Buffer,
    pub deco: Deco,
    deco_key: (u64, Sel, bool),
    pub lines: Vec<Line>,
    pub tables: Vec<TableView>,
    layout_key: (u64, Option<usize>, u32, u64),
    cache: Cache,
    pub look: Look,
    pub scroll: f32,
    scroll_from: f32,
    scroll_target: f32,
    scroll_start: Option<Instant>,
    /// Remembered x for vertical caret motion.
    goal_x: Option<f32>,
    pub blink: Instant,
    pub focused: bool,
    pub doc_dir: Option<PathBuf>,
    pub images: Images,
    /// Geometry of the last paint, in window DIPs.
    pub rect: Rect,
    pub content_x: f32,
    pub content_w: f32,
    pub dragging: bool,
    /// IME text not yet committed, already in the buffer; see `compose`.
    pub composition: Option<Range<usize>>,
    pub read_only: bool,
    pub highlight_line: bool,
    pub searching: bool,
    pub margin_left: f32,
    pub margin_right: f32,
    look_gen: u64,
    /// An image was clicked; `Z` views it fullscreen until the caret moves on.
    pub image_selected: bool,
    pub search: Option<super::search::Search>,
    pub cursor_style: mdwisp_core::settings::CursorStyle,
    /// Code block under the pointer, for the language chip.
    pub hover_block: Option<usize>,
    /// Clickable widgets drawn this frame: (box, id). The pane registers them.
    pub widget_hits: Vec<(Rect, u32)>,
    /// Track of the overlay scrollbar from the last paint, and how visible it is.
    pub scrollbar: Option<Rect>,
    pub scrollbar_alpha: f32,
    /// Lines still carrying an estimated height; shaped in the background.
    pub pending: bool,
    /// How many off-screen lines the next relayout may shape (background fill).
    pub fill_budget: usize,
}

impl View {
    pub fn new(text: String, doc_dir: Option<PathBuf>) -> View {
        View {
            buf: Buffer::new(text),
            deco: Deco::default(),
            deco_key: (u64::MAX, Sel::default(), false),
            lines: vec![],
            tables: vec![],
            layout_key: (u64::MAX, None, 0, 0),
            cache: Cache::new(),
            look: Look::default(),
            scroll: 0.0,
            scroll_from: 0.0,
            scroll_target: 0.0,
            scroll_start: None,
            goal_x: None,
            blink: Instant::now(),
            focused: true,
            doc_dir,
            images: Images::default(),
            rect: Rect::default(),
            content_x: 0.0,
            content_w: 0.0,
            dragging: false,
            composition: None,
            read_only: false,
            highlight_line: true,
            searching: false,
            margin_left: 5.0,
            margin_right: 5.0,
            look_gen: 0,
            image_selected: false,
            search: None,
            cursor_style: mdwisp_core::settings::CursorStyle::Bar,
            hover_block: None,
            widget_hits: vec![],
            pending: false,
            scrollbar: None,
            scrollbar_alpha: 0.0,
            fill_budget: 0,
        }
    }

    /// Layouts carry brushes, and images are bitmaps, of the old graphics device.
    pub fn device_lost(&mut self) {
        self.cache = Cache::new();
        self.layout_key.0 = u64::MAX;
        self.images.clear();
    }

    pub fn set_look(&mut self, look: Look) {
        if look != self.look {
            self.look = look;
            self.look_gen += 1;
        }
    }

    /// Recompute decorations and line geometry when the text, selection or width changed.
    pub fn relayout(&mut self, g: &Gfx, t: &Theme) {
        self.searching = self.search.is_some();
        if let Some(s) = &mut self.search {
            s.refresh(&self.buf.text, self.buf.version);
        }
        let sel = self.buf.sel;
        let key = (self.buf.version, sel, self.searching);
        let mut changed = false;
        if key != self.deco_key {
            let old = self.deco_key.1;
            let moved_only = key.0 == self.deco_key.0 && key.2 == self.deco_key.2 && !self.deco.reveals(old.from(), old.to(), old.head) && !self.deco.reveals(sel.from(), sel.to(), sel.head);
            if !moved_only {
                let focus = Focus { from: sel.from(), to: sel.to(), head: sel.head, searching: self.searching };
                self.deco = if self.look.source { source_deco(&self.buf.text) } else { deco::decorate(&self.buf.text, &focus) };
                changed = true;
            }
            self.deco_key = key;
        }
        // The caret only changes geometry inside a table, where its cell shows the source.
        let table_caret = self.deco.tables.iter().any(|c| sel.head >= c.start && sel.head <= c.end).then_some(sel.head);
        let lk = (self.buf.version, table_caret, self.content_w.to_bits(), self.look_gen ^ self.images.generation << 20);
        if !changed && lk == self.layout_key && self.fill_budget == 0 {
            return;
        }
        self.layout_key = lk;
        // Scroll anchoring: keep the caret's line (or else the first visible one)
        // where it was when lines above change height, e.g. an image arriving.
        let caret = self.lines.get(self.line_of(sel.head)).filter(|l| l.y >= self.scroll && l.y < self.scroll + self.rect.h);
        let anchor = caret.or_else(|| self.lines.get(self.line_at_y(self.scroll))).map(|l| (l.src.start, self.scroll - l.y));
        let src = &self.buf.text;
        let d = &self.deco;
        let mut lines = Vec::with_capacity(d.starts.len());
        let mut tables = vec![];
        let mut y = PAD_TOP;
        let mut l = 0;
        // Only what is near the screen is shaped now; the rest is estimated and
        // shaped after the first frame, a budget at a time.
        let view_h = if self.rect.h > 0.0 { self.rect.h } else { 1200.0 };
        let (near_top, near_bottom) = (self.scroll - view_h, self.scroll + view_h * 2.0);
        let mut budget = std::mem::take(&mut self.fill_budget);
        let mut pending = false;
        let mut next_table = 0;
        let images = &self.images;
        let dir = self.doc_dir.clone();
        let size_of = |s: &str| images.size(dir.as_deref(), s);
        while l < d.starts.len() {
            let start = d.starts[l];
            while next_table < d.tables.len() && d.tables[next_table].start < start {
                next_table += 1;
            }
            if let Some(ctx) = d.tables.get(next_table).filter(|c| c.start == start) {
                let caret = (sel.head >= ctx.start && sel.head <= ctx.end).then_some(sel.head);
                let tv = table::build(g, &mut self.cache, src, ctx, self.content_w, &self.look, t, caret);
                let h = tv.h;
                let end = ctx.end;
                lines.push(Line { src: start..end, deco: Default::default(), table: Some(tables.len()), shaped: layout::Shaped::empty(), y, h });
                tables.push(tv);
                y += h;
                l = d.line_of(end.saturating_sub(1).max(start)) + 1;
                continue;
            }
            let end = d.line_end(l, src);
            let ld = d.lines[l];
            let near = y >= near_top && y <= near_bottom;
            // The budget counts lines actually shaped: cached ones are free, or the
            // fill would spend it on the top of the document and never finish.
            let cached = layout::shape_line(g, &mut self.cache, src, d, start..end, &ld, self.content_w, &self.look, t, &size_of, !near);
            let shaped = match cached.or_else(|| (budget > 0).then(|| {
                budget -= 1;
                layout::shape(g, &mut self.cache, src, d, start..end, &ld, self.content_w, &self.look, t, &size_of)
            })) {
                Some(s) => s,
                None => {
                    pending = true;
                    layout::Shaped { text_h: layout::estimate(src, &(start..end), &ld, self.content_w, &self.look, t), ..layout::Shaped::empty() }
                }
            };
            let h = shaped.pad_top + shaped.text_h + shaped.pad_bottom;
            lines.push(Line { src: start..end, deco: ld, table: None, shaped, y, h });
            y += h;
            l += 1;
        }
        self.lines = lines;
        self.tables = tables;
        self.pending = pending;
        if let Some((start, off)) = anchor.filter(|_| self.scroll > 0.0 && self.scroll_start.is_none()) {
            if let Some(l) = self.lines.iter().find(|l| l.src.start >= start) {
                self.scroll = (l.y + off).max(0.0);
                self.scroll_target = self.scroll;
            }
        }
    }

    pub fn content_height(&self) -> f32 {
        self.lines.last().map_or(PAD_TOP, |l| l.y + l.h) + self.rect.h * 0.4
    }

    pub fn max_scroll(&self) -> f32 {
        (self.content_height() - self.rect.h).max(0.0)
    }

    fn line_at_y(&self, y: f32) -> usize {
        self.lines.partition_point(|l| l.y + l.h <= y).min(self.lines.len().saturating_sub(1))
    }

    pub fn line_of(&self, pos: usize) -> usize {
        self.lines.partition_point(|l| l.src.end < pos).min(self.lines.len().saturating_sub(1))
    }

    // ---- geometry ------------------------------------------------------------

    /// Caret rectangle for `pos`, in document coordinates (before scrolling).
    pub fn caret_at(&self, pos: usize) -> Option<(f32, f32, f32)> {
        let line = self.lines.get(self.line_of(pos))?;
        if let Some(ti) = line.table {
            let (x, y, h) = self.tables[ti].caret_at(pos)?;
            return Some((self.content_x + x, line.y + table::PAD_Y + y, h));
        }
        let s = &line.shaped;
        let x0 = self.content_x + s.indent;
        let y0 = line.y + s.pad_top;
        let Some(layout) = &s.layout else { return Some((x0, y0, s.text_h)) };
        let d = src_to_disp_pos(&s.segs, pos, &self.buf.text);
        let mut x = 0.0;
        let mut y = 0.0;
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        unsafe { layout.HitTestTextPosition(d, false, &mut x, &mut y, &mut m).ok()? };
        // The box of the font drawing `pos` (a CJK fallback sits lower than Latin):
        // where its glyphs are, not the line spacing around them.
        Some((x0 + x, y0 + m.top, m.height))
    }

    /// Source position under a window point.
    pub fn pos_at(&self, wx: f32, wy: f32) -> usize {
        let y = wy - self.rect.y + self.scroll;
        if self.lines.is_empty() {
            return 0;
        }
        if y < PAD_TOP {
            return 0;
        }
        let i = self.line_at_y(y);
        let line = &self.lines[i];
        if let Some(ti) = line.table {
            return self.tables[ti].pos_at(wx - self.content_x, y - line.y - table::PAD_Y);
        }
        let s = &line.shaped;
        let Some(layout) = &s.layout else { return line.src.start };
        let mut trailing = BOOL(0);
        let mut inside = BOOL(0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        let lx = wx - self.content_x - s.indent;
        let ly = (y - line.y - s.pad_top).clamp(0.0, s.text_h - 0.5);
        unsafe { layout.HitTestPoint(lx, ly, &mut trailing, &mut inside, &mut m).unwrap() };
        let d = m.textPosition + if trailing.as_bool() { m.length } else { 0 };
        disp_to_src(&s.segs, &line.src, d, &self.buf.text)
    }

    pub fn scroll_into_view(&mut self, pos: usize, center: bool) {
        let Some((_, y, h)) = self.caret_at(pos) else { return };
        let view_h = self.rect.h;
        let target = if center {
            y - view_h / 2.0
        } else if y < self.scroll_target + 16.0 {
            y - 16.0
        } else if y + h > self.scroll_target + view_h - 16.0 {
            y + h - view_h + 16.0
        } else {
            return;
        };
        self.scroll_to(target, false);
    }

    pub fn scroll_to(&mut self, target: f32, smooth: bool) {
        let target = target.clamp(0.0, self.max_scroll());
        if smooth {
            self.scroll_from = self.scroll;
            self.scroll_start = Some(Instant::now());
        } else {
            self.scroll = target;
            self.scroll_start = None;
        }
        self.scroll_target = target;
    }

    /// Put `pos` near the top of the view (outline and fragment jumps).
    pub fn scroll_to_line_top(&mut self, pos: usize) {
        if let Some((_, y, _)) = self.caret_at(pos) {
            self.scroll_to(y - 16.0, true);
        }
    }

    pub fn wheel(&mut self, lines: f32) {
        let base = if self.scroll_start.is_some() { self.scroll_target } else { self.scroll };
        self.scroll_to(base - lines * 100.0, true);
    }

    /// Advance smooth scrolling; true while it is still moving.
    pub fn tick(&mut self) -> bool {
        let Some(start) = self.scroll_start else { return false };
        let t = start.elapsed().as_secs_f32() / 0.16;
        if t >= 1.0 {
            self.scroll = self.scroll_target;
            self.scroll_start = None;
            return false;
        }
        let e = crate::theme::Ease::Out.at(t);
        self.scroll = self.scroll_from + (self.scroll_target - self.scroll_from) * e;
        true
    }

    // ---- painting --------------------------------------------------------------

    pub fn paint(&mut self, g: &Gfx, t: &Theme, rect: Rect) {
        self.rect = rect;
        let left = (rect.w * self.margin_left / 100.0).clamp(16.0, rect.w * 0.35);
        let right = (rect.w * self.margin_right / 100.0).clamp(16.0, rect.w * 0.35);
        self.content_x = rect.x + left;
        self.content_w = (rect.w - left - right).max(40.0);
        self.relayout(g, t);
        // Scrolled into lines that still carry estimates: shape them before drawing.
        if self.pending {
            let (top, bottom) = (self.scroll - 200.0, self.scroll + rect.h + 200.0);
            let a = self.line_at_y(top.max(0.0));
            let unshaped = self.lines[a..].iter().take_while(|l| l.y < bottom).any(|l| l.shaped.layout.is_none() && l.table.is_none() && !l.deco.blank && l.src.len() > 0);
            if unshaped {
                self.fill_budget = 200;
                self.relayout(g, t);
            }
        }
        self.scroll = self.scroll.min(self.max_scroll());
        self.scroll_target = self.scroll_target.min(self.max_scroll());

        self.widget_hits.clear();
        g.clip(rect);
        let top = self.scroll;
        let bottom = self.scroll + rect.h;
        let first = self.line_at_y(top);
        let sel = self.buf.sel;
        let caret_line = self.line_of(sel.head);
        let oy = rect.y - self.scroll;
        let mut i = first;
        while i < self.lines.len() && self.lines[i].y < bottom {
            self.paint_line(g, t, i, oy, i == caret_line);
            i += 1;
        }
        if self.buf.text.is_empty() {
            if let Some(line) = self.lines.first() {
                let l = g.layout(mdwisp_core::i18n::t("从这里开始写作…"), crate::gfx::Style::new(crate::gfx::Stack::Read, self.look.size()), self.content_w);
                g.text(&l, self.content_x + line.shaped.indent, line.y + oy + line.shaped.pad_top + (line.shaped.text_h - crate::gfx::metrics(&l).h) / 2.0, t.text_disabled);
            }
        }
        // Caret.
        if self.focused && sel.empty() {
            let on = (self.blink.elapsed().as_millis() / 530) % 2 == 0;
            if on {
                if let Some((x, y, h)) = self.caret_at(sel.head) {
                    use mdwisp_core::settings::CursorStyle;
                    let cw = self.caret_at(self.buf.next_char(sel.head)).filter(|(nx, ny, _)| *ny == y && *nx > x).map_or(self.look.size() * 0.55, |(nx, _, _)| nx - x);
                    match self.cursor_style {
                        CursorStyle::Bar => g.fill(Rect::new(x - 0.5, y + oy, 1.5, h), t.accent),
                        CursorStyle::Block => g.fill(Rect::new(x, y + oy, cw, h), with_alpha(t.accent, 0.55)),
                        CursorStyle::Underline => g.fill(Rect::new(x, y + oy + h - 2.0, cw, 2.0), t.accent),
                    }
                }
            }
        }
        self.paint_scrollbar(g, t);
        g.unclip();
    }

    fn paint_line(&mut self, g: &Gfx, t: &Theme, i: usize, oy: f32, caret_line: bool) {
        let line = &self.lines[i];
        let y = line.y + oy;
        let x = self.content_x;
        let w = self.content_w;
        if let Some(ti) = line.table {
            let tv = &self.tables[ti];
            let sel = self.buf.sel;
            let caret_cell = if self.focused { tv.cell_of(sel.head) } else { None };
            table::paint(g, t, tv, x, y, caret_cell, None, sel.from()..sel.to());
            return;
        }
        let s = &line.shaped;
        let d = &line.deco;

        // Line backgrounds.
        if d.code != Code::None {
            let r = Rect::new(x, y, w, line.h);
            // A `surface` block, square.
            g.fill(r, t.bg_inset);
        } else if caret_line && self.highlight_line && self.focused {
            // The text rows only: a heading's spacing above it is not part of the line you are on.
            g.fill(Rect::new(x, y + s.pad_top, w, line.h - s.pad_top - s.pad_bottom), t.bg_hover);
        }
        // The two section levels carry the accent so a long document shows its structure at a glance:
        // H1 sits on a band with a bar hung in the margin (the text keeps its left edge), H2 over a rule.
        let rows = (y + s.pad_top, y + line.h - s.pad_bottom);
        if d.heading == 1 {
            g.fill(Rect::new(x - S4, rows.0 - S1, w + S4, rows.1 - rows.0 + S2), t.accent_wash);
            g.fill(Rect::new(x - S4, rows.0 - S1, 3.0, rows.1 - rows.0 + S2), t.accent);
        }
        if d.heading == 2 {
            g.fill(Rect::new(x, rows.1 + S1, w, 1.0), t.accent);
        }
        if d.quote {
            g.fill(Rect::new(x, y, 2.0, line.h), t.accent);
        }
        if d.rule {
            g.fill(Rect::new(x, y + s.pad_top, w, 1.0), t.border_strong);
        }
        if d.code == Code::Header {
            // The language picker shows on hover and while the caret is in the block.
            let block = self.deco.code_blocks.iter().position(|(r, _, _)| r.start == line.src.start || (r.start >= line.src.start && r.start <= line.src.end));
            let widget = self.deco.widgets.iter().find_map(|(r, wd)| match wd {
                super::deco::Widget::CodeLang { lang, detected, .. } if r.start == line.src.start || (r.start >= line.src.start && r.start <= line.src.end) => Some((lang.clone(), *detected)),
                _ => None,
            });
            if let (Some(bi), Some((lang, detected))) = (block, widget) {
                let (range, _, _) = &self.deco.code_blocks[bi];
                let head = self.buf.sel.head;
                let active = self.focused && head >= range.start && head <= range.end;
                if active || self.hover_block == Some(bi) {
                    let label = if !lang.is_empty() {
                        lang.split_whitespace().next().unwrap_or("").to_string()
                    } else {
                        match detected {
                            Some(l) => mdwisp_core::i18n::tf("自动检测 · {0}", &[&l]),
                            None => mdwisp_core::i18n::t("自动检测").into(),
                        }
                    };
                    let l = g.layout(&label, crate::gfx::Style::new(crate::gfx::Stack::Code, crate::theme::TEXT_XS), 400.0);
                    let m = crate::gfx::metrics(&l);
                    let cr = Rect::new(x + w - 16.0 - m.w - 16.0, y + (line.h - m.h - 8.0) / 2.0, m.w + 16.0, m.h + 8.0);
                    g.fill(cr, t.bg_overlay);
                    g.stroke_round(cr, 0.0, t.border_soft, 1.0);
                    g.text(&l, cr.x + 8.0, cr.y + 4.0, t.text_secondary);
                    self.widget_hits.push((cr, bi as u32));
                }
            }
        }

        let tx = x + s.indent;
        let ty = y + s.pad_top;
        let Some(layout) = &s.layout else { return };

        // Selection.
        let sel = self.buf.sel;
        if !sel.empty() && sel.from() <= line.src.end && sel.to() >= line.src.start {
            let a = src_to_disp_pos(&s.segs, sel.from().max(line.src.start), &self.buf.text);
            let b = src_to_disp_pos(&s.segs, sel.to().min(line.src.end), &self.buf.text);
            for r in range_rects(layout, a, b) {
                g.fill(Rect::new(tx + r.x, ty + r.y, r.w, r.h), t.accent_soft);
            }
            if sel.to() > line.src.end {
                let end_x = range_rects(layout, b, b + 1).last().map_or(0.0, |r| r.x);
                let (_, ly, lh) = last_line(layout);
                g.fill(Rect::new(tx + end_x, ty + ly, (s.width - end_x).max(6.0), lh), t.accent_soft);
            }
        }

        // Search matches; the selected one is stronger.
        if let Some(search) = &self.search {
            let first = search.matches.partition_point(|m| m.end <= line.src.start);
            for m in search.matches[first..].iter().take_while(|m| m.start <= line.src.end) {
                let a = src_to_disp_pos(&s.segs, m.start.max(line.src.start), &self.buf.text);
                let b = src_to_disp_pos(&s.segs, m.end.min(line.src.end), &self.buf.text);
                let selected = sel.from() == m.start && sel.to() == m.end;
                for r in range_rects(layout, a, b.max(a + 1)) {
                    let rr = Rect::new(tx + r.x, ty + r.y, r.w, r.h);
                    g.fill(rr, if selected { t.accent_line } else { t.accent_soft });
                    g.stroke_round(rr, 0.0, if selected { t.accent } else { t.accent_line }, 1.0);
                }
            }
        }

        // IME composition, underlined below its glyphs as the system would.
        if let Some(c) = self.composition.as_ref().filter(|c| c.start >= line.src.start && c.start <= line.src.end) {
            let a = src_to_disp_pos(&s.segs, c.start, &self.buf.text);
            let b = src_to_disp_pos(&s.segs, c.end.min(line.src.end), &self.buf.text);
            let rows = range_rects(layout, a, b);
            if let (Some(first), Some((_, cy, ch))) = (rows.first(), self.caret_at(c.start)) {
                let under = cy + ch - (line.y + s.pad_top) - first.y - 1.0;
                for r in &rows {
                    g.fill(Rect::new(tx + r.x, ty + r.y + under, r.w, 1.0), t.text_primary);
                }
            }
        }

        // Inline backgrounds.
        for (r, bg) in &s.backgrounds {
            let c = match bg {
                Bg::Code => t.bg_inset,
                Bg::Mark => with_alpha(t.hl_attr, 0.32),
            };
            for rr in range_rects(layout, r.start, r.end) {
                let pad = if *bg == Bg::Code { 0.15 * self.look.size() } else { 0.0 };
                let h = rr.h * 0.62;
                g.fill_round(Rect::new(tx + rr.x - 1.0, ty + rr.y + (rr.h - h) / 2.0 - pad * 0.3, rr.w + 2.0, h + pad), 0.0, c);
            }
        }

        if let Some((marker, _)) = &s.marker {
            let size = self.look.size();
            g.text(marker, tx - 1.35 * size, ty, t.accent);
        }
        g.text(layout, tx, ty, t.text_primary);

        // Widgets drawn into their reserved boxes.
        for (at, obj) in &s.objects {
            let mut px = 0.0;
            let mut py = 0.0;
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            unsafe { layout.HitTestTextPosition(*at, false, &mut px, &mut py, &mut m).unwrap() };
            match obj {
                Obj::Checkbox { checked, .. } => {
                    let b = Rect::new(tx + px, ty + m.top + (m.height - 15.0) / 2.0, 15.0, 15.0);
                    if *checked {
                        g.fill(b, t.accent);
                        g.line(b.x + 4.0, b.y + 7.8, b.x + 6.6, b.y + 10.4, t.on_accent, 2.0);
                        g.line(b.x + 6.6, b.y + 10.4, b.x + 11.2, b.y + 4.6, t.on_accent, 2.0);
                    } else {
                        g.stroke_round(b, 0.0, t.accent, 1.5);
                    }
                }
                Obj::Image { src, alt, range } => {
                    let r = Rect::new(tx + px, ty + m.top, m.width, m.height);
                    self.images.draw(g, t, self.doc_dir.as_deref(), src, alt, r);
                    if self.image_selected && self.buf.sel.head == range.start {
                        g.stroke_round(r.inset(-2.0), 0.0, t.accent_line, 2.0);
                    }
                }
            }
        }
    }

    /// The thumb's box for the current scroll position.
    pub fn thumb(&self) -> Option<Rect> {
        let total = self.content_height();
        if total <= self.rect.h + 1.0 {
            return None;
        }
        let r = self.rect;
        let h = (r.h * r.h / total).max(24.0);
        let y = r.y + (r.h - h) * (self.scroll / self.max_scroll().max(1.0));
        Some(Rect::new(r.right() - 10.0, y, 10.0, h))
    }

    /// Overlay scrollbar: 10 px track, a 4 px thumb inset by a transparent
    /// 3 px border, only visible while the pointer is over the editor.
    fn paint_scrollbar(&mut self, g: &Gfx, t: &Theme) {
        self.scrollbar = None;
        let Some(th) = self.thumb() else { return };
        let r = self.rect;
        self.scrollbar = Some(Rect::new(r.right() - 10.0, r.y, 10.0, r.h));
        if self.scrollbar_alpha > 0.0 {
            g.fill_round(th.inset(3.0), 2.0, with_alpha(t.border_strong, self.scrollbar_alpha));
        }
    }

    /// Scroll so the thumb's top follows a drag: `dy` pixels of thumb travel.
    pub fn thumb_drag(&mut self, start_scroll: f32, dy: f32) {
        let Some(th) = self.thumb() else { return };
        let travel = (self.rect.h - th.h).max(1.0);
        self.scroll_to(start_scroll + dy * self.max_scroll() / travel, false);
    }

    // ---- editing ---------------------------------------------------------------

    pub fn insert(&mut self, text: &str) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        let at = sel.from() + text.len();
        let kind = if text.len() <= 4 && sel.empty() { Kind::Type } else { Kind::Other };
        self.buf.replace(sel.from(), sel.to(), text, Sel::at(at), kind);
        self.after_edit();
    }

    pub fn replace(&mut self, from: usize, to: usize, text: &str, cursor: usize) {
        if self.read_only {
            return;
        }
        self.buf.replace(from, to, text, Sel::at(cursor), Kind::Other);
        self.after_edit();
    }

    pub(super) fn after_edit(&mut self) {
        self.goal_x = None;
        self.blink = Instant::now();
    }

    pub fn delete_back(&mut self, word: bool) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        if !sel.empty() {
            self.buf.replace(sel.from(), sel.to(), "", Sel::at(sel.from()), Kind::Delete);
        } else if sel.head > 0 {
            let p = if word { self.buf.word(sel.head, -1) } else { self.visual_step(sel.head, -1) };
            self.buf.replace(p, sel.head, "", Sel::at(p), Kind::Delete);
        }
        self.after_edit();
    }

    pub fn delete_forward(&mut self, word: bool) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        if !sel.empty() {
            self.buf.replace(sel.from(), sel.to(), "", Sel::at(sel.from()), Kind::Delete);
        } else if sel.head < self.buf.len() {
            let p = if word { self.buf.word(sel.head, 1) } else { self.buf.next_char(sel.head) };
            self.buf.replace(sel.head, p, "", Sel::at(sel.head), Kind::Delete);
        }
        self.after_edit();
    }

    /// One step left/right in what the reader sees: hidden markup is jumped over.
    pub fn visual_step(&self, pos: usize, dir: i32) -> usize {
        let i = self.line_of(pos);
        let Some(line) = self.lines.get(i) else { return pos };
        let s = &line.shaped;
        let text = &self.buf.text;
        if s.layout.is_none() || s.segs.is_empty() {
            return if dir < 0 { self.buf.prev_char(pos) } else { self.buf.next_char(pos) };
        }
        let d = src_to_disp_pos(&s.segs, pos, text);
        let first = s.segs.first().map_or(line.src.start, |s| s.src);
        if dir < 0 {
            if pos <= first || d == 0 {
                // Off the start of the line: to the end of the previous one.
                return if line.src.start == 0 { 0 } else { line.src.start - 1 };
            }
            let mut p = disp_to_src(&s.segs, &line.src, d - 1, text);
            // A surrogate pair is one character.
            if p == pos {
                p = self.buf.prev_char(pos);
            }
            p
        } else {
            let total: u32 = s.segs.last().map_or(0, |s| s.disp + s.disp_len);
            if d >= total {
                return if line.src.end >= text.len() { text.len() } else { line.src.end + 1 };
            }
            let p = disp_to_src(&s.segs, &line.src, d + 1, text);
            if p <= pos { self.buf.next_char(pos) } else { p }
        }
    }

    pub fn move_to(&mut self, pos: usize, extend: bool) {
        let pos = pos.min(self.buf.len());
        self.buf.sel = if extend { Sel { anchor: self.buf.sel.anchor, head: pos } } else { Sel::at(pos) };
        self.buf.isolate();
        self.blink = Instant::now();
    }

    pub fn move_h(&mut self, dir: i32, word: bool, extend: bool) {
        let sel = self.buf.sel;
        let pos = if !extend && !sel.empty() && !word {
            if dir < 0 { sel.from() } else { sel.to() }
        } else if word {
            self.buf.word(sel.head, dir)
        } else {
            self.visual_step(sel.head, dir)
        };
        self.goal_x = None;
        self.move_to(pos, extend);
    }

    pub fn move_v(&mut self, dir: i32, extend: bool, page: bool) {
        let Some((x, y, h)) = self.caret_at(self.buf.sel.head) else { return };
        let gx = *self.goal_x.get_or_insert(x);
        let dy = if page { self.rect.h - 40.0 } else { h * 0.5 + 2.0 };
        let target_y = if dir < 0 { y - dy } else { y + h + dy - h * 0.5 };
        let pos = if target_y < PAD_TOP && dir < 0 {
            0
        } else if target_y > self.lines.last().map_or(0.0, |l| l.y + l.h) {
            self.buf.len()
        } else {
            self.pos_at(gx, target_y - self.scroll + self.rect.y)
        };
        if page {
            self.scroll_to(self.scroll_target + dir as f32 * dy, false);
        }
        self.move_to(pos, extend);
        self.goal_x = Some(gx);
    }

    /// Home / End of the visual (wrapped) line.
    pub fn move_line_edge(&mut self, end: bool, extend: bool) {
        let head = self.buf.sel.head;
        let i = self.line_of(head);
        let Some(line) = self.lines.get(i) else { return };
        let s = &line.shaped;
        let pos = match &s.layout {
            Some(layout) => {
                let d = src_to_disp_pos(&s.segs, head, &self.buf.text);
                let (a, b) = visual_line_bounds(layout, d);
                let first_visible = s.segs.first().map_or(line.src.start, |s| s.src);
                if end {
                    disp_to_src(&s.segs, &line.src, b, &self.buf.text)
                } else if a == 0 {
                    // Home on the first row: past hidden prefixes, then to the true line start.
                    if head == first_visible { line.src.start } else { first_visible }
                } else {
                    disp_to_src(&s.segs, &line.src, a, &self.buf.text)
                }
            }
            None => if end { line.src.end } else { line.src.start },
        };
        self.goal_x = None;
        self.move_to(pos, extend);
    }

    pub fn select_all(&mut self) {
        self.buf.sel = Sel { anchor: 0, head: self.buf.len() };
    }

    pub fn select_word_at(&mut self, pos: usize) {
        let a = self.buf.word(self.buf.next_char(pos).min(self.buf.len()), -1).min(pos);
        let b = self.buf.word(a, 1);
        self.buf.sel = Sel { anchor: a, head: b.max(pos) };
    }

    pub fn selected_text(&self) -> &str {
        &self.buf.text[self.buf.sel.from()..self.buf.sel.to()]
    }

    pub fn link_at(&self, pos: usize) -> Option<&str> {
        self.deco.links.iter().find(|l| l.range.start <= pos && pos < l.range.end).map(|l| l.href.as_str())
    }

    pub fn code_block_at(&self, pos: usize) -> Option<&(Range<usize>, Range<usize>, String)> {
        self.deco.code_blocks.iter().find(|(r, _, _)| r.start <= pos && pos <= r.end)
    }
}

/// Decorations for source mode: plain lines, nothing hidden.
fn source_deco(src: &str) -> Deco {
    let mut d = Deco::default();
    d.starts.push(0);
    d.starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
    d.lines = vec![Default::default(); d.starts.len()];
    d
}

fn last_line(layout: &IDWriteTextLayout) -> (f32, f32, f32) {
    let mut count = 0;
    unsafe {
        let _ = layout.GetLineMetrics(None, &mut count);
        let mut lm = vec![DWRITE_LINE_METRICS::default(); count as usize];
        let _ = layout.GetLineMetrics(Some(&mut lm), &mut count);
        let y: f32 = lm.iter().take(lm.len().saturating_sub(1)).map(|m| m.height).sum();
        (0.0, y, lm.last().map_or(0.0, |m| m.height))
    }
}

/// Displayed [start, end) of the wrapped row containing `d`.
fn visual_line_bounds(layout: &IDWriteTextLayout, d: u32) -> (u32, u32) {
    let mut count = 0;
    unsafe {
        let _ = layout.GetLineMetrics(None, &mut count);
        let mut lm = vec![DWRITE_LINE_METRICS::default(); count as usize];
        let _ = layout.GetLineMetrics(Some(&mut lm), &mut count);
        let mut a = 0;
        for (i, m) in lm.iter().enumerate() {
            let b = a + m.length;
            let last = i + 1 == lm.len();
            if d < b || last {
                let end = if last { b - m.newlineLength } else { b - m.trailingWhitespaceLength.min(m.length) };
                return (a, end.max(a));
            }
            a = b;
        }
        (0, 0)
    }
}

/// Rectangles covering displayed text [a, b), relative to the layout origin.
pub fn range_rects(layout: &IDWriteTextLayout, a: u32, b: u32) -> Vec<Rect> {
    if b <= a {
        return vec![];
    }
    unsafe {
        let mut n = 0;
        let _ = layout.HitTestTextRange(a, b - a, 0.0, 0.0, None, &mut n);
        let mut m = vec![DWRITE_HIT_TEST_METRICS::default(); n as usize];
        if layout.HitTestTextRange(a, b - a, 0.0, 0.0, Some(&mut m), &mut n).is_err() {
            return vec![];
        }
        m.iter().map(|m| Rect::new(m.left, m.top, m.width, m.height)).collect()
    }
}
