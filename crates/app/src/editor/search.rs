//! Find and replace inside the document, like CodeMirror's search panel:
//! plain, case-sensitive, regular-expression and whole-word matching.

use std::ops::Range;

use regex::RegexBuilder;

use super::buffer::{Kind, Sel};
use super::view::View;
use crate::shell::widgets::Field;

#[derive(Default)]
pub struct Search {
    pub find: Field,
    pub replace: Field,
    pub case: bool,
    pub regex: bool,
    pub word: bool,
    /// Which field has the keyboard: 0 find, 1 replace, None the document.
    pub focus: Option<u8>,
    pub matches: Vec<Range<usize>>,
    key: Option<(String, bool, bool, bool, u64)>,
    pub invalid: bool,
}

impl Search {
    /// Recompute matches when the query or the text changed.
    pub fn refresh(&mut self, text: &str, version: u64) {
        let key = (self.find.text.clone(), self.case, self.regex, self.word, version);
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.matches.clear();
        self.invalid = false;
        if self.find.text.is_empty() {
            return;
        }
        let pattern = if self.regex { self.find.text.clone() } else { regex::escape(&self.find.text) };
        let pattern = if self.word { format!(r"\b(?:{pattern})\b") } else { pattern };
        match RegexBuilder::new(&pattern).case_insensitive(!self.case).multi_line(true).build() {
            Ok(re) => {
                // Cap the work on pathological patterns over huge documents.
                self.matches = re.find_iter(text).filter(|m| !m.is_empty()).take(100_000).map(|m| m.range()).collect();
            }
            Err(_) => self.invalid = true,
        }
    }
}

impl View {
    /// Select the next (or previous) match from the selection, wrapping around.
    pub fn search_step(&mut self, dir: i32) -> bool {
        let Some(s) = &mut self.search else { return false };
        s.refresh(&self.buf.text, self.buf.version);
        if s.matches.is_empty() {
            return false;
        }
        let sel = self.buf.sel;
        let m = if dir > 0 {
            s.matches.iter().find(|m| m.start >= sel.to() && !(m.start == sel.from() && m.end == sel.to())).or(s.matches.first())
        } else {
            s.matches.iter().rev().find(|m| m.end <= sel.from()).or(s.matches.last())
        }
        .cloned();
        if let Some(m) = m {
            self.buf.sel = Sel { anchor: m.start, head: m.end };
            self.buf.isolate();
            self.scroll_into_view(m.start, true);
        }
        true
    }

    fn replacement(&self, m: &Range<usize>) -> String {
        let Some(s) = &self.search else { return String::new() };
        if !s.regex {
            return s.replace.text.clone();
        }
        let re = RegexBuilder::new(&s.find.text).case_insensitive(!s.case).multi_line(true).build();
        match re {
            Ok(re) => re.replace(&self.buf.text[m.clone()], s.replace.text.as_str()).into_owned(),
            Err(_) => s.replace.text.clone(),
        }
    }

    /// Replace the selected match, then move on to the next.
    pub fn search_replace(&mut self) {
        if self.read_only {
            return;
        }
        let sel = self.buf.sel;
        let current = self.search.as_ref().and_then(|s| s.matches.iter().find(|m| m.start == sel.from() && m.end == sel.to()).cloned());
        if let Some(m) = current {
            let rep = self.replacement(&m);
            self.buf.replace(m.start, m.end, &rep, Sel::at(m.start + rep.len()), Kind::Other);
            self.buf.isolate();
        }
        self.search_step(1);
    }

    pub fn search_replace_all(&mut self) -> usize {
        if self.read_only {
            return 0;
        }
        let Some(s) = &mut self.search else { return 0 };
        s.refresh(&self.buf.text, self.buf.version);
        let matches = s.matches.clone();
        if matches.is_empty() {
            return 0;
        }
        let mut out = String::with_capacity(self.buf.text.len());
        let mut last = 0;
        for m in &matches {
            out.push_str(&self.buf.text[last..m.start]);
            out.push_str(&self.replacement(m));
            last = m.end;
        }
        out.push_str(&self.buf.text[last..]);
        // One undo step for the whole replace-all.
        self.buf.isolate();
        let head = self.buf.sel.head.min(out.len());
        let head = (0..=head).rev().find(|i| out.is_char_boundary(*i)).unwrap_or(0);
        let (from, to, ins) = super::buffer::minimal_change(&self.buf.text, &out);
        self.buf.replace(from, to, &ins, Sel::at(head), Kind::Other);
        self.buf.isolate();
        matches.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_plain_case_word_and_regex_matches() {
        let mut s = Search { find: Field::new("ab"), ..Default::default() };
        s.refresh("ab AB cab", 1);
        assert_eq!(s.matches, [0..2, 3..5, 7..9]);
        s.case = true;
        s.refresh("ab AB cab", 1);
        assert_eq!(s.matches.len(), 2);
        s.word = true;
        s.refresh("ab AB cab", 1);
        assert_eq!(s.matches, [0..2]);
        let mut r = Search { find: Field::new("a+("), regex: true, ..Default::default() };
        r.refresh("aaa", 1);
        assert!(r.invalid);
    }

    #[test]
    fn replace_all_is_one_step() {
        let mut v = View::new("a-b-c".into(), None);
        v.search = Some(Search { find: Field::new("-"), replace: Field::new("+"), ..Default::default() });
        assert_eq!(v.search_replace_all(), 2);
        assert_eq!(v.buf.text, "a+b+c");
        v.buf.undo();
        assert_eq!(v.buf.text, "a-b-c");
    }
}
