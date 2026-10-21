//! The document text, its selection and undo history. Offsets are UTF-8 bytes
//! and always sit on char boundaries.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sel {
    pub anchor: usize,
    pub head: usize,
}

impl Sel {
    pub fn at(pos: usize) -> Sel {
        Sel { anchor: pos, head: pos }
    }
    pub fn from(self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn to(self) -> usize {
        self.anchor.max(self.head)
    }
    pub fn empty(self) -> bool {
        self.anchor == self.head
    }
}

/// One replacement: `text[at..at + removed.len()]` became `inserted`.
#[derive(Clone, Debug)]
struct Edit {
    at: usize,
    removed: String,
    inserted: String,
}

#[derive(Clone, Debug)]
struct Step {
    edits: Vec<Edit>,
    before: Sel,
    after: Sel,
    kind: Kind,
    time: Instant,
}

/// Consecutive edits of the same kind within the grouping delay undo together,
/// like CodeMirror's history.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Type,
    Delete,
    Other,
}

const GROUP: Duration = Duration::from_millis(500);

pub struct Buffer {
    pub text: String,
    pub sel: Sel,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Bumped on every text change; views key their caches on it.
    pub version: u64,
    /// Stop the next edit from merging into the previous step.
    isolate: bool,
}

impl Buffer {
    pub fn new(text: String) -> Buffer {
        Buffer { text, sel: Sel::default(), undo: vec![], redo: vec![], version: 0, isolate: false }
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Replace `from..to` and leave the selection at `sel`.
    pub fn replace(&mut self, from: usize, to: usize, insert: &str, sel: Sel, kind: Kind) {
        if from == to && insert.is_empty() {
            self.sel = sel;
            return;
        }
        let edit = Edit { at: from, removed: self.text[from..to].to_string(), inserted: insert.to_string() };
        self.text.replace_range(from..to, insert);
        self.version += 1;
        self.redo.clear();
        let now = Instant::now();
        let merge = !self.isolate
            && kind != Kind::Other
            && self.undo.last().is_some_and(|s| s.kind == kind && now - s.time < GROUP && s.after == self.sel);
        if merge {
            let s = self.undo.last_mut().unwrap();
            s.edits.push(edit);
            s.after = sel;
            s.time = now;
        } else {
            self.undo.push(Step { edits: vec![edit], before: self.sel, after: sel, kind, time: now });
        }
        self.isolate = false;
        self.sel = sel;
    }

    /// Replace the whole text as one isolated step, touching only the part
    /// that changed so the caret stays put where the text did not move.
    pub fn set_text(&mut self, next: &str) {
        let (from, to, insert) = minimal_change(&self.text, next);
        let map = |p: usize| if p <= from { p } else if p >= to { p - to + from + insert.len() } else { from + insert.len() };
        let sel = Sel { anchor: map(self.sel.anchor).min(next.len()), head: map(self.sel.head).min(next.len()) };
        self.isolate = true;
        self.replace(from, to, &insert, sel, Kind::Other);
        self.isolate = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        for e in step.edits.iter().rev() {
            self.text.replace_range(e.at..e.at + e.inserted.len(), &e.removed);
        }
        self.sel = step.before;
        self.version += 1;
        self.redo.push(step);
        self.isolate = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        for e in &step.edits {
            self.text.replace_range(e.at..e.at + e.removed.len(), &e.inserted);
        }
        self.sel = step.after;
        self.version += 1;
        self.undo.push(step);
        self.isolate = true;
        true
    }

    pub fn isolate(&mut self) {
        self.isolate = true;
    }

    /// Change the text outside the undo history: IME composition, spliced back
    /// out before anything is recorded.
    pub fn splice(&mut self, from: usize, to: usize, insert: &str, sel: Sel) {
        self.text.replace_range(from..to, insert);
        self.version += 1;
        self.sel = sel;
    }

    // ---- positions -----------------------------------------------------------

    pub fn line_start(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    pub fn line_end(&self, pos: usize) -> usize {
        self.text[pos..].find('\n').map_or(self.text.len(), |i| pos + i)
    }

    /// 1-based line number of `pos`.
    pub fn line_number(&self, pos: usize) -> usize {
        self.text.as_bytes()[..pos].iter().filter(|b| **b == b'\n').count() + 1
    }

    pub fn prev_char(&self, pos: usize) -> usize {
        self.text[..pos].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    pub fn next_char(&self, pos: usize) -> usize {
        self.text[pos..].chars().next().map_or(pos, |c| pos + c.len_utf8())
    }

    /// Word boundary in `dir` (-1 / +1): skips spaces, then one run of the same class.
    pub fn word(&self, pos: usize, dir: i32) -> usize {
        let class = |c: char| if c.is_whitespace() { 0 } else if c.is_alphanumeric() || c == '_' { if (c as u32) < 0x2e80 { 1 } else { 3 } } else { 2 };
        let mut p = pos;
        if dir < 0 {
            let mut it = self.text[..p].char_indices().rev().peekable();
            while let Some(&(i, c)) = it.peek() {
                if class(c) != 0 || c == '\n' {
                    break;
                }
                p = i;
                it.next();
            }
            let Some(&(_, first)) = it.peek() else { return p };
            if first == '\n' {
                return if p == pos { p - 1 } else { p };
            }
            let k = class(first);
            for (i, c) in it {
                if class(c) != k {
                    break;
                }
                p = i;
            }
        } else {
            let mut it = self.text[p..].char_indices().map(|(i, c)| (i + pos, c)).peekable();
            while let Some(&(i, c)) = it.peek() {
                if class(c) != 0 || c == '\n' {
                    break;
                }
                p = i + c.len_utf8();
                it.next();
            }
            let Some(&(_, first)) = it.peek() else { return p };
            if first == '\n' {
                return if p == pos { p + 1 } else { p };
            }
            let k = class(first);
            for (i, c) in it {
                if class(c) != k {
                    break;
                }
                p = i + c.len_utf8();
            }
        }
        p
    }
}

/// The smallest `(from, to, insert)` turning `a` into `b` (common prefix and suffix kept).
pub fn minimal_change(a: &str, b: &str) -> (usize, usize, String) {
    let mut pre = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(pre) || !b.is_char_boundary(pre) {
        pre -= 1;
    }
    let max_suf = a.len().min(b.len()) - pre;
    let mut suf = a.bytes().rev().zip(b.bytes().rev()).take(max_suf).take_while(|(x, y)| x == y).count();
    while !a.is_char_boundary(a.len() - suf) || !b.is_char_boundary(b.len() - suf) {
        suf -= 1;
    }
    (pre, a.len() - suf, b[pre..b.len() - suf].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_groups_and_undoes_together() {
        let mut b = Buffer::new(String::new());
        for (i, c) in "abc".chars().enumerate() {
            b.replace(i, i, &c.to_string(), Sel::at(i + 1), Kind::Type);
        }
        assert_eq!(b.text, "abc");
        assert!(b.undo());
        assert_eq!(b.text, "");
        assert!(b.redo());
        assert_eq!((b.text.as_str(), b.sel), ("abc", Sel::at(3)));
    }

    #[test]
    fn different_kinds_do_not_merge() {
        let mut b = Buffer::new("ab".into());
        b.sel = Sel::at(2);
        b.replace(2, 2, "c", Sel::at(3), Kind::Type);
        b.replace(2, 3, "", Sel::at(2), Kind::Delete);
        b.undo();
        assert_eq!(b.text, "abc");
    }

    #[test]
    fn set_text_keeps_the_caret_where_nothing_moved() {
        let mut b = Buffer::new("hello world".into());
        b.sel = Sel::at(9);
        b.set_text("hello big world");
        assert_eq!(b.sel, Sel::at(13));
        b.undo();
        assert_eq!(b.text, "hello world");
    }

    #[test]
    fn minimal_change_respects_char_boundaries() {
        assert_eq!(minimal_change("中文", "中字"), (3, 6, "字".into()));
        assert_eq!(minimal_change("aaa", "aa"), (2, 3, String::new()));
    }

    #[test]
    fn word_motion() {
        let b = Buffer::new("foo bar_1, 中文".into());
        assert_eq!(b.word(0, 1), 3);
        assert_eq!(b.word(3, 1), 9);
        assert_eq!(b.word(9, 1), 10);
        assert_eq!(b.word(b.len(), -1), 11);
        assert_eq!(b.word(9, -1), 4);
    }
}
