//! The live table: a markdown table drawn as a grid. Cells show rendered
//! inline markdown; the cell holding the caret shows its raw source and is
//! edited in place, so the document stays the only copy of the table.

use std::ops::Range;

use mdwisp_table::model::{Align, CellPos, TableContext, cell_at, cell_range};
use windows::Win32::Graphics::DirectWrite::*;

use super::deco::{self, Focus, LineDeco};
use super::layout::{self, Cache, Look, Shaped, disp_to_src, src_to_disp_pos};
use crate::gfx::{Gfx, Rect};
use crate::theme::Theme;

pub const PAD_Y: f32 = 16.0;
const CELL_PX: f32 = 16.0;
const CELL_PY: f32 = 8.0;
const MIN_COL: f32 = 64.0;

pub struct Cell {
    pub range: Range<usize>,
    /// Shaped against `text`, which starts at `range.start` in the document.
    pub shaped: Shaped,
    pub text: String,
    pub raw: bool,
}

pub struct TableView {
    pub ctx: TableContext,
    pub cols: Vec<f32>,
    pub rows: Vec<f32>,
    pub cells: Vec<Vec<Cell>>,
    pub h: f32,
}

impl TableView {
    pub fn width(&self) -> f32 {
        self.cols.iter().sum()
    }

    /// Cell box relative to the table's top-left (inside the vertical padding).
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        let x: f32 = self.cols[..col].iter().sum();
        let y: f32 = self.rows[..row].iter().sum();
        Rect::new(x, y, self.cols[col], self.rows[row])
    }

    pub fn cell_at_point(&self, x: f32, y: f32) -> (usize, usize) {
        let mut row = self.rows.len() - 1;
        let mut acc = 0.0;
        for (i, h) in self.rows.iter().enumerate() {
            if y < acc + h {
                row = i;
                break;
            }
            acc += h;
        }
        let mut col = self.cols.len() - 1;
        acc = 0.0;
        for (i, w) in self.cols.iter().enumerate() {
            if x < acc + w {
                col = i;
                break;
            }
            acc += w;
        }
        (row, col)
    }

    /// Source position under a point relative to the table's top-left.
    pub fn pos_at(&self, x: f32, y: f32) -> usize {
        let (row, col) = self.cell_at_point(x, y);
        let c = &self.cells[row][col];
        let r = self.cell_rect(row, col);
        let Some(l) = &c.shaped.layout else { return c.range.start };
        let mut trailing = windows::core::BOOL(0);
        let mut inside = windows::core::BOOL(0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        unsafe { l.HitTestPoint(x - r.x - CELL_PX, (y - r.y - CELL_PY).max(0.0), &mut trailing, &mut inside, &mut m).unwrap() };
        let d = m.textPosition + if trailing.as_bool() { m.length } else { 0 };
        c.range.start + disp_to_src(&c.shaped.segs, &(0..c.text.len()), d, &c.text)
    }

    /// Caret box for `pos` relative to the table's top-left.
    pub fn caret_at(&self, pos: usize) -> Option<(f32, f32, f32)> {
        let p = cell_at(&self.ctx, pos)?;
        let c = self.cells.get(p.row)?.get(p.col)?;
        let r = self.cell_rect(p.row, p.col);
        let l = c.shaped.layout.as_ref()?;
        let d = src_to_disp_pos(&c.shaped.segs, pos.saturating_sub(c.range.start).min(c.text.len()), &c.text);
        let (mut x, mut y) = (0.0, 0.0);
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        unsafe { l.HitTestTextPosition(d, false, &mut x, &mut y, &mut m).ok()? };
        Some((r.x + CELL_PX + x, r.y + CELL_PY + m.top, m.height))
    }

    pub fn cell_of(&self, pos: usize) -> Option<CellPos> {
        cell_at(&self.ctx, pos)
    }
}

pub fn build(g: &Gfx, cache: &mut Cache, src: &str, ctx: &TableContext, width: f32, look: &Look, t: &Theme, caret: Option<usize>) -> TableView {
    let rows_n = ctx.body_rows + 1;
    let focus_cell = caret.and_then(|c| cell_at(ctx, c));
    let none = Focus { from: usize::MAX, to: usize::MAX, head: usize::MAX, searching: false };
    let mut cells: Vec<Vec<Cell>> = Vec::with_capacity(rows_n);
    // First pass at an unconstrained width gives each column its max-content width.
    let mut max_w = vec![MIN_COL; ctx.cols];
    let mut min_w = vec![MIN_COL; ctx.cols];
    for row in 0..rows_n {
        let mut line = vec![];
        for col in 0..ctx.cols {
            let r = cell_range(ctx, CellPos { row, col });
            let range = r.from..r.to;
            let text = src[range.clone()].to_string();
            let raw = focus_cell == Some(CellPos { row, col });
            let ld = LineDeco { table: if row == 0 { 1 } else { 2 }, ..Default::default() };
            let d = if raw { plain(&text) } else { deco::decorate(&text, &none) };
            let shaped = layout::shape(g, cache, &text, &d, 0..text.len(), &ld, 1.0e5, look, t, &|_| None);
            if let Some(l) = &shaped.layout {
                let w = crate::gfx::metrics(l).w + CELL_PX * 2.0 + 1.0;
                max_w[col] = max_w[col].max(w);
                let min = unsafe { l.DetermineMinWidth().unwrap_or(0.0) };
                min_w[col] = min_w[col].max(min + CELL_PX * 2.0 + 1.0);
            }
            line.push(Cell { range, shaped, text, raw });
        }
        cells.push(line);
    }
    let cols = distribute(&min_w, &max_w, width);
    // Second pass: wrap each cell to its column.
    let mut rows = vec![0.0f32; rows_n];
    for (row, line) in cells.iter_mut().enumerate() {
        for (col, c) in line.iter_mut().enumerate() {
            let ld = LineDeco { table: if row == 0 { 1 } else { 2 }, ..Default::default() };
            let d = if c.raw { plain(&c.text) } else { deco::decorate(&c.text, &none) };
            let inner = (cols[col] - CELL_PX * 2.0).max(1.0);
            c.shaped = layout::shape(g, cache, &c.text, &d, 0..c.text.len(), &ld, inner, look, t, &|_| None);
            if let Some(l) = &c.shaped.layout {
                let align = match ctx.aligns.get(col) {
                    Some(Align::Center) => DWRITE_TEXT_ALIGNMENT_CENTER,
                    Some(Align::Right) => DWRITE_TEXT_ALIGNMENT_TRAILING,
                    _ => DWRITE_TEXT_ALIGNMENT_LEADING,
                };
                unsafe { l.SetTextAlignment(align).unwrap() };
            }
            rows[row] = rows[row].max(c.shaped.text_h + CELL_PY * 2.0 + 1.0);
        }
    }
    let h = rows.iter().sum::<f32>() + PAD_Y * 2.0;
    TableView { ctx: ctx.clone(), cols, rows, cells, h }
}

/// CSS automatic table layout, at `width: 100%`: every column gets at least
/// its minimum, the rest is shared in proportion to how much more it wants.
fn distribute(min: &[f32], max: &[f32], width: f32) -> Vec<f32> {
    let sum_min: f32 = min.iter().sum();
    let sum_max: f32 = max.iter().sum();
    if sum_max <= width {
        let extra = width - sum_max;
        return max.iter().map(|m| m + extra * m / sum_max).collect();
    }
    if sum_min >= width {
        return min.to_vec();
    }
    let room = width - sum_min;
    let want: f32 = max.iter().zip(min).map(|(a, b)| a - b).sum();
    max.iter().zip(min).map(|(a, b)| b + room * (a - b) / want.max(1e-3)).collect()
}

fn plain(text: &str) -> deco::Deco {
    let mut d = deco::Deco::default();
    d.starts.push(0);
    d.lines.push(Default::default());
    let _ = text;
    d
}

pub fn paint(g: &Gfx, t: &Theme, tv: &TableView, x: f32, y: f32, caret_cell: Option<CellPos>, hover_row: Option<usize>, sel: Range<usize>) {
    let y = y + PAD_Y;
    let w = tv.width();
    let h: f32 = tv.rows.iter().sum();
    let outer = Rect::new(x, y, w, h);
    g.fill_round(outer, 8.0, t.bg_base);
    // Header row background, clipped to the rounded frame.
    g.clip(Rect::new(x, y, w, tv.rows[0]));
    g.fill_round(Rect::new(x, y, w, tv.rows[0] + 8.0), 8.0, t.bg_raised);
    g.unclip();
    let mut ry = y;
    for (row, rh) in tv.rows.iter().enumerate() {
        if row > 0 && hover_row == Some(row) {
            g.fill(Rect::new(x + 1.0, ry, w - 2.0, *rh - 1.0), t.bg_hover);
        }
        if row + 1 < tv.rows.len() {
            g.fill(Rect::new(x + 1.0, ry + rh - 1.0, w - 2.0, 1.0), if row == 0 { t.border_strong } else { t.border_soft });
        }
        let mut cx = x;
        for (col, cw) in tv.cols.iter().enumerate() {
            let c = &tv.cells[row][col];
            if !sel.is_empty() && sel.start < c.range.end && sel.end > c.range.start {
                if let Some(l) = &c.shaped.layout {
                    let a = src_to_disp_pos(&c.shaped.segs, sel.start.saturating_sub(c.range.start), &c.text);
                    let b = src_to_disp_pos(&c.shaped.segs, (sel.end - c.range.start.min(sel.end)).min(c.text.len()), &c.text);
                    for r in super::view::range_rects(l, a, b) {
                        g.fill(Rect::new(cx + CELL_PX + r.x, ry + CELL_PY + r.y, r.w, r.h), t.accent_soft);
                    }
                }
            }
            if caret_cell == Some(CellPos { row, col }) {
                g.stroke_round(Rect::new(cx, ry, *cw, *rh), 0.0, t.accent_line, 1.0);
            }
            if let Some(l) = &c.shaped.layout {
                for (r, _) in &c.shaped.backgrounds {
                    for rr in super::view::range_rects(l, r.start, r.end) {
                        g.fill_round(Rect::new(cx + CELL_PX + rr.x - 1.0, ry + CELL_PY + rr.y + rr.h * 0.19, rr.w + 2.0, rr.h * 0.62), 5.0, t.bg_inset);
                    }
                }
                g.text(l, cx + CELL_PX, ry + CELL_PY, t.text_primary);
            }
            cx += cw;
        }
        ry += rh;
    }
    g.stroke_round(outer, 8.0, t.border_soft, 1.0);
}
