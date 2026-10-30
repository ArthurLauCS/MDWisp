//! Live preview: what to hide, style and replace, computed from the markdown
//! source. The source is never rewritten; the view only decorates it, the way
//! `live-preview.ts` decorated CodeMirror.

use std::ops::Range;

use mdwisp_table::model::{TableContext, find_table_at};
use pulldown_cmark::{CodeBlockKind, Event, LinkType, Tag, TagEnd};

/// Per-line styling, the CSS line classes of the TS editor.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub struct LineDeco {
    /// 1–6 for headings, 0 otherwise.
    pub heading: u8,
    pub para_start: bool,
    pub para_end: bool,
    /// Nesting depth for list lines, 0 outside lists.
    pub list: u8,
    pub list_start: bool,
    pub quote: bool,
    pub code: Code,
    pub frontmatter: bool,
    pub blank: bool,
    pub image: bool,
    pub rule: bool,
    /// Table cell text: 1 header, 2 body.
    pub table: u8,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub enum Code {
    #[default]
    None,
    Body,
    /// The opening fence line; `from` identifies the block.
    Header,
    /// The closing fence line.
    End,
}

/// Inline styles; several may overlap.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Style {
    Strong,
    Em,
    Strike,
    Code,
    Mark,
    Underline,
    Sup,
    Sub,
    Link,
    Math,
    /// `1.` of an ordered list: kept as typed, in the accent colour.
    ListMarker,
    /// A code token inside a fenced block.
    Hl(mdwisp_text::highlight::Token),
}

#[derive(Clone, PartialEq, Debug, Hash)]
pub enum Widget {
    Bullet,
    Checkbox { checked: bool },
    Image { src: String, alt: String },
    Rule,
    /// Replaces the opening fence: the language picker. `info` is the fence's info range;
    /// `detected` is what an unlabeled block was recognised as.
    CodeLang { block: Range<usize>, info: Range<usize>, lang: String, detected: Option<&'static str> },
}

#[derive(Clone, Debug)]
pub struct Link {
    pub range: Range<usize>,
    pub href: String,
}

#[derive(Default)]
pub struct Deco {
    pub lines: Vec<LineDeco>,
    /// Byte offset where each line starts.
    pub starts: Vec<usize>,
    pub marks: Vec<(Range<usize>, Style)>,
    /// Sorted, merged source ranges that are not displayed.
    pub hidden: Vec<Range<usize>>,
    pub widgets: Vec<(Range<usize>, Widget)>,
    pub tables: Vec<TableContext>,
    pub links: Vec<Link>,
    /// Fenced code blocks: (whole block, code text, language as written).
    pub code_blocks: Vec<(Range<usize>, Range<usize>, String)>,
    /// Links and images: the only sources whose display depends on the selection.
    pub reveal: Vec<Range<usize>>,
    /// Longest mark and link, so per-line lookups can binary-search.
    pub mark_span: usize,
    pub link_span: usize,
}

/// What the selection touches; links, images and formulas reveal their source there.
pub struct Focus {
    pub from: usize,
    pub to: usize,
    pub head: usize,
    pub searching: bool,
}

impl Deco {
    pub fn line_of(&self, pos: usize) -> usize {
        self.starts.partition_point(|s| *s <= pos).saturating_sub(1)
    }

    /// Whether a selection touches anything that shows its source when selected;
    /// if neither the old nor the new selection does, the decorations stay the same.
    pub fn reveals(&self, from: usize, to: usize, head: usize) -> bool {
        self.reveal.iter().any(|r| (r.start < to && r.end > from) || (head > r.start && head < r.end))
    }
}

pub fn decorate(src: &str, focus: &Focus) -> Deco {
    let mut d = Deco::default();
    d.starts.push(0);
    d.starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
    d.lines = vec![LineDeco::default(); d.starts.len()];
    let body = mdwisp_text::frontmatter_end(src);
    for l in 0..d.line_of(body.saturating_sub(1)) + usize::from(body > 0) {
        d.lines[l].frontmatter = true;
    }
    let mut hidden: Vec<Range<usize>> = vec![];

    let events: Vec<(Event, Range<usize>)> =
        mdwisp_text::parser(&src[body..]).map(|(e, r)| (e, r.start + body..r.end + body)).collect();
    let mut code_ranges: Vec<Range<usize>> = vec![];
    for (e, r) in &events {
        if matches!(e, Event::Code(_) | Event::Html(_) | Event::InlineHtml(_)) {
            code_ranges.push(r.clone());
        }
    }
    // Events come in document order, so the ranges are sorted and disjoint.
    let in_code = |at: usize| {
        let i = code_ranges.partition_point(|c| c.start <= at);
        i > 0 && at < code_ranges[i - 1].end
    };

    let mut list_depth = 0u8;
    let mut list_lines: Vec<(usize, u8, bool)> = vec![];
    let mut skip_until = 0usize;
    // Inline containers whose text gets the `==mark==` and bare-URL scan.
    let mut inline_blocks: Vec<Range<usize>> = vec![];

    for (i, (e, r)) in events.iter().enumerate() {
        let (from, to) = (r.start, r.end);
        if from < skip_until && !matches!(e, Event::End(_)) {
            continue;
        }
        match e {
            Event::Start(Tag::Heading { level, .. }) => {
                let l = d.line_of(from);
                d.lines[l].heading = *level as u8;
                hide_heading_marks(src, from, to, &mut hidden);
                inline_blocks.push(r.clone());
            }
            Event::Start(Tag::Paragraph) => {
                let (a, b) = (d.line_of(from), d.line_of(to.saturating_sub(1).max(from)));
                d.lines[a].para_start = true;
                d.lines[b].para_end = true;
                inline_blocks.push(r.clone());
            }
            Event::Start(Tag::Item) => {
                list_depth += 1;
                let first = d.line_of(from);
                let last = d.line_of(to.saturating_sub(1).max(from));
                for l in first..=last {
                    list_lines.push((l, list_depth, l == first));
                }
                // The marker: `-`, `*`, `+` become a bullet; numbers stay as typed.
                let line = &src[from..d.line_end(first, src)];
                let ind = line.len() - line.trim_start().len();
                let m0 = from + ind;
                let rest = &src[m0..];
                let mlen = if rest.starts_with(['-', '*', '+']) {
                    1
                } else {
                    rest.bytes().take_while(u8::is_ascii_digit).count() + 1
                };
                let mut m1 = (m0 + mlen).min(src.len());
                if src[m1..].starts_with(' ') {
                    m1 += 1;
                }
                if rest.starts_with(['-', '*', '+']) {
                    d.widgets.push((m0..m1, Widget::Bullet));
                } else {
                    d.marks.push((m0..m1, Style::ListMarker));
                }
                // A tight item has no paragraph: its text is the events up to its first nested block.
                if !matches!(events.get(i + 1), Some((Event::Start(Tag::Paragraph), _))) {
                    let n = events[i + 1..].iter().position(|(e, _)| {
                        matches!(e, Event::End(TagEnd::Item) | Event::Start(Tag::List(_) | Tag::CodeBlock(_) | Tag::BlockQuote(_) | Tag::Table(_) | Tag::HtmlBlock | Tag::Heading { .. }))
                    });
                    if let Some(n @ 1..) = n {
                        inline_blocks.push(events[i + 1].1.start..events[i + n].1.end);
                    }
                }
            }
            Event::End(TagEnd::Item) => list_depth -= 1,
            Event::Start(Tag::Strong) => delim(r, 2, Style::Strong, &mut d, &mut hidden),
            Event::Start(Tag::Emphasis) => delim(r, 1, Style::Em, &mut d, &mut hidden),
            Event::Start(Tag::Strikethrough) => {
                let n = src[from..].bytes().take_while(|b| *b == b'~').count().clamp(1, 2);
                delim(r, n, Style::Strike, &mut d, &mut hidden)
            }
            Event::Start(Tag::Superscript) => delim(r, 1, Style::Sup, &mut d, &mut hidden),
            Event::Start(Tag::Subscript) => delim(r, 1, Style::Sub, &mut d, &mut hidden),
            Event::Code(_) => {
                d.marks.push((r.clone(), Style::Code));
                let n = src[from..to].bytes().take_while(|b| *b == b'`').count();
                hidden.push(from..from + n);
                hidden.push(to - n..to);
                let inner = &src[from + n..to - n];
                if inner.starts_with(' ') && inner.ends_with(' ') && !inner.trim().is_empty() {
                    hidden.push(from + n..from + n + 1);
                    hidden.push(to - n - 1..to - n);
                }
            }
            Event::TaskListMarker(checked) => d.widgets.push((r.clone(), Widget::Checkbox { checked: *checked })),
            Event::InlineHtml(tag) => {
                let tag = tag.as_ref();
                let name = tag.strip_prefix('<').and_then(|t| t.strip_suffix('>')).filter(|t| ["u", "strong", "em", "del", "mark"].contains(t));
                if let Some(name) = name {
                    let mut depth = 1;
                    let (open_tag, close_tag) = (format!("<{name}>"), format!("</{name}>"));
                    let close = events[i + 1..].iter().find(|(e, _)| {
                        if let Event::InlineHtml(t) = e {
                            if t.as_ref() == open_tag {
                                depth += 1;
                            } else if t.as_ref() == close_tag {
                                depth -= 1;
                                return depth == 0;
                            }
                        }
                        matches!(e, Event::End(TagEnd::Paragraph | TagEnd::Heading(_)))
                    });
                    if let Some((Event::InlineHtml(_), c)) = close {
                        hidden.push(r.clone());
                        let style = match name {
                            "u" => Style::Underline,
                            "strong" => Style::Strong,
                            "em" => Style::Em,
                            "del" => Style::Strike,
                            _ => Style::Mark,
                        };
                        d.marks.push((to..c.start, style));
                        hidden.push(c.clone());
                    }
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                for l in d.line_of(from)..=d.line_of(to.saturating_sub(1).max(from)) {
                    let s = d.starts[l];
                    let line = &src[s..d.line_end(l, src)];
                    let mut p = 0;
                    let mut any = false;
                    loop {
                        let rest = &line[p..];
                        let sp = rest.len() - rest.trim_start_matches(' ').len();
                        if sp > 3 || !rest[sp..].starts_with('>') {
                            break;
                        }
                        p += sp + 1;
                        if line[p..].starts_with(' ') {
                            p += 1;
                        }
                        any = true;
                    }
                    if any {
                        d.lines[l].quote = true;
                        hidden.push(s..s + p);
                    }
                }
            }
            Event::Start(Tag::Link { link_type, dest_url, .. }) => {
                d.links.push(Link { range: r.clone(), href: dest_url.to_string() });
                d.reveal.push(r.clone());
                d.marks.push((r.clone(), Style::Link));
                match link_type {
                    LinkType::Autolink | LinkType::Email => {
                        hidden.push(from..from + 1);
                        hidden.push(to - 1..to);
                    }
                    _ if src[from..].starts_with('[') => {
                        let label_end = label_end(&events[i + 1..], from + 1);
                        hidden.push(from..from + 1);
                        if focus.searching || focus.head <= label_end || focus.head >= to {
                            hidden.push(label_end..to);
                        }
                    }
                    _ => {}
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let row = d.line_of(from);
                let (ls, le) = (d.starts[row], d.line_end(row, src));
                // A linked image `[![a](x)](y)` is standalone when the link fills the line.
                let container = d.links.last().filter(|l| l.range.start <= from && l.range.end >= to).map_or(from..to, |l| l.range.clone());
                // Alt text or a link label may run over several lines; such an image is inline.
                let standalone = container.start >= ls && container.end <= le && src[ls..container.start].trim().is_empty() && src[container.end..le].trim().is_empty();
                if standalone {
                    d.lines[row].image = true;
                }
                d.reveal.push(r.clone());
                let selected = focus.from < to && focus.to > from;
                let inside = focus.head > from && focus.head < to;
                if focus.searching || standalone || (!selected && !inside) {
                    let alt = alt_text(&events[i + 1..]);
                    d.widgets.push((r.clone(), Widget::Image { src: dest_url.to_string(), alt }));
                }
                skip_until = to;
            }
            Event::Rule => {
                let l = d.line_of(from);
                d.lines[l].rule = true;
                d.widgets.push((from..trim_nl(src, from, to), Widget::Rule));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let first = d.line_of(from);
                let first_end = d.line_end(first, src);
                let end = trim_nl(src, from, to);
                let last = d.line_of(end.saturating_sub(1).max(from));
                let fence = fence_of(&src[from..first_end]);
                let closed = last > first && fence.is_some_and(|f| is_close(&src[d.starts[last]..d.line_end(last, src)], f));
                // Keep an unfinished opening fence visible until Enter creates a body.
                if first_end >= end {
                    skip_until = to;
                    continue;
                }
                for l in first..=last {
                    d.lines[l].code = if l == first { Code::Header } else if closed && l == last { Code::End } else { Code::Body };
                }
                let info_start = from + src[from..first_end].find(info.as_ref()).unwrap_or(first_end - from);
                let info_range = if info.is_empty() { first_end..first_end } else { info_start..info_start + info.len() };
                let text_start = (first_end + 1).min(end);
                let text_end = if closed { d.starts[last].saturating_sub(1).max(text_start) } else { end };
                let code = &src[text_start..text_end];
                let lang = info.split_whitespace().next().unwrap_or("");
                let (detected, spans) = highlighted(lang, code);
                for (r, tok) in spans {
                    d.marks.push((text_start + r.start..text_start + r.end, Style::Hl(tok)));
                }
                d.widgets.push((from..first_end, Widget::CodeLang { block: from..end, info: info_range, lang: info.to_string(), detected }));
                d.code_blocks.push((from..end, text_start..text_end, lang.to_string()));
                if closed {
                    hidden.push(d.starts[last]..d.line_end(last, src));
                }
                skip_until = to;
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => {
                for l in d.line_of(from)..=d.line_of(trim_nl(src, from, to).saturating_sub(1).max(from)) {
                    d.lines[l].code = Code::Body;
                }
                skip_until = to;
            }
            Event::Start(Tag::Table(_)) => {
                if let Some(ctx) = find_table_at(src, from) {
                    skip_until = ctx.end.max(to);
                    d.tables.push(ctx);
                }
            }
            _ => {}
        }
    }

    // Bare URLs are checked against the parsed links only; those are in document order.
    let parsed_links = std::mem::take(&mut d.links);
    for block in &inline_blocks {
        let text = &src[block.clone()];
        // `==mark==`: the opener hugs its content, the closer too.
        let mut open: Option<usize> = None;
        let mut k = 0;
        while let Some(p) = text[k..].find("==") {
            let at = block.start + k + p;
            k += p + 2;
            if in_code(at) {
                continue;
            }
            let after = src[at + 2..].chars().next().is_some_and(|c| !c.is_whitespace());
            let before = src[..at].chars().next_back().is_some_and(|c| !c.is_whitespace());
            match open {
                Some(o) if before => {
                    hidden.push(o..o + 2);
                    hidden.push(at..at + 2);
                    d.marks.push((o..at + 2, Style::Mark));
                    open = None;
                }
                None if after => open = Some(at),
                _ => {}
            }
        }
        let before = d.marks.len();
        // A `$$ … $$` paragraph is TeX source like `$inline$`, one formula.
        if mdwisp_text::math::block_math(text).is_some() {
            d.marks.push((block.clone(), Style::Math));
        }
        for m in mdwisp_text::math::find_inline_math(text, |at| in_code(block.start + at)) {
            d.marks.push((block.start + m.start..block.start + m.end, Style::Math));
        }
        bare_urls(src, block.clone(), &parsed_links, &in_code, &mut d);
        // Intraword `^sup^` / `~sub~`, which the parser leaves as text; never inside
        // a formula, a bare URL, or a link or image source (an image may sit in a link).
        let taken: Vec<Range<usize>> = d.marks[before..].iter().map(|(r, _)| r.clone()).collect();
        let mut k = 0;
        while let Some(p) = text[k..].find(['^', '~']) {
            let at = block.start + k + p;
            k += p + 1;
            let i = d.reveal.partition_point(|r| r.start <= at);
            // The second `~` of a `~~strike~~` delimiter never opens a span.
            let doubled = at > 0 && src.as_bytes()[at - 1] == src.as_bytes()[at];
            if doubled || in_code(at) || taken.iter().any(|r| r.contains(&at)) || d.reveal[..i].iter().rev().take(2).any(|r| r.contains(&at)) {
                continue;
            }
            if let Some((tag, _, len)) = mdwisp_text::inline_span(&src[at..block.end]) {
                hidden.push(at..at + 1);
                hidden.push(at + len - 1..at + len);
                d.marks.push((at + 1..at + len - 1, if tag == "sup" { Style::Sup } else { Style::Sub }));
                k = at + len - block.start;
            }
        }
    }
    d.links.extend(parsed_links);

    for (l, depth, first) in list_lines {
        if d.lines[l].code != Code::None {
            continue;
        }
        d.lines[l].list = depth;
        d.lines[l].list_start |= first;
        let s = d.starts[l];
        let line = &src[s..d.line_end(l, src)];
        let ind = line.len() - line.trim_start().len();
        if ind > 0 {
            hidden.push(s..s + ind);
        }
    }
    // An empty document's one line holds the placeholder and the caret: it keeps its full height.
    for l in 0..d.lines.len() {
        let s = d.starts[l];
        if s >= body && !src.is_empty() && src[s..d.line_end(l, src)].trim().is_empty() && d.lines[l].code == Code::None {
            d.lines[l].blank = true;
        }
    }
    hidden.sort_by_key(|r| r.start);
    for r in hidden {
        match d.hidden.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ if r.end > r.start => d.hidden.push(r),
            _ => {}
        }
    }
    d.marks.sort_by_key(|(r, _)| r.start);
    d.widgets.sort_by_key(|(r, _)| r.start);
    d.links.sort_by_key(|l| l.range.start);
    d.mark_span = d.marks.iter().map(|(r, _)| r.len()).max().unwrap_or(0);
    d.link_span = d.links.iter().map(|l| l.range.len()).max().unwrap_or(0);
    d
}

impl Deco {
    /// Hidden ranges touching `line` (merged and sorted, so a binary search finds them).
    pub fn hidden_in(&self, line: &Range<usize>) -> &[Range<usize>] {
        let a = self.hidden.partition_point(|h| h.end <= line.start);
        let b = a + self.hidden[a..].partition_point(|h| h.start < line.end + 1);
        &self.hidden[a..b]
    }

    pub fn widgets_in(&self, line: &Range<usize>) -> &[(Range<usize>, Widget)] {
        let a = self.widgets.partition_point(|(r, _)| r.start < line.start);
        let b = a + self.widgets[a..].partition_point(|(r, _)| r.start < line.end.max(line.start + 1));
        &self.widgets[a..b]
    }

    pub fn marks_in(&self, line: &Range<usize>) -> impl Iterator<Item = &(Range<usize>, Style)> {
        let a = self.marks.partition_point(|(r, _)| r.start < line.start.saturating_sub(self.mark_span));
        let b = self.marks.partition_point(|(r, _)| r.start < line.end);
        self.marks[a..b.max(a)].iter().filter(move |(r, _)| r.end > line.start)
    }

    pub fn line_end(&self, l: usize, src: &str) -> usize {
        self.starts.get(l + 1).map_or(src.len(), |s| s - 1)
    }
}

thread_local! {
    /// Detection and highlighting by block content: a keystroke changes one
    /// block at most, and redoing every block made typing scale with all the code.
    static HIGHLIGHTS: std::cell::RefCell<std::collections::HashMap<u64, (Option<&'static str>, mdwisp_text::highlight::Spans)>> = Default::default();
}

fn highlighted(lang: &str, code: &str) -> (Option<&'static str>, mdwisp_text::highlight::Spans) {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    (lang, code).hash(&mut h);
    let key = h.finish();
    HIGHLIGHTS.with_borrow_mut(|cache| {
        if cache.len() > 4000 {
            cache.clear();
        }
        cache
            .entry(key)
            .or_insert_with(|| {
                let detected = if lang.is_empty() { mdwisp_text::detect::detect_language(code).lang } else { None };
                (detected, mdwisp_text::highlight::highlight(detected.unwrap_or(lang), code))
            })
            .clone()
    })
}

/// Emphasis-style delimiters: `n` characters at each end of the range.
fn delim(r: &Range<usize>, n: usize, style: Style, d: &mut Deco, hidden: &mut Vec<Range<usize>>) {
    let n = n.min((r.end - r.start) / 2);
    d.marks.push((r.clone(), style));
    hidden.push(r.start..r.start + n);
    hidden.push(r.end - n..r.end);
}

fn hide_heading_marks(src: &str, from: usize, to: usize, hidden: &mut Vec<Range<usize>>) {
    let text = &src[from..to];
    let ind = text.len() - text.trim_start_matches(' ').len();
    if text[ind..].starts_with('#') {
        let n = text[ind..].bytes().take_while(|b| *b == b'#').count();
        let mut end = from + ind + n;
        if src[end..].starts_with(' ') {
            end += 1;
        }
        hidden.push(from..end);
        // A closing `###` sequence after a space is part of the mark too.
        let line = text.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_end_matches(' ').trim_end_matches('#');
        if trimmed.len() < line.trim_end_matches(' ').len() && trimmed.ends_with(' ') && trimmed.len() > ind + n {
            hidden.push(from + trimmed.trim_end().len()..from + line.len());
        }
    } else if let Some(nl) = text.trim_end_matches(['\n', '\r']).rfind('\n') {
        // Setext: the underline is the mark.
        hidden.push(from + nl + 1..from + text.trim_end_matches(['\n', '\r']).len());
    }
}

/// Where a link's label ends (the `]`), from the events inside the link.
fn label_end(inner: &[(Event, Range<usize>)], start: usize) -> usize {
    let mut end = start;
    let mut depth = 0;
    for (e, r) in inner {
        match e {
            Event::Start(_) => depth += 1,
            Event::End(TagEnd::Link) if depth == 0 => break,
            Event::End(_) => depth -= 1,
            _ => {}
        }
        end = end.max(r.end);
    }
    end
}

fn alt_text(inner: &[(Event, Range<usize>)]) -> String {
    let mut alt = String::new();
    for (e, _) in inner {
        match e {
            Event::End(TagEnd::Image) => break,
            Event::Text(t) | Event::Code(t) => alt.push_str(t),
            _ => {}
        }
    }
    alt
}

fn trim_nl(src: &str, from: usize, to: usize) -> usize {
    let mut e = to;
    while e > from && matches!(src.as_bytes()[e - 1], b'\n' | b'\r') {
        e -= 1;
    }
    e
}

fn fence_of(line: &str) -> Option<&str> {
    let t = line.trim_start_matches(' ');
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.len() - t.trim_start_matches(c).len();
    (n >= 3).then(|| &t[..n])
}

fn is_close(line: &str, open: &str) -> bool {
    fence_of(line).is_some_and(|f| f.as_bytes()[0] == open.as_bytes()[0] && f.len() >= open.len()) && line.trim().trim_matches(open.as_bytes()[0] as char).is_empty()
}

/// Bare URLs and e-mail addresses by the shared rule, minus those inside code or a parsed link.
fn bare_urls(src: &str, block: Range<usize>, links: &[Link], in_code: &impl Fn(usize) -> bool, d: &mut Deco) {
    for (r, href) in mdwisp_text::links::bare_links(&src[block.clone()]) {
        let (start, end) = (block.start + r.start, block.start + r.end);
        let i = links.partition_point(|l| l.range.start <= start);
        if in_code(start) || (i > 0 && start < links[i - 1].range.end) {
            continue;
        }
        d.links.push(Link { range: start..end, href });
        d.marks.push((start..end, Style::Link));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus_none() -> Focus {
        Focus { from: usize::MAX, to: usize::MAX, head: usize::MAX, searching: false }
    }

    fn visible(src: &str, d: &Deco) -> String {
        let mut out = String::new();
        let mut p = 0;
        for h in &d.hidden {
            out.push_str(&src[p..h.start]);
            p = h.end;
        }
        out.push_str(&src[p..]);
        out
    }

    #[test]
    fn hides_heading_and_emphasis_marks() {
        let src = "# Title\n\nSome **bold** and *em* and `code` ==hi==.";
        let d = decorate(src, &focus_none());
        assert_eq!(d.lines[0].heading, 1);
        assert!(d.lines[1].blank);
        assert!(!decorate("", &focus_none()).lines[0].blank);
        assert_eq!(visible(src, &d), "Title\n\nSome bold and em and code hi.");
        assert!(d.marks.iter().any(|(_, s)| *s == Style::Mark));
    }

    #[test]
    fn lists_quotes_and_tasks() {
        let src = "- a\n  - b\n- [x] done\n\n> quote\n> more";
        let d = decorate(src, &focus_none());
        assert_eq!((d.lines[0].list, d.lines[1].list), (1, 2));
        assert!(d.widgets.iter().any(|(_, w)| *w == Widget::Checkbox { checked: true }));
        assert_eq!(d.widgets.iter().filter(|(_, w)| *w == Widget::Bullet).count(), 3);
        assert!(d.lines[4].quote && d.lines[5].quote);
        assert!(visible(src, &d).ends_with("quote\nmore"));
    }

    #[test]
    fn fenced_code_lines_and_language_widget() {
        let src = "```rust\nfn a() {}\n```\nafter";
        let d = decorate(src, &focus_none());
        assert_eq!([d.lines[0].code, d.lines[1].code, d.lines[2].code], [Code::Header, Code::Body, Code::End]);
        assert!(matches!(&d.widgets[0].1, Widget::CodeLang { lang, .. } if lang == "rust"));
        assert_eq!(d.code_blocks[0].1, 8..17);
    }

    #[test]
    fn links_reveal_their_target_only_while_editing() {
        let src = "see [label](http://x.y) end";
        let d = decorate(src, &focus_none());
        assert_eq!(visible(src, &d), "see label end");
        let d = decorate(src, &Focus { from: 8, to: 8, head: 8, searching: false });
        assert_eq!(visible(src, &d), "see label end");
        let d = decorate(src, &Focus { from: 14, to: 14, head: 14, searching: false });
        assert_eq!(visible(src, &d), "see label](http://x.y) end");
    }

    #[test]
    fn tables_images_rules_and_bare_urls() {
        let src = "| a | b |\n| - | - |\n| 1 | 2 |\n\n![cat](./c.png)\n\n---\n\nvisit https://example.com.";
        let d = decorate(src, &focus_none());
        assert_eq!(d.tables.len(), 1);
        assert!(d.lines[4].image);
        assert!(d.lines[6].rule);
        assert_eq!(d.links.last().map(|l| l.href.as_str()), Some("https://example.com"));
    }

    #[test]
    fn links_urls_against_cjk_and_marks_intraword_sub_sup_and_block_math() {
        let src = "网站：https://a.cn/~x~y。邮件me@a.cn\n\nH~2~O x^2^ ~~del~~x [a~b~](./c~d~.md) `e~f~`\n\n$$\nx^2^\n$$\n";
        let d = decorate(src, &focus_none());
        let hrefs: Vec<_> = d.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, ["https://a.cn/~x~y", "mailto:me@a.cn", "./c~d~.md"]);
        let marked = |s: Style| d.marks.iter().filter(|(_, m)| *m == s).map(|(r, _)| &src[r.clone()]).collect::<Vec<_>>();
        assert_eq!(marked(Style::Sub), ["2"]);
        assert_eq!(marked(Style::Sup), ["2"]);
        assert_eq!(marked(Style::Math), ["$$\nx^2^\n$$\n"]);
        assert_eq!(visible(src, &d).lines().nth(2), Some("H2O x2 delx a~b~ e~f~"));
    }

    #[test]
    fn tight_list_items_get_the_inline_scan_once() {
        let src = "- 主页https://a.cn ==m==\n  - www.b.cn\n- [ ] me@c.cn\n\n1. loose https://d.cn\n\n2. x\n";
        let d = decorate(src, &focus_none());
        let hrefs: Vec<_> = d.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, ["https://a.cn", "http://www.b.cn", "mailto:me@c.cn", "https://d.cn"]);
        assert_eq!(d.marks.iter().filter(|(_, m)| *m == Style::Mark).count(), 1);
    }

    #[test]
    fn frontmatter_is_not_parsed_as_markdown() {
        let src = "---\ntitle: a\n---\n# x";
        let d = decorate(src, &focus_none());
        assert!(d.lines[0].frontmatter && d.lines[2].frontmatter && !d.lines[3].frontmatter);
        assert_eq!(d.lines[1].heading, 0);
        assert_eq!(d.lines[3].heading, 1);
    }
}
