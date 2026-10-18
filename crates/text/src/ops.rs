//! Source-level markdown transforms. Every function is pure: it takes the
//! whole document and a selection (byte offsets), and returns the new
//! document plus where the cursor lands, so the same op serves a shortcut, a
//! menu item and the palette.

use std::ops::Range;
use std::sync::LazyLock;

use mdwisp_core::i18n::tf;
use pulldown_cmark::{Event, Tag};
use regex::{Captures, Regex};

#[derive(Debug, Clone, PartialEq)]
pub struct EditResult {
    pub text: String,
    pub cursor: usize,
    /// Selection to restore, when the op leaves something selected.
    pub selection: Option<Range<usize>>,
}

fn edit(text: String, cursor: usize, selection: Option<Range<usize>>) -> EditResult {
    EditResult { text, cursor, selection }
}

fn re(src: &str) -> Regex {
    Regex::new(src).unwrap()
}

/// `(bold, **)` … The wrapper list defines what the UI can apply.
pub const INLINE_WRAPPERS: &[(&str, &str)] =
    &[("bold", "**"), ("italic", "*"), ("strike", "~~"), ("code", "`"), ("mark", "=="), ("underline", "<u>")];

/// The one changed span between two texts, `(replaced range in source, insert)`,
/// so the editor keeps its unchanged ranges and scroll anchors.
pub fn minimal_change<'a>(source: &str, text: &'a str) -> (Range<usize>, &'a str) {
    let (a, b) = (source.as_bytes(), text.as_bytes());
    let mut from = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !source.is_char_boundary(from) || !text.is_char_boundary(from) {
        from -= 1;
    }
    let (mut to, mut end) = (a.len(), b.len());
    while to > from && end > from && a[to - 1] == b[end - 1] {
        to -= 1;
        end -= 1;
    }
    while !source.is_char_boundary(to) || !text.is_char_boundary(end) {
        to += 1;
        end += 1;
    }
    (from..to, &text[from..end])
}

/// Expand a range to whole lines, returning the line boundaries.
fn line_bounds(src: &str, start: usize, end: usize) -> (usize, usize) {
    let from = src[..start].rfind('\n').map_or(0, |i| i + 1);
    let at = if end > start && src.as_bytes()[end - 1] == b'\n' { end - 1 } else { end };
    (from, src[at..].find('\n').map_or(src.len(), |i| at + i))
}

/// Does the parser read `text` as one span opened by `open` and closed by `close`?
fn is_wrapped(text: &str, open: &str, close: &str) -> bool {
    if open == "==" {
        return !text[2..text.len() - 2].contains("==");
    }
    let whole = |r: &Range<usize>| *r == (0..text.len());
    if open.starts_with('`') {
        return crate::parser(text).any(|(e, r)| matches!(e, Event::Code(_)) && whole(&r));
    }
    let span = |e: &Event| match open {
        "**" => matches!(e, Event::Start(Tag::Strong)),
        "*" => matches!(e, Event::Start(Tag::Emphasis)),
        "~~" => matches!(e, Event::Start(Tag::Strikethrough)),
        _ => false,
    };
    if ["**", "*", "~~"].contains(&open) {
        return crate::parser(text).any(|(e, r)| span(&e) && whole(&r));
    }
    let mut depth = 0;
    for (e, r) in crate::parser(text) {
        if !matches!(e, Event::InlineHtml(_)) {
            continue;
        }
        let raw = &text[r.clone()];
        if raw == open {
            depth += 1;
        }
        if raw == close {
            depth -= 1;
            if depth == 0 {
                return r.end == text.len();
            }
        }
    }
    false
}

/// `^ .* $` with content: a code span whose padding spaces are significant.
fn padded(s: &str) -> bool {
    s.len() >= 2 && s.starts_with(' ') && s.ends_with(' ') && !s.trim().is_empty()
}

pub fn toggle_inline(src: &str, start: usize, end: usize, wrap: &str) -> EditResult {
    let tag = match wrap {
        "**" => Some("strong"),
        "*" => Some("em"),
        "~~" => Some("del"),
        "==" => Some("mark"),
        "<u>" => Some("u"),
        _ => None,
    };
    let close = if wrap == "<u>" { "</u>" } else { wrap };
    if start == end {
        // Nothing selected: insert the pair and put the cursor between them so
        // the next typed character lands inside the emphasis.
        return edit(format!("{}{wrap}{close}{}", &src[..start], &src[end..]), start + wrap.len(), None);
    }
    if src[start..end].trim().is_empty() {
        return edit(src.to_string(), start, Some(start..end));
    }
    if src[start..end].contains('\n') {
        let mut offset = start;
        let rows: Vec<String> = src[start..end]
            .split('\n')
            .map(|row| {
                let result =
                    if row.trim().is_empty() { src.to_string() } else { toggle_inline(src, offset, offset + row.len(), wrap).text };
                let replacement = result[offset..result.len() - (src.len() - offset - row.len())].to_string();
                offset += row.len() + 1;
                replacement
            })
            .collect();
        let text = rows.join("\n");
        let len = text.len();
        return edit(format!("{}{text}{}", &src[..start], &src[end..]), start, Some(start..start + len));
    }
    let (mut start, mut end) = (start, end);
    // Spaces outside emphasis remain prose, rather than invalidating the delimiters.
    if wrap != "`" {
        let raw = &src[start..end];
        start += raw.len() - raw.trim_start().len();
        end -= raw.len() - raw.trim_end().len();
    }
    let selected = &src[start..end];
    let mut pairs: Vec<(String, String)> = vec![(wrap.into(), close.into())];
    if let Some(tag) = tag.filter(|_| wrap != "<u>") {
        pairs.insert(0, (format!("<{tag}>"), format!("</{tag}>")));
    }
    if wrap == "`" {
        static TICKS: LazyLock<Regex> = LazyLock::new(|| re(r"(`+)( ?)$"));
        if let Some(c) = TICKS.captures(&src[..start]) {
            pairs.insert(0, (format!("{}{}", &c[1], &c[2]), format!("{}{}", &c[2], &c[1])));
        }
        let ticks = selected.len() - selected.trim_start_matches('`').len();
        if ticks > 0 {
            pairs.insert(0, (selected[..ticks].into(), selected[..ticks].into()));
        }
    }
    let b = src.as_bytes();
    for (open, shut) in &pairs {
        let (open, shut) = (open.as_str(), shut.as_str());
        let before = &b[start.saturating_sub(open.len())..start];
        let after = &b[end..(end + shut.len()).min(b.len())];
        // One star out of ** is not an italic wrapper; *** contains both formats.
        let italic_in_bold = open == "*" && (start - src[..start].trim_end_matches('*').len()) % 2 == 0;
        if before == open.as_bytes()
            && after == shut.as_bytes()
            && !italic_in_bold
            && is_wrapped(&src[start - open.len()..end + shut.len()], open, shut)
        {
            let text = format!("{}{selected}{}", &src[..start - open.len()], &src[end + shut.len()..]);
            return edit(text, start - open.len(), Some(start - open.len()..end - open.len()));
        }
        let leading = selected.len() - selected.trim_start_matches('*').len();
        if selected.starts_with(open)
            && selected.ends_with(shut)
            && selected.len() > open.len() + shut.len()
            && !(open == "*" && leading % 2 == 0)
            && is_wrapped(selected, open, shut)
        {
            let mut inner = &selected[open.len()..selected.len() - shut.len()];
            if wrap == "`" && padded(inner) {
                inner = &inner[1..inner.len() - 1];
            }
            return edit(format!("{}{inner}{}", &src[..start], &src[end..]), start, Some(start..start + inner.len()));
        }
    }
    let (mut open, mut shut) = (wrap.to_string(), close.to_string());
    if wrap == "`" {
        static RUN: LazyLock<Regex> = LazyLock::new(|| re("`+"));
        open = "`".repeat(RUN.find_iter(selected).map(|m| m.len()).max().unwrap_or(0) + 1);
        shut = open.clone();
        if selected.starts_with('`') || selected.ends_with('`') || padded(selected) {
            open.push(' ');
            shut.insert(0, ' ');
        }
    } else if let Some(tag) = tag.filter(|_| wrap != "<u>") {
        static PUNCT: LazyLock<Regex> = LazyLock::new(|| re(r"[\p{P}\p{S}]"));
        let space = |c: Option<char>| c.is_none_or(char::is_whitespace);
        let punct = |c: Option<char>| c.is_some_and(|c| PUNCT.is_match(c.encode_utf8(&mut [0; 4])));
        let before = src[..start].chars().next_back();
        let (first, last) = (selected.chars().next(), selected.chars().next_back());
        let after = src[end..].chars().next();
        let opens = !space(first) && (!punct(first) || space(before) || punct(before));
        let closes = !space(last) && (!punct(last) || space(after) || punct(after));
        // Raw inline HTML preserves exact punctuation without inserting visible spaces.
        if !opens
            || !closes
            || selected.starts_with(wrap)
            || selected.ends_with(wrap)
            || (wrap.contains('*') && (before == Some('*') || after == Some('*')))
        {
            open = format!("<{tag}>");
            shut = format!("</{tag}>");
        }
    }
    let text = format!("{}{open}{selected}{shut}{}", &src[..start], &src[end..]);
    edit(text, start + open.len(), Some(start + open.len()..end + open.len()))
}

/// Toggle a per-line prefix (`> `, `- `, `1. `, …), given per line index.
/// If every line already carries it, the prefix is removed instead.
pub fn toggle_line_prefix(src: &str, start: usize, end: usize, prefix: impl Fn(usize) -> String) -> EditResult {
    let (from, to) = line_bounds(src, start, end);
    let lines: Vec<&str> = src[from..to].split('\n').collect();
    let has = lines.iter().enumerate().all(|(i, l)| l.trim_start().starts_with(&prefix(i)));
    let next: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let p = prefix(i);
            let body = l.trim_start();
            if has { format!("{}{}", &l[..l.len() - body.len()], &body[p.len()..]) } else { p + l }
        })
        .collect();
    let joined = next.join("\n");
    let len = joined.len();
    edit(format!("{}{joined}{}", &src[..from], &src[to..]), from, Some(from..from + len))
}

/// Set (or clear with `0`) an ATX heading level on the selected lines.
pub fn set_heading(src: &str, start: usize, end: usize, level: usize) -> EditResult {
    static MARK: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*#{1,6}\s+"));
    let (from, to) = line_bounds(src, start, end);
    let next: Vec<String> = src[from..to]
        .split('\n')
        .map(|l| {
            let bare = MARK.replace(l, "");
            if level == 0 { bare.into_owned() } else { format!("{} {bare}", "#".repeat(level)) }
        })
        .collect();
    edit(format!("{}{}{}", &src[..from], next.join("\n"), &src[to..]), from, None)
}

pub fn insert_block(src: &str, at: usize, block: &str) -> EditResult {
    let (before, after) = src.split_at(at);
    // Exactly one blank line on each side — no more, no less.
    let pad_before = match () {
        _ if before.is_empty() || before.ends_with("\n\n") => "",
        _ if before.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    let pad_after = match () {
        _ if after.is_empty() || after.starts_with("\n\n") => "",
        _ if after.starts_with('\n') => "\n",
        _ => "\n\n",
    };
    edit(format!("{before}{pad_before}{block}{pad_after}{after}"), at + pad_before.len() + block.len(), None)
}

/// Smart continuation: what the next line should start with after Enter.
pub fn continuation_prefix(line: &str) -> Option<String> {
    static TASK: LazyLock<Regex> = LazyLock::new(|| re(r"^(\s*)- \[[ xX]\] "));
    static UL: LazyLock<Regex> = LazyLock::new(|| re(r"^(\s*)([-*+]) "));
    // CommonMark caps list numbers at nine digits.
    static OL: LazyLock<Regex> = LazyLock::new(|| re(r"^(\s*)([0-9]{1,9})([.)]) "));
    static QUOTE: LazyLock<Regex> = LazyLock::new(|| re(r"^(\s*)> ?"));
    let trimmed = line.trim();
    if let Some(c) = TASK.captures(line) {
        // An empty task item ends the list rather than spawning another.
        return (trimmed != "- [ ]").then(|| format!("{}- [ ] ", &c[1]));
    }
    if let Some(c) = UL.captures(line) {
        return (trimmed != &c[2]).then(|| format!("{}{} ", &c[1], &c[2]));
    }
    if let Some(c) = OL.captures(line) {
        let n: u64 = c[2].parse().unwrap();
        return (trimmed != format!("{}{}", &c[2], &c[3])).then(|| format!("{}{}{} ", &c[1], n + 1, &c[3]));
    }
    let c = QUOTE.captures(line)?;
    (trimmed != ">").then(|| format!("{}> ", &c[1]))
}

const PAIRS: &[(char, char)] = &[
    ('*', '*'),
    ('_', '_'),
    ('`', '`'),
    ('~', '~'),
    ('[', ']'),
    ('(', ')'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('“', '”'),
    ('（', '）'),
    ('【', '】'),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Typed {
    Edit(EditResult),
    /// The typed closer is already under the caret: step over it.
    SkipClose,
}

/// Decide what a typed character should do, before it reaches the document.
/// `None` lets the character through unchanged.
pub fn auto_pair(src: &str, cursor: usize, selection: Option<Range<usize>>, ch: char) -> Option<Typed> {
    let empty = selection.as_ref().is_none_or(|s| s.is_empty());
    // Literal Markdown delimiters must remain typeable, including ___ and *** rules.
    if empty && "*_~".contains(ch) {
        return None;
    }
    if selection.is_none() && (ch == '\'' || ch == '"') && src[..cursor].chars().next_back().is_some_and(char::is_alphanumeric) {
        return None;
    }
    // A fence is typed literally. Pairing its backticks leaves an extra closer
    // inside the new block and moves the caret onto a hidden delimiter.
    let line_start = src[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[cursor..].find('\n').map_or(src.len(), |i| cursor + i);
    let head = &src[line_start..cursor];
    let indent = head.len() - head.trim_start_matches(' ').len();
    if ch == '`'
        && selection.is_none()
        && indent <= 3
        && head.trim_start_matches(' ').bytes().all(|b| b == b'`')
        && src[cursor..line_end].bytes().all(|b| b == b'`')
    {
        return None;
    }
    // Closing a pair that is already there should step over it, not double it.
    if PAIRS.iter().any(|p| p.1 == ch) && src[cursor..].starts_with(ch) && empty {
        return Some(Typed::SkipClose);
    }
    let &(_, close) = PAIRS.iter().find(|p| p.0 == ch)?;
    let n = ch.len_utf8();
    Some(Typed::Edit(match selection.filter(|s| !s.is_empty()) {
        Some(s) => {
            let text = format!("{}{ch}{}{close}{}", &src[..s.start], &src[s.clone()], &src[s.end..]);
            edit(text, s.start + n, Some(s.start + n..s.end + n))
        }
        None => edit(format!("{}{ch}{close}{}", &src[..cursor], &src[cursor..]), cursor + n, None),
    }))
}

/// Fenced code block helper — adds the language slot and places the cursor.
pub fn insert_code_block(src: &str, at: usize, lang: &str) -> EditResult {
    let res = insert_block(src, at, &format!("```{lang}\n\n```"));
    edit(res.text, res.cursor - 3, None)
}

pub fn insert_table(src: &str, at: usize, rows: usize, cols: usize) -> EditResult {
    let row = |cell: &dyn Fn(usize) -> String| format!("| {} |", (0..cols).map(cell).collect::<Vec<_>>().join(" | "));
    let mut lines = vec![row(&|i| tf("列{0}", &[&(i + 1)])), row(&|_| "---".into())];
    lines.extend((1..rows).map(|_| row(&|_| "   ".into())));
    let block = lines.join("\n");
    let res = insert_block(src, at, &block);
    edit(res.text, res.cursor - block.len() + 2, None)
}

/// Strip markdown syntax down to readable plain text — for "copy as text".
pub fn to_plain_text(md: &str) -> String {
    static RULES: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        [
            (r"!\[([^\]]*)\]\([^)]*\)", "$1"),
            (r"\[([^\]]*)\]\([^)]*\)", "$1"),
            (r"(?m)^\s{0,3}#{1,6}\s+", ""),
            (r"\*\*(.*?)\*\*|__(.*?)__", "$1$2"),
            (r"\*(.*?)\*|_(.*?)_", "$1$2"),
            (r"~~(.*?)~~", "$1"),
            (r"==(.*?)==", "$1"),
            (r"`([^`]*)`", "$1"),
            (r"(?m)^\s*>\s?", ""),
        ]
        .into_iter()
        .map(|(s, to)| (re(s), to))
        .collect()
    });
    static TAGS: LazyLock<Regex> = LazyLock::new(|| re(r"</?(?:strong|em|del|mark|u)>"));
    static FRONT: LazyLock<Regex> = LazyLock::new(|| re(r"^---\n[\s\S]*?\n---\n"));
    static FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"```[\s\S]*?```"));
    static FENCE_MARK: LazyLock<Regex> = LazyLock::new(|| re(r"```[0-9A-Za-z_]*\n?"));
    let s = TAGS.replace_all(md, "");
    let s = FRONT.replace(&s, "");
    let mut s = FENCE.replace_all(&s, |c: &Captures| FENCE_MARK.replace_all(&c[0], "").into_owned()).into_owned();
    for (re, to) in RULES.iter() {
        s = re.replace_all(&s, *to).into_owned();
    }
    s.trim().to_string()
}

/// Words as a reader counts them: one per CJK character, one per Latin or number run, never markup.
pub fn count_words(md: &str) -> usize {
    static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"<[^>]+>"));
    static WORD: LazyLock<Regex> = LazyLock::new(|| {
        let cjk = r"\p{Han}\p{Hiragana}\p{Katakana}";
        let run = format!(r"[[\p{{L}}\p{{N}}_]--[{cjk}]]+");
        re(&format!(r"[{cjk}]|{run}(?:['’.\-]{run})*"))
    });
    WORD.find_iter(&TAG.replace_all(&to_plain_text(md), " ")).count()
}

/// `[selected](url)` when a single URL is pasted over a one-line selection.
pub fn paste_as_link(selected: &str, pasted: &str) -> Option<String> {
    static URL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:https?://|mailto:)\S+$"));
    let url = pasted.trim();
    if selected.trim().is_empty() || selected.contains('\n') || !URL.is_match(url) {
        return None;
    }
    let label = selected.replace('[', "\\[").replace(']', "\\]");
    Some(format!("[{label}]({})", url.replace('(', "%28").replace(')', "%29")))
}

/// Normalise whitespace and list markers without touching prose content.
pub fn tidy_markdown(src: &str) -> String {
    static TRAILING: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)[ \t]+$"));
    static BLANKS: LazyLock<Regex> = LazyLock::new(|| re(r"\n{4,}"));
    static BULLET: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^(\s*)[-*+]\s+"));
    static END: LazyLock<Regex> = LazyLock::new(|| re(r"\n+$"));
    let s = src.replace("\r\n", "\n");
    let s = TRAILING.replace_all(&s, "");
    let s = BLANKS.replace_all(&s, "\n\n\n");
    let s = BULLET.replace_all(&s, "${1}- ");
    END.replace(&s, "\n").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TS tests index in UTF-16 units; convert to the byte offsets used here.
    fn byte(s: &str, utf16: usize) -> usize {
        let mut units = 0;
        for (i, c) in s.char_indices() {
            if units >= utf16 {
                return i;
            }
            units += c.len_utf16();
        }
        s.len()
    }

    fn render(md: &str) -> String {
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new_ext(md, crate::OPTIONS));
        html
    }

    fn toggle_back(r: &EditResult, wrap: &str) -> String {
        let sel = r.selection.clone().unwrap();
        toggle_inline(&r.text, sel.start, sel.end, wrap).text
    }

    const EXAMPLES: &[(&str, usize, usize)] = &[
        ("复审：保留", 0, 3),
        ("复审： 保留", 0, 4),
        ("复审:保留", 0, 3),
        ("Review:keep", 0, 7),
        ("前（保留）后", 1, 5),
        ("前，后", 1, 2),
        ("前：保留", 1, 4),
        ("hello world!", 6, 12),
        ("  word  ", 0, 8),
        ("中文English123混排", 2, 12),
        ("emoji🙂next", 5, 7),
        ("日本語：保持", 0, 4),
        ("한국어：유지", 0, 4),
        ("café déjà vu", 0, 4),
        ("a\tword\tb", 1, 7),
        ("(word)", 1, 5),
        ("word.", 0, 4),
        ("hello *world*", 0, 13),
    ];

    #[test]
    fn formats_prose_without_changing_its_text() {
        // pulldown-cmark has no `==mark==`, so its rendering is not checked.
        for (wrap, tag) in [("**", "<strong>"), ("*", "<em>"), ("~~", "<del>"), ("==", ""), ("<u>", "<u>")] {
            for &(source, s, e) in EXAMPLES {
                let (s, e) = (byte(source, s), byte(source, e));
                let r = toggle_inline(source, s, e, wrap);
                assert!(render(&r.text).contains(tag), "{wrap} {source:?} → {}", r.text);
                let sel = r.selection.clone().unwrap();
                assert_eq!(&r.text[sel], source[s..e].trim(), "{wrap} {source:?}");
                assert_eq!(toggle_back(&r, wrap), source, "{wrap} {source:?}");
            }
        }
    }

    #[test]
    fn multiple_paragraphs_and_empty_selections() {
        for wrap in ["**", "*", "~~", "==", "<u>", "`"] {
            let source = "first\n\n中文： 保留\nlast";
            let r = toggle_inline(source, 0, source.len(), wrap);
            assert_eq!(r.text.split('\n').nth(1), Some(""));
            assert_eq!(toggle_back(&r, wrap), source, "{wrap}");
            let r = toggle_inline("ab", 1, 1, wrap);
            assert_eq!(r.text.as_bytes()[r.cursor - 1], *wrap.as_bytes().last().unwrap());
            assert!(r.text.starts_with('a') && r.text.ends_with('b'));
        }
    }

    #[test]
    fn bold_and_italic_nest() {
        assert!(render(&toggle_inline("**word**", 0, 8, "*").text).contains("<em><strong>word</strong></em>"));
        let r = toggle_inline("**word**", 2, 6, "*");
        assert!(render(&r.text).contains("<em>word</em>"));
        assert_eq!(toggle_back(&r, "*"), "**word**");
        assert_eq!(toggle_inline("***word***", 3, 7, "*").text, "**word**");
        assert_eq!(toggle_inline("***word***", 3, 7, "**").text, "*word*");
    }

    #[test]
    fn blank_and_code_selections() {
        for source in [" ", "\t  "] {
            assert_eq!(toggle_inline(source, 0, source.len(), "**").text, source);
        }
        assert_eq!(toggle_inline("`word`", 0, 6, "`").text, "word");
        assert_eq!(toggle_inline("   ", 0, 3, "`").text, "   ");
    }

    #[test]
    fn underline_uses_a_closing_tag() {
        assert_eq!(toggle_inline("", 0, 0, "<u>").text, "<u></u>");
        assert_eq!(toggle_inline("word", 0, 4, "<u>").text, "<u>word</u>");
        assert_eq!(toggle_inline("<u>word</u>", 3, 7, "<u>").text, "word");
        assert_eq!(toggle_inline("<u>word</u>", 0, 11, "<u>").text, "word");
    }

    #[test]
    fn line_formats_stop_at_the_selection() {
        assert_eq!(toggle_line_prefix("first\nsecond", 0, 6, |_| "> ".into()).text, "> first\nsecond");
        assert_eq!(toggle_line_prefix("> a\n> b", 0, 7, |_| "> ".into()).text, "a\nb");
        assert_eq!(toggle_line_prefix("a\nb", 0, 3, |i| format!("{}. ", i + 1)).text, "1. a\n2. b");
        assert_eq!(set_heading("first\nsecond", 0, 6, 2).text, "## first\nsecond");
        assert_eq!(set_heading("\nsecond", 0, 0, 2).text, "## \nsecond");
        assert_eq!(set_heading("### a", 0, 0, 0).text, "a");
    }

    #[test]
    fn punctuation_inside_containers() {
        for context in ["# 复审：保留", "- 复审：保留", "> 复审：保留", "- [ ] 复审：保留", "[复审：保留](./doc.md)", "| 内容 |\n| --- |\n| 复审：保留 |"] {
            let start = context.find("复审：").unwrap();
            let r = toggle_inline(context, start, start + "复审：".len(), "**");
            assert!(render(&r.text).contains("<strong>复审：</strong>保留"), "{context}");
            assert_eq!(toggle_back(&r, "**"), context);
        }
    }

    #[test]
    fn separate_spans_are_not_one_wrapper() {
        for (source, wrap) in [("**one** **two**", "**"), ("*one* *two*", "*"), ("<u>one</u> <u>two</u>", "<u>")] {
            let r = toggle_inline(source, 0, source.len(), wrap);
            let html = render(&r.text);
            assert!(!html.contains('*') && !html.contains("&lt;") && !html.contains("&gt;"), "{html}");
            assert_eq!(toggle_back(&r, wrap), source);
        }
        assert_eq!(to_plain_text(&toggle_inline("复审：保留", 0, 9, "**").text), "复审：保留");
    }

    #[test]
    fn inline_code_round_trips() {
        for source in ["a`b", "``", "a``b`c", " spaced ", "const x = \"a_b\"", "中文`代码`"] {
            let r = toggle_inline(source, 0, source.len(), "`");
            let html = render(&r.text);
            // pulldown-cmark leaves `"` unescaped in text, unlike markdown-it.
            let escaped = source.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            assert!(html.contains("<code>") && html.contains(&escaped), "{source:?} → {html}");
            assert_eq!(toggle_back(&r, "`"), source);
        }
    }

    #[test]
    fn typing() {
        for ch in ['_', '*', '~'] {
            for source in ["", "snake", "__", "**", "---\n", "前缀"] {
                assert_eq!(auto_pair(source, source.len(), None, ch), None);
            }
        }
        let Some(Typed::Edit(r)) = auto_pair("word", 0, Some(0..4), '_') else { panic!() };
        assert_eq!(r.text, "_word_");
        assert_eq!(auto_pair("tail", 0, None, '('), Some(Typed::Edit(edit("()tail".into(), 1, None))));
        assert_eq!(auto_pair("()", 1, None, ')'), Some(Typed::SkipClose));
        assert_eq!(auto_pair("don", 3, None, '\''), None);
        assert_eq!(auto_pair("```", 3, None, '`'), None);
        let Some(Typed::Edit(r)) = auto_pair("中", 3, None, '（') else { panic!() };
        assert_eq!((r.text.as_str(), r.cursor), ("中（）", 6));
    }

    #[test]
    fn minimal_changes() {
        assert_eq!(minimal_change("a\nb\nc", "a\nb()\nc"), (3..3, "()"));
        assert_eq!(minimal_change("abc", "ac"), (1..2, ""));
        assert_eq!(minimal_change("", "_"), (0..0, "_"));
        assert_eq!(minimal_change("same", "same"), (4..4, ""));
        // `中` and `丰` share their first two UTF-8 bytes.
        assert_eq!(minimal_change("a中b", "a丰b"), (1..4, "丰"));
    }

    #[test]
    fn words() {
        for (source, n) in [
            ("", 0),
            ("写作，从这里开始。", 7),
            ("Hello world, it’s 2026.", 4),
            ("# 标题 Title\n\n**加粗** and [链接](https://example.com/a-b)", 8),
            ("| 名称 | 数量 |\n| --- | --- |\n| 苹果<br>梨 | 2 |", 8),
            ("混排English单词", 5),
        ] {
            assert_eq!(count_words(source), n, "{source}");
        }
        assert_eq!(to_plain_text("~~gone~~ ==hi=="), "gone hi");
    }

    #[test]
    fn paste_links() {
        assert_eq!(paste_as_link("MDWisp", " https://github.com/x \n").as_deref(), Some("[MDWisp](https://github.com/x)"));
        assert_eq!(paste_as_link("a [b]", "https://e.com/(1)").as_deref(), Some("[a \\[b\\]](https://e.com/%281%29)"));
        assert_eq!(paste_as_link("", "https://e.com"), None);
        assert_eq!(paste_as_link("two\nlines", "https://e.com"), None);
        assert_eq!(paste_as_link("text", "not a url"), None);
        assert_eq!(paste_as_link("text", "https://a.com https://b.com"), None);
    }

    #[test]
    fn blocks_and_continuation() {
        assert_eq!(insert_block("a", 1, "B").text, "a\n\nB");
        assert_eq!(insert_block("a\n\nb", 1, "B").text, "a\n\nB\n\nb");
        assert_eq!(insert_code_block("", 0, "rs"), edit("```rs\n\n```".into(), 7, None));
        assert_eq!(insert_table("", 0, 2, 2).text, "| 列1 | 列2 |\n| --- | --- |\n|     |     |");
        assert_eq!(continuation_prefix("  - [x] done").as_deref(), Some("  - [ ] "));
        assert_eq!(continuation_prefix("- [ ] "), None);
        assert_eq!(continuation_prefix("* item").as_deref(), Some("* "));
        assert_eq!(continuation_prefix("9) item").as_deref(), Some("10) "));
        assert_eq!(continuation_prefix("1."), None);
        assert_eq!(continuation_prefix("> quote").as_deref(), Some("> "));
        assert_eq!(continuation_prefix("plain"), None);
        assert_eq!(tidy_markdown("* a  \r\n+ b\n\n\n\n\nc\n\n"), "- a\n- b\n\n\nc\n");
    }
}
