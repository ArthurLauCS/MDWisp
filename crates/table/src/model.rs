//! Locates a table in a source string, parses it into a rectangular cell
//! matrix and serializes it back.
//!
//! Offsets are UTF-8 byte offsets into the source. Line endings are
//! normalized to `\n` on parse, so a `\r\n` source serializes back as `\n` —
//! byte-identical round-tripping is only guaranteed for `\n` sources.

use std::sync::LazyLock;

use regex::Regex;

pub type Cells = Vec<Vec<String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableContext {
    /// Byte offset of the table block's first byte in the source.
    pub start: usize,
    /// Byte offset just past the block's last byte.
    pub end: usize,
    /// Source slice `[start, end)`.
    pub raw: String,
    pub cols: usize,
    /// Rows below the delimiter row.
    pub body_rows: usize,
    /// As written in the delimiter row, padded or cut to `cols`.
    pub aligns: Vec<Align>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    pub from: usize,
    pub to: usize,
    /// The row is shorter than the table, so the cell has no text yet.
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEdit {
    pub text: String,
    pub cursor: usize,
}

struct Line<'a> {
    text: &'a str,
    start: usize,
}

/// Cell bounds relative to the line they came from, trimmed of blanks.
type Span = (usize, usize);

struct Block {
    first: usize,
    len: usize,
    aligns: Vec<Align>,
    start: usize,
    end: usize,
}

static DELIM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?$").unwrap());
pub(crate) static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:```|~~~)").unwrap());
static BLOCK_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s{0,3}(?:#{1,6}\s|>|[-*+]\s|[0-9]+[.)]\s)").unwrap());
/// The render-style marker a table may carry on the line after it.
pub(crate) static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*<!--\s*mdview:table\s+style=([^>]*?)\s*-->\s*$").unwrap());

fn split_lines(src: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, _) in src.match_indices('\n') {
        let text = &src[start..i];
        lines.push(Line { text: text.strip_suffix('\r').unwrap_or(text), start });
        start = i + 1;
    }
    if start < src.len() {
        lines.push(Line { text: &src[start..], start });
    }
    lines
}

/// Pipes from byte `from` on that split cells: not escaped as `\|` and not
/// inside a code span. An unclosed backtick run opens a span that never ends.
fn pipes(text: &str, from: usize) -> Vec<usize> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    // Length of the backtick run that opened the current code span; 0 outside.
    let mut tick = 0;
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'\\' if b.get(i + 1) == Some(&b'|') => i += 2,
            b'`' => {
                let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                if tick == 0 {
                    tick = n;
                } else if n == tick {
                    tick = 0;
                }
                i += n;
            }
            c => {
                if c == b'|' && tick == 0 {
                    out.push(i);
                }
                i += 1;
            }
        }
    }
    out
}

fn has_pipe(text: &str) -> bool {
    !pipes(text, 0).is_empty()
}

fn split_row(text: &str) -> Vec<Span> {
    let b = text.as_bytes();
    let lead = usize::from(text.starts_with('|'));
    let mut cuts = pipes(text, lead);
    // A trailing pipe closes the row rather than opening an empty last cell.
    let last = match cuts.last() {
        Some(&p) if text.ends_with('|') => {
            cuts.pop();
            p
        }
        _ => text.len(),
    };
    let trim = |mut s: usize, mut e: usize| {
        while s < e && matches!(b[s], b' ' | b'\t') {
            s += 1;
        }
        while e > s && matches!(b[e - 1], b' ' | b'\t') {
            e -= 1;
        }
        (s, e)
    };
    let mut spans = Vec::with_capacity(cuts.len() + 1);
    let mut pos = lead;
    for c in cuts {
        spans.push(trim(pos, c));
        pos = c + 1;
    }
    spans.push(trim(pos, last));
    spans
}

fn delimiter_aligns(text: &str) -> Option<Vec<Align>> {
    if !DELIM.is_match(text) || !text.contains('-') {
        return None;
    }
    let aligns = split_row(text).into_iter().map(|(s, e)| {
        let t = &text[s..e];
        match (t.starts_with(':'), t.ends_with(':')) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            (true, false) => Align::Left,
            _ => Align::None,
        }
    });
    Some(aligns.collect())
}

/// Validate the header/delimiter grammar at `i`, then extend over body rows.
fn try_collect(lines: &[Line], i: usize) -> Option<Block> {
    let delim = lines.get(i + 1)?;
    if !has_pipe(lines[i].text) {
        return None;
    }
    let aligns = delimiter_aligns(delim.text)?;
    let mut j = i + 2;
    while let Some(line) = lines.get(j) {
        let t = line.text;
        if t.trim().is_empty() || FENCE.is_match(t) || BLOCK_START.is_match(t) {
            break;
        }
        // The style marker sits under the table but is not part of it; a
        // one-column table would otherwise swallow it as a body row.
        if MARKER.is_match(t) {
            break;
        }
        // A lone paragraph line under a one-column table is a body row.
        if aligns.len() != 1 && !has_pipe(t) {
            break;
        }
        j += 1;
    }
    let last = &lines[j - 1];
    Some(Block { first: i, len: j - i, aligns, start: lines[i].start, end: last.start + last.text.len() })
}

/// Index of the line containing `offset`; an offset at a line's end belongs
/// to the next line.
fn line_at(lines: &[Line], offset: usize) -> usize {
    let last = lines.len() - 1;
    (0..last).find(|&i| offset < lines[i].start + lines[i].text.len()).unwrap_or(last)
}

pub fn find_table_at(src: &str, offset: usize) -> Option<TableContext> {
    let lines = split_lines(src);
    if lines.is_empty() {
        return None;
    }
    let li = line_at(&lines, offset);
    for i in (0..=li).rev() {
        if i < li && (lines[i].text.trim().is_empty() || FENCE.is_match(lines[i].text)) {
            break;
        }
        let Some(block) = try_collect(&lines, i) else { continue };
        // A cursor on the newline just past the table is already outside it.
        if offset >= block.start && offset <= block.end {
            let cols = split_row(lines[block.first].text).len();
            let mut aligns = block.aligns;
            aligns.resize(cols, Align::None);
            return Some(TableContext {
                start: block.start,
                end: block.end,
                raw: src[block.start..block.end].to_string(),
                cols,
                body_rows: block.len - 2,
                aligns,
            });
        }
    }
    None
}

/// Rows of the table with the delimiter row removed, row 0 = header.
///
/// The matrix is the logical table, not the source: the delimiter row carries
/// no data and is regenerated from `aligns` on serialize. Including it here
/// would corrupt every row operation, since a sort or a move would drag it out
/// of position 1.
pub fn parse_table(ctx: &TableContext) -> Cells {
    split_lines(&ctx.raw)
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != 1)
        .map(|(_, line)| {
            let mut row: Vec<String> =
                split_row(line.text).into_iter().map(|(s, e)| line.text[s..e].to_string()).collect();
            row.resize(ctx.cols, String::new());
            row
        })
        .collect()
}

/// Map a document offset to a matrix position. Row 0 is the header, matching
/// `parse_table`; the delimiter row is skipped rather than offset around, so
/// callers never have to know it exists.
///
/// None when the offset is not inside a cell — an empty line under the
/// table, or the delimiter row itself.
pub fn cell_at(ctx: &TableContext, offset: usize) -> Option<CellPos> {
    let local = offset.checked_sub(ctx.start).filter(|&l| l <= ctx.raw.len())?;
    let lines = split_lines(&ctx.raw);
    let raw_line = lines.iter().position(|l| local >= l.start && local <= l.start + l.text.len())?;
    if raw_line == 1 {
        return None;
    }
    let row = if raw_line == 0 { 0 } else { raw_line - 1 };
    let line = &lines[raw_line];
    let inset = local - line.start;
    let spans = split_row(line.text);
    // A caret on a separator belongs to the cell it just left, which is what
    // typing at the end of a cell should extend.
    let col = spans.iter().position(|&(s, e)| inset >= s && inset <= e).unwrap_or(spans.len() - 1);
    (col < ctx.cols).then_some(CellPos { row, col })
}

/// Source range of a visible cell; shares the parser's escaped-pipe rules.
pub fn cell_range(ctx: &TableContext, cell: CellPos) -> CellRange {
    let lines = split_lines(&ctx.raw);
    let line = &lines[if cell.row == 0 { 0 } else { cell.row + 1 }];
    let end = line.text.len() - usize::from(line.text.ends_with('|'));
    let span = split_row(line.text).get(cell.col).copied();
    let (s, e) = span.unwrap_or((end, end));
    let base = ctx.start + line.start;
    CellRange { from: base + s, to: base + e, missing: span.is_none() }
}

/// Display width: CJK and fullwidth forms occupy two columns.
pub fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xA960..=0xA97F | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

/// A raw pipe in cell text must be escaped or it splits the cell.
pub fn escape_pipes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev = '\0';
    for c in s.chars() {
        if c == '|' && prev != '\\' {
            out.push('\\');
        }
        out.push(c);
        prev = c;
    }
    out
}

pub fn unescape_pipes(s: &str) -> String {
    s.replace("\\|", "|")
}

fn inline_cell(s: &str) -> String {
    s.replace("\r\n", "<br>").replace(['\r', '\n'], "<br>")
}

/// The delimiter row sets the column width, so it is a run of dashes as wide
/// as the cells. Widths are at least 3, which `:-:` needs.
fn align_delim(width: usize, align: Align) -> String {
    match align {
        Align::Center => format!(":{}:", "-".repeat(width - 2)),
        Align::Right => format!("{}:", "-".repeat(width - 1)),
        Align::Left => format!(":{}", "-".repeat(width - 1)),
        Align::None => "-".repeat(width),
    }
}

/// Widest cell per column in display columns, floored at 3 for the delimiter
/// and capped at 40 so one long cell does not stretch the whole source.
pub fn suggest_column_widths(cells: &[Vec<String>]) -> Vec<usize> {
    let cols = cells.iter().map(Vec::len).max().unwrap_or(0);
    (0..cols)
        .map(|c| {
            let w = cells.iter().filter_map(|r| r.get(c)).map(|v| display_width(v)).fold(3, usize::max);
            w.min(40)
        })
        .collect()
}

/// The header row fixes the width: a longer row is truncated, a shorter one
/// padded. `pad` aligns the pipes by display width.
pub fn serialize_table(cells: &[Vec<String>], aligns: &[Align], pad: bool) -> String {
    let Some(header) = cells.first() else { return String::new() };
    let cols = header.len();
    if cols == 0 {
        return String::new();
    }
    let widths = pad.then(|| suggest_column_widths(cells));
    let render = |row: &[String]| {
        let parts: Vec<String> = (0..cols)
            .map(|c| {
                let mut text = inline_cell(row.get(c).map_or("", String::as_str));
                if let Some(w) = &widths {
                    // Widths are display columns: pad by the display width the
                    // text is short, not by its length.
                    let short = w[c].saturating_sub(display_width(&text));
                    text.extend(std::iter::repeat_n(' ', short));
                }
                text
            })
            .collect();
        format!("| {} |", parts.join(" | "))
    };
    let delim: Vec<String> = (0..cols)
        .map(|c| align_delim(widths.as_ref().map_or(3, |w| w[c]), aligns.get(c).copied().unwrap_or(Align::None)))
        .collect();
    let mut lines = vec![render(header), format!("| {} |", delim.join(" | "))];
    lines.extend(cells[1..].iter().map(|r| render(r)));
    lines.join("\n")
}

/// Raw source line index for a matrix row: the delimiter row is line 1.
fn raw_line_for(row: usize) -> usize {
    if row >= 1 { row + 1 } else { row }
}

/// Keep the same line/cell/inset across an edit, falling back sensibly.
fn map_cursor(old: &str, next: &str, cursor: usize) -> usize {
    let cursor = cursor.min(old.len());
    let lines = split_lines(old);
    let next_lines = split_lines(next);
    if lines.is_empty() || next_lines.is_empty() {
        return 0;
    }
    let row = line_at(&lines, cursor);
    // Negative when the cursor sits on the newline that ends the line before.
    let offset = cursor as isize - lines[row].start as isize;
    let spans = split_row(lines[row].text);
    // Only the last span may be empty (a trailing cell); matching an empty span
    // would let it swallow the cursor from a real cell before it.
    let last = spans.len() - 1;
    let col = spans[..last]
        .iter()
        .position(|&(s, e)| offset >= s as isize && offset <= e as isize)
        .unwrap_or(last);
    let from = spans[col].0 as isize;

    let line = &next_lines[raw_line_for(row).min(next_lines.len() - 1)];
    let next_spans = split_row(line.text);
    let (s, e) = next_spans[col.min(next_spans.len() - 1)];
    let inset = (offset - from).clamp(0, (e - s) as isize) as usize;
    // The inset was measured in the old cell's bytes and may split a
    // character of the new one.
    next.floor_char_boundary(line.start + s + inset)
}

pub fn replace_table(
    src: &str,
    ctx: &TableContext,
    cells: &[Vec<String>],
    aligns: &[Align],
    pad: bool,
    cursor: Option<usize>,
) -> TableEdit {
    let next = serialize_table(cells, aligns, pad);
    let at = cursor.unwrap_or(ctx.start).saturating_sub(ctx.start);
    let cursor = map_cursor(&ctx.raw, &next, at) + ctx.start;
    TableEdit { text: format!("{}{}{}", &src[..ctx.start], next, &src[ctx.end..]), cursor }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ops::{insert_col, insert_row, set_cell};

    pub(crate) fn ctx_of(src: &str, offset: usize) -> TableContext {
        find_table_at(src, offset).expect("no table found")
    }

    pub(crate) fn cells(rows: &[&[&str]]) -> Cells {
        rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
    }

    fn round(src: &str) -> String {
        let ctx = ctx_of(src, 0);
        serialize_table(&parse_table(&ctx), &ctx.aligns, false)
    }

    const N: Align = Align::None;

    // ---- findTableAt

    const SRC: &str = "# Title\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\nafter";

    #[test]
    fn finds_the_table_from_a_cursor_inside_it() {
        let ctx = ctx_of(SRC, SRC.find("1 | 2").unwrap());
        assert_eq!(ctx.cols, 2);
        assert_eq!(ctx.body_rows, 1);
        assert_eq!(ctx.aligns, [N, N]);
        assert_eq!(ctx.raw, "| a | b |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn reports_start_and_end_offsets_that_slice_back_to_raw() {
        let ctx = ctx_of(SRC, SRC.find("1 | 2").unwrap());
        assert_eq!(&SRC[ctx.start..ctx.end], ctx.raw);
    }

    #[test]
    fn handles_a_cursor_in_the_header_and_on_the_delimiter_row() {
        assert!(find_table_at(SRC, SRC.find("| a").unwrap()).is_some());
        assert!(find_table_at(SRC, SRC.find("| ---").unwrap()).is_some());
    }

    #[test]
    fn handles_a_cursor_on_trailing_spaces_of_a_cell() {
        let padded = "| a   | b |\n| --- | --- |\n| 1 | 2 |";
        assert!(find_table_at(padded, padded.find('a').unwrap() + 4).is_some());
    }

    #[test]
    fn handles_a_cursor_in_an_empty_cell() {
        let gappy = "| a | b |\n| --- | --- |\n|  | 2 |";
        let at = gappy.find("\n|  | 2 |").unwrap() + 4;
        let ctx = find_table_at(gappy, at).unwrap();
        assert_eq!(parse_table(&ctx)[1], ["", "2"]);
    }

    #[test]
    fn returns_none_outside_a_table() {
        assert!(find_table_at(SRC, 0).is_none());
        assert!(find_table_at(SRC, SRC.len()).is_none());
    }

    #[test]
    fn requires_a_delimiter_row() {
        let no_delim = "| a | b |\n| 1 | 2 |";
        assert!(find_table_at(no_delim, no_delim.find('1').unwrap()).is_none());
    }

    #[test]
    fn reads_alignment_markers_from_the_delimiter_row() {
        let aligned = "| a | b | c | d |\n| :-- | :-: | --: | --- |\n| 1 | 2 | 3 | 4 |";
        assert_eq!(ctx_of(aligned, 0).aligns, [Align::Left, Align::Center, Align::Right, N]);
    }

    #[test]
    fn stops_at_a_blank_line() {
        assert_eq!(ctx_of("| a | b |\n| --- | --- |\n| 1 | 2 |\n\ntext | more", 0).body_rows, 1);
    }

    #[test]
    fn accepts_a_single_column_table() {
        let one = ctx_of("| a |\n| --- |\n| 1 |", 0);
        assert_eq!(one.cols, 1);
        assert_eq!(one.body_rows, 1);
    }

    // ---- parseTable

    #[test]
    fn parses_without_leading_or_trailing_pipes() {
        assert_eq!(parse_table(&ctx_of("a | b\n--- | ---\n1 | 2", 0)), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn parses_with_both_leading_and_trailing_pipes() {
        let src = "| a | b |\n| --- | --- |\n| 1 | 2 |";
        assert_eq!(parse_table(&ctx_of(src, 0)), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn omits_the_delimiter_row() {
        let src = "| a | b |\n| :-- | --: |\n| 1 | 2 |";
        assert_eq!(parse_table(&ctx_of(src, 0)), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn parses_a_header_only_table_as_a_single_row() {
        assert_eq!(parse_table(&ctx_of("| a | b |\n| --- | --- |", 0)), cells(&[&["a", "b"]]));
    }

    #[test]
    fn trims_cells_to_content_without_touching_inner_spacing() {
        let src = "|   a b   |  c   |\n| --- | --- |\n| a   b | c |";
        assert_eq!(parse_table(&ctx_of(src, 0))[0], ["a b", "c"]);
    }

    #[test]
    fn does_not_split_on_an_escaped_pipe() {
        let src = "| a \\| b | c |\n| --- | --- |\n| x | y |";
        assert_eq!(parse_table(&ctx_of(src, 0))[0], ["a \\| b", "c"]);
    }

    #[test]
    fn does_not_split_on_a_pipe_inside_a_code_span() {
        let src = "| `a|b` | c |\n| --- | --- |\n| x | y |";
        assert_eq!(parse_table(&ctx_of(src, 0))[0], ["`a|b`", "c"]);
    }

    #[test]
    fn handles_pipes_in_code_spans_and_escaped_pipes_in_the_same_row() {
        let src = "| `a|b` | c \\| d | e |\n| --- | --- | --- |\n| 1 | 2 | 3 |";
        assert_eq!(parse_table(&ctx_of(src, 0))[0], ["`a|b`", "c \\| d", "e"]);
    }

    #[test]
    fn treats_a_multi_backtick_span_as_one_span() {
        let src = "| ``a|`b`` | c |\n| --- | --- |\n| 1 | 2 |";
        assert_eq!(parse_table(&ctx_of(src, 0))[0], ["``a|`b``", "c"]);
    }

    #[test]
    fn treats_an_unclosed_backtick_as_opening_a_span_to_the_end() {
        let src = "| `a | b |\n| --- | --- |\n| 1 | 2 |";
        assert_eq!(parse_table(&ctx_of(src, 0))[0], ["`a | b |"]);
    }

    #[test]
    fn pads_ragged_rows_with_empty_strings() {
        let src = "| a | b | c |\n| --- | --- | --- |\n| 1 |";
        assert_eq!(parse_table(&ctx_of(src, 0))[1], ["1", "", ""]);
    }

    #[test]
    fn describes_a_lone_pipe_as_empty_cells() {
        assert_eq!(parse_table(&ctx_of("| a | b |\n| --- | --- |\n| |", 0))[1], ["", ""]);
    }

    #[test]
    fn normalizes_crlf_without_leaving_carriage_returns_in_cells() {
        let parsed = parse_table(&ctx_of("| a | b |\r\n| --- | --- |\r\n| 1 | 2 |", 0));
        assert_eq!(parsed, cells(&[&["a", "b"], &["1", "2"]]));
        assert!(!parsed.iter().flatten().any(|c| c.contains('\r')));
    }

    #[test]
    fn round_trips_a_crlf_source_to_lf() {
        assert_eq!(round("| a | b |\r\n| --- | --- |\r\n| 1 | 2 |"), "| a | b |\n| --- | --- |\n| 1 | 2 |");
    }

    // ---- serializeTable

    #[test]
    fn writes_padded_pipes_and_generates_the_delimiter_row() {
        let out = serialize_table(&cells(&[&["a", "b"], &["1", "2"]]), &[N, N], false);
        assert_eq!(out, "| a | b |\n| --- | --- |\n| 1 | 2 |");
    }

    #[test]
    fn emits_alignment_markers_sized_to_the_column() {
        let c = cells(&[&["aaaa", "bbbb", "cccc"], &["1", "2", "3"]]);
        let out = serialize_table(&c, &[Align::Left, Align::Center, Align::Right], true);
        assert_eq!(out.split('\n').nth(1).unwrap(), "| :--- | :--: | ---: |");
    }

    #[test]
    fn keeps_every_delimiter_at_least_three_dashes_wide() {
        let second = |c: Cells, a: Align| serialize_table(&c, &[a], false).split('\n').nth(1).unwrap().to_string();
        assert_eq!(second(cells(&[&["a"]]), N), "| --- |");
        assert_eq!(second(cells(&[&["a"], &["1"]]), Align::Center), "| :-: |");
        assert_eq!(second(cells(&[&["a"], &["1"]]), Align::Right), "| --: |");
    }

    #[test]
    fn truncates_rows_wider_than_the_header() {
        let out = serialize_table(&cells(&[&["a", "b"], &["1", "2", "3"]]), &[N, N], false);
        assert_eq!(out.split('\n').nth(2).unwrap(), "| 1 | 2 |");
    }

    #[test]
    fn never_writes_a_delimiter_row_twice() {
        let out = serialize_table(&cells(&[&["a", "b"], &["1", "2"]]), &[N, N], false);
        assert_eq!(out.split('\n').filter(|l| l.contains("---")).count(), 1);
    }

    #[test]
    fn turns_embedded_newlines_into_br() {
        let out = serialize_table(&cells(&[&["a\nb", "c"]]), &[N, N], false);
        assert_eq!(out.split('\n').next().unwrap(), "| a<br>b | c |");
    }

    #[test]
    fn returns_an_empty_string_for_an_empty_matrix() {
        assert_eq!(serialize_table(&[], &[], false), "");
    }

    #[test]
    fn pads_with_pad_true() {
        let out = serialize_table(&cells(&[&["name", "n"], &["a-longer-cell", "1"]]), &[N, Align::Right], true);
        let lines: Vec<_> = out.split('\n').collect();
        // Column 0 is 14 wide, column 1 is 3 wide (the delimiter floor).
        assert_eq!(lines, ["| name          | n   |", "| ------------- | --: |", "| a-longer-cell | 1   |"]);
    }

    #[test]
    fn counts_cjk_cells_as_two_columns_wide_when_padding() {
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("ab"), 2);
        let out = serialize_table(&cells(&[&["名称", "x"], &["a", "y"]]), &[N, N], true);
        let lines: Vec<_> = out.split('\n').collect();
        // 名称 is four display columns, so it needs no padding at all.
        assert_eq!(lines, ["| 名称 | x   |", "| ---- | --- |", "| a    | y   |"]);
    }

    // ---- suggestColumnWidths

    #[test]
    fn uses_the_widest_cell_with_a_floor_of_3() {
        assert_eq!(suggest_column_widths(&cells(&[&["a"], &["12345"]])), [5]);
    }

    #[test]
    fn caps_runaway_columns() {
        assert_eq!(suggest_column_widths(&[vec!["x".repeat(200)]]), [40]);
    }

    #[test]
    fn covers_every_column_even_when_rows_are_ragged() {
        assert_eq!(suggest_column_widths(&cells(&[&["a"], &["b", "ccccc"]])).len(), 2);
    }

    // ---- escapePipes

    #[test]
    fn escapes_bare_pipes_and_leaves_escaped_ones_alone() {
        assert_eq!(escape_pipes("a|b"), "a\\|b");
        assert_eq!(escape_pipes("a\\|b"), "a\\|b");
    }

    // ---- replaceTable

    const DOC: &str = "before\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\nafter";

    #[test]
    fn replaces_only_the_table_block() {
        let ctx = ctx_of(DOC, DOC.find("1 | 2").unwrap());
        let c = set_cell(&parse_table(&ctx), CellPos { row: 1, col: 0 }, "9");
        let edit = replace_table(DOC, &ctx, &c, &ctx.aligns, false, None);
        assert_eq!(edit.text, "before\n\n| a | b |\n| --- | --- |\n| 9 | 2 |\n\nafter");
    }

    #[test]
    fn leaves_the_surrounding_text_untouched() {
        let ctx = ctx_of(DOC, DOC.find("1 | 2").unwrap());
        assert_eq!(replace_table(DOC, &ctx, &parse_table(&ctx), &ctx.aligns, false, None).text, DOC);
    }

    #[test]
    fn places_the_cursor_inside_the_edited_cell() {
        let ctx = ctx_of(DOC, DOC.find("1 | 2").unwrap());
        // Row 2 does not exist, so the table is unchanged; the cursor stays on `1`.
        let c = set_cell(&parse_table(&ctx), CellPos { row: 2, col: 0 }, "12345");
        let edit = replace_table(DOC, &ctx, &c, &ctx.aligns, false, DOC.find("1 | 2"));
        assert_eq!(&edit.text[edit.cursor - 2..edit.cursor + 1], "| 1");
    }

    #[test]
    fn keeps_the_cursor_on_the_same_cell_when_a_column_is_appended_after_it() {
        let ctx = ctx_of(DOC, DOC.find("1 | 2").unwrap());
        let c = insert_col(&parse_table(&ctx), 1, false);
        let edit = replace_table(DOC, &ctx, &c, &ctx.aligns, false, Some(DOC.rfind("| 2 |").unwrap() + 2));
        assert_eq!(&edit.text[edit.cursor..edit.cursor + 2], "2 ");
    }

    #[test]
    fn keeps_the_cursor_sane_after_a_row_insert() {
        let ctx = ctx_of(DOC, DOC.find("1 | 2").unwrap());
        let c = insert_row(&parse_table(&ctx), 2, true);
        let edit = replace_table(DOC, &ctx, &c, &ctx.aligns, false, DOC.find("2 |"));
        assert!(edit.cursor > 0);
        assert!(edit.cursor < DOC.len() + 10);
    }

    #[test]
    fn never_puts_the_cursor_inside_a_character() {
        let src = "| a |\n| --- |\n| xyz |";
        let ctx = ctx_of(src, 0);
        let c = set_cell(&parse_table(&ctx), CellPos { row: 1, col: 0 }, "中");
        let edit = replace_table(src, &ctx, &c, &ctx.aligns, false, src.find("yz"));
        assert!(edit.text.is_char_boundary(edit.cursor));
    }

    // ---- cell ranges

    #[test]
    fn maps_visible_cells_back_to_exact_source_ranges() {
        let source = "正文\r\n\r\n| A | B |\r\n| --- | --- |\r\n| x\\|y | `a|b` |";
        let ctx = find_table_at(source, source.find("| A").unwrap()).unwrap();
        for (col, text) in ["x\\|y", "`a|b`"].into_iter().enumerate() {
            let range = cell_range(&ctx, CellPos { row: 1, col });
            assert_eq!(&source[range.from..range.to], text);
            assert_eq!(cell_at(&ctx, range.from), Some(CellPos { row: 1, col }));
        }
    }

    #[test]
    fn keeps_later_rows_and_padded_cells_addressable() {
        let source = "| A | B |\n| --- | --- |\n| 1 | x |\n| 2 | y |\n| 3 | z |\n| 4 |";
        let ctx = find_table_at(source, source.find('4').unwrap()).unwrap();
        assert!(cell_range(&ctx, CellPos { row: 4, col: 1 }).missing);
        assert!(!cell_range(&ctx, CellPos { row: 4, col: 0 }).missing);
    }

    // ---- cellAt

    const GRID: &str = "前言\n| a | b | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n结尾";

    fn at(offset: usize) -> Option<CellPos> {
        find_table_at(GRID, offset).and_then(|ctx| cell_at(&ctx, offset))
    }

    fn pos(needle: &str) -> usize {
        GRID.find(needle).unwrap()
    }

    fn rc(row: usize, col: usize) -> Option<CellPos> {
        Some(CellPos { row, col })
    }

    #[test]
    fn maps_the_header_row_to_row_0() {
        assert_eq!(at(pos("a") + 1), rc(0, 0));
        assert_eq!(at(pos("b") + 1), rc(0, 1));
        assert_eq!(at(pos("c") + 1), rc(0, 2));
    }

    #[test]
    fn reports_nothing_for_the_delimiter_row() {
        assert_eq!(at(pos("| --- |") + 3), None);
    }

    #[test]
    fn maps_body_rows_to_1_and_2() {
        assert_eq!(at(pos("1")), rc(1, 0));
        assert_eq!(at(pos("4")), rc(2, 0));
        assert_eq!(at(pos("5")), rc(2, 1));
    }

    #[test]
    fn puts_a_caret_on_a_separator_with_the_cell_it_just_left() {
        let sep = pos("| 1 |") + GRID[pos("| 1 |")..].find(" | ").unwrap();
        assert_eq!(at(sep), rc(1, 0));
    }

    #[test]
    fn cell_at_returns_none_outside_the_table() {
        assert_eq!(at(0), None);
        // The last character of 结尾, in bytes.
        assert_eq!(at(GRID.len() - '尾'.len_utf8()), None);
    }

    #[test]
    fn holds_the_last_cell_at_the_end_of_a_row() {
        assert_eq!(at(pos("| 1 |") + "| 1 | 2 | 3 |".len() - 1), rc(1, 2));
    }
}
