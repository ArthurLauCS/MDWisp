//! Markdown-aware typing: list continuation, fences, auto-pairs, indentation,
//! table-cell navigation — CodeMirror's markdown keymap, ported.

use mdwisp_table::model::{CellPos, cell_at, cell_range};
use mdwisp_text::ops::{Typed, auto_pair, continuation_prefix};

use super::buffer::{Kind, Sel};
use super::deco::Code;
use super::view::View;

/// List or quote markup at the start of a line: `  - [ ] `, `1. `, `> `.
fn markup_prefix(line: &str) -> usize {
    let b = line.as_bytes();
    let mut i = b.iter().take_while(|c| **c == b' ').count();
    if b.get(i) == Some(&b'>') {
        i += 1;
        if b.get(i) == Some(&b' ') {
            i += 1;
        }
        return i;
    }
    let start = i;
    if matches!(b.get(i), Some(b'-' | b'*' | b'+')) {
        i += 1;
    } else {
        i += b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if i == start || !matches!(b.get(i), Some(b'.' | b')')) {
            return 0;
        }
        i += 1;
    }
    if b.get(i) != Some(&b' ') {
        return 0;
    }
    i += 1;
    if line[i..].starts_with("[ ] ") || line[i..].starts_with("[x] ") || line[i..].starts_with("[X] ") {
        i += 4;
    }
    i
}

impl View {
    pub fn type_text(&mut self, text: &str, auto: bool) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        let mut chars = text.chars();
        if let (Some(ch), None, true) = (chars.next(), chars.next(), auto) {
            let in_code = self.in_code(sel.head);
            if !in_code {
                let range = (!sel.empty()).then(|| sel.from()..sel.to());
                match auto_pair(&self.buf.text, sel.head, range, ch) {
                    Some(Typed::SkipClose) => {
                        self.move_to(sel.head + ch.len_utf8(), false);
                        return;
                    }
                    Some(Typed::Edit(r)) => {
                        let (from, to, ins) = super::buffer::minimal_change(&self.buf.text, &r.text);
                        let s = r.selection.map_or(Sel::at(r.cursor), |s| Sel { anchor: s.start, head: s.end });
                        self.buf.replace(from, to, &ins, s, Kind::Type);
                        return;
                    }
                    None => {}
                }
            }
        }
        self.insert(text);
    }

    fn in_code(&self, pos: usize) -> bool {
        let l = self.deco.line_of(pos);
        self.deco.lines.get(l).is_some_and(|d| d.code != Code::None)
            || self.deco.marks.iter().any(|(r, s)| *s == super::deco::Style::Code && r.start < pos && pos < r.end)
    }

    /// Enter: continue lists and quotes, open fences, move between table rows.
    pub fn enter(&mut self, smart: bool, shift: bool) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        if let Some(ctx) = self.deco.tables.iter().find(|c| c.start <= sel.head && sel.head <= c.end).cloned() {
            // Enter moves between rows like a spreadsheet; line breaks go through Alt+Enter.
            if let Some(p) = cell_at(&ctx, sel.head) {
                let row = if shift { p.row.checked_sub(1) } else { Some(p.row + 1) };
                if let Some(row) = row.filter(|r| *r <= ctx.body_rows) {
                    let r = cell_range(&ctx, CellPos { row, col: p.col });
                    self.move_to(r.to, false);
                }
                return;
            }
        }
        let ls = self.buf.line_start(sel.head);
        let le = self.buf.line_end(sel.head);
        let line = self.buf.text[ls..le].to_string();
        // An unfinished fence at its end gets a body and a closing fence.
        if sel.empty() && sel.head == le {
            let t = line.trim_start_matches(' ');
            let indent = &line[..line.len() - t.len()];
            let fence_len = t.len() - t.trim_start_matches(['`', '~']).len();
            let fence = &t[..fence_len];
            let open = fence_len >= 3 && fence.chars().all(|c| c == fence.chars().next().unwrap()) && !t[fence_len..].contains(['`', ' ']);
            let unclosed = self.deco.code_blocks.iter().any(|(r, _, _)| r.start == ls + indent.len() && !self.buf.text[r.clone()].trim_end().ends_with(fence)) || !self.deco.code_blocks.iter().any(|(r, _, _)| r.start <= ls && ls < r.end);
            if open && unclosed && indent.len() <= 3 {
                let ins = format!("\n{indent}\n{indent}{fence}");
                let at = le + 1 + indent.len();
                self.buf.replace(le, le, &ins, Sel::at(at), Kind::Other);
                return;
            }
        }
        if smart && !self.in_code(sel.head) {
            let before = &self.buf.text[ls..sel.from()];
            if markup_prefix(&line) > 0 {
                match continuation_prefix(before) {
                    Some(prefix) => {
                        let ins = format!("\n{prefix}");
                        self.buf.replace(sel.from(), sel.to(), &ins, Sel::at(sel.from() + ins.len()), Kind::Other);
                    }
                    // Enter on an empty item ends the list.
                    None => self.buf.replace(ls, le, "", Sel::at(ls), Kind::Other),
                }
                return;
            }
        }
        // Inside code, keep the line's indentation.
        let indent = if self.in_code(sel.head) { line.len() - line.trim_start_matches([' ', '\t']).len() } else { 0 };
        let ins = format!("\n{}", &line[..indent.min(sel.from() - ls)]);
        self.buf.replace(sel.from(), sel.to(), &ins, Sel::at(sel.from() + ins.len()), Kind::Other);
    }

    /// Backspace: removes list markup first, a whole rule line, then characters.
    pub fn backspace(&mut self, smart: bool, word: bool) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        if sel.empty() && !word {
            let ls = self.buf.line_start(sel.head);
            let le = self.buf.line_end(sel.head);
            let line = &self.buf.text[ls..le];
            let n = markup_prefix(line);
            if smart && n > 0 && sel.head == ls + n && !self.in_code(sel.head) {
                self.buf.replace(ls, ls + n, "", Sel::at(ls), Kind::Delete);
                return;
            }
            // A horizontal rule goes as one line, from inside or just after it.
            let row = if sel.head == ls && ls > 0 { self.deco.line_of(ls - 1) } else { self.deco.line_of(sel.head) };
            if self.deco.lines.get(row).is_some_and(|d| d.rule) {
                let s = self.deco.starts[row];
                let e = self.deco.line_end(row, &self.buf.text);
                let (from, to) = if e < self.buf.len() { (s, e + 1) } else { (s.saturating_sub(1), e) };
                self.buf.replace(from, to, "", Sel::at(from), Kind::Delete);
                return;
            }
        }
        self.delete_back(word);
    }

    /// Tab / Shift+Tab: indent or outdent the selected lines; Tab in a table goes to the next cell.
    pub fn tab(&mut self, back: bool, unit: usize) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        if let Some(ctx) = self.deco.tables.iter().find(|c| c.start <= sel.head && sel.head <= c.end).cloned() {
            if let Some(p) = cell_at(&ctx, sel.head) {
                let n = ctx.cols * (ctx.body_rows + 1);
                let i = p.row * ctx.cols + p.col;
                let j = if back { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < n) };
                if let Some(j) = j {
                    let r = cell_range(&ctx, CellPos { row: j / ctx.cols, col: j % ctx.cols });
                    self.buf.sel = Sel { anchor: r.from, head: r.to };
                }
                return;
            }
        }
        let first = self.buf.line_start(sel.from());
        let last_end = self.buf.line_end(sel.to());
        let block = self.buf.text[first..last_end].to_string();
        let pad = " ".repeat(unit);
        let mut out = String::new();
        let mut delta_head: isize = 0;
        let mut delta_anchor: isize = 0;
        let mut pos = first;
        for (i, line) in block.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let change: isize = if back {
                let n = line.len() - line.trim_start_matches(' ').len();
                -(n.min(unit) as isize)
            } else {
                unit as isize
            };
            if back {
                out.push_str(&line[(-change) as usize..]);
            } else {
                out.push_str(&pad);
                out.push_str(line);
            }
            if pos <= sel.head {
                delta_head += change;
            }
            if pos <= sel.anchor {
                delta_anchor += change;
            }
            pos += line.len() + 1;
        }
        let head = (sel.head as isize + delta_head).max(first as isize) as usize;
        let anchor = (sel.anchor as isize + delta_anchor).max(first as isize) as usize;
        self.buf.replace(first, last_end, &out, Sel { anchor, head }, Kind::Other);
    }

    /// Click below the last line of a code block ending the document: continue after it.
    pub fn leave_code_block(&mut self, end: usize) {
        let text = &self.buf.text;
        let next = (end < text.len()).then(|| end + 1);
        match next {
            Some(at) => {
                let next_line_empty = text[at..].starts_with('\n') || at == text.len();
                let ins = if next_line_empty { "" } else { "\n" };
                self.buf.replace(at, at, ins, Sel::at(at), Kind::Other);
            }
            None => self.buf.replace(end, end, "\n\n", Sel::at(end + 2), Kind::Other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::markup_prefix;

    #[test]
    fn finds_list_and_quote_markup() {
        assert_eq!(markup_prefix("- a"), 2);
        assert_eq!(markup_prefix("  - [ ] task"), 8);
        assert_eq!(markup_prefix("12. x"), 4);
        assert_eq!(markup_prefix("> q"), 2);
        assert_eq!(markup_prefix("-x"), 0);
        assert_eq!(markup_prefix("plain"), 0);
    }
}
