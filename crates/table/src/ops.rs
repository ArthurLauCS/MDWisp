//! Table edit operations. Every function is pure: it returns a new matrix and
//! never mutates its argument.
//!
//! Cell text is stored exactly as it appears in the markdown source, so the
//! values produced here can be handed straight back to `serialize_table`.

use std::cmp::Ordering;
use std::sync::LazyLock;

use mdwisp_core::i18n::Locale;
use regex::Regex;

use crate::model::{Align, CellPos, Cells, serialize_table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsKind {
    Sum,
    Avg,
    Count,
    Min,
    Max,
}

static MERGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)^<t[dh]\s+(colspan|rowspan)="([0-9]+)"\s*>(.*)</t[dh]>$"#).unwrap());
static TRAILING_BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:<br\s*/?>)+$").unwrap());
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[+-]?([0-9]+\.?[0-9]*|\.[0-9]+)([eE][+-]?[0-9]+)?%?$").unwrap());
static ISO_DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]{4})-([0-9]{2})-([0-9]{2})(?:[T ]([0-9]{2}):([0-9]{2})(?::([0-9]{2}))?(?:\.([0-9]+))?Z?)?$")
        .unwrap()
});
static SLASH_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]{4})/([0-9]{1,2})/([0-9]{1,2})$").unwrap());

const STAT_LABELS: [[&str; 5]; 2] =
    [["合计", "平均", "计数", "最小", "最大"], ["Sum", "Average", "Count", "Minimum", "Maximum"]];

fn col_count(cells: &[Vec<String>]) -> usize {
    cells.iter().map(Vec::len).max().unwrap_or(0)
}

fn empty_row(cols: usize) -> Vec<String> {
    vec![String::new(); cols]
}

fn padded(row: &[String], cols: usize) -> Vec<String> {
    let mut out = row.to_vec();
    out.resize(cols, String::new());
    out
}

/// Insert `value` after `at`, or directly before it when `before` is set.
fn insert_at<T: Clone>(list: &[T], at: usize, value: T, before: bool) -> Vec<T> {
    let mut out = list.to_vec();
    let i = if before { at } else { at + 1 };
    out.insert(i.min(out.len()), value);
    out
}

pub fn insert_row(cells: &[Vec<String>], at: usize, before: bool) -> Cells {
    insert_at(cells, at, empty_row(col_count(cells)), before)
}

pub fn insert_col(cells: &[Vec<String>], at: usize, before: bool) -> Cells {
    let cols = col_count(cells);
    cells.iter().map(|row| insert_at(&padded(row, cols), at, String::new(), before)).collect()
}

pub fn delete_row(cells: &[Vec<String>], at: usize) -> Cells {
    if cells.is_empty() {
        return Vec::new();
    }
    let out: Cells = cells.iter().enumerate().filter(|&(i, _)| i != at).map(|(_, r)| r.clone()).collect();
    // A table needs its header row — never drop the last one.
    if out.is_empty() { vec![empty_row(col_count(cells))] } else { out }
}

pub fn delete_col(cells: &[Vec<String>], at: usize) -> Cells {
    let cols = col_count(cells);
    if cols <= 1 {
        return cells.iter().map(|_| vec![String::new()]).collect();
    }
    cells
        .iter()
        .map(|row| padded(row, cols).into_iter().enumerate().filter(|&(i, _)| i != at).map(|(_, v)| v).collect())
        .collect()
}

pub fn move_row(cells: &[Vec<String>], from: usize, to: usize) -> Cells {
    let mut out = cells.to_vec();
    // Out-of-range indices are caller mistakes — leave the table alone rather
    // than snapping the row to an edge. `move_col` follows the same rule.
    if from < out.len() && to < out.len() {
        let row = out.remove(from);
        out.insert(to, row);
    }
    out
}

pub fn move_col(cells: &[Vec<String>], from: usize, to: usize) -> Cells {
    let cols = col_count(cells);
    if cols == 0 {
        return Vec::new();
    }
    cells
        .iter()
        .map(|row| {
            let mut out = padded(row, cols);
            if from < cols && to < cols {
                let v = out.remove(from);
                out.insert(to, v);
            }
            out
        })
        .collect()
}

pub fn transpose(cells: &[Vec<String>]) -> Cells {
    (0..col_count(cells)).map(|c| cells.iter().map(|row| row.get(c).cloned().unwrap_or_default()).collect()).collect()
}

pub fn set_cell(cells: &[Vec<String>], pos: CellPos, value: &str) -> Cells {
    let mut out = cells.to_vec();
    if let Some(row) = out.get_mut(pos.row) {
        row[pos.col] = value.to_string();
    }
    out
}

fn in_bounds(cells: &[Vec<String>], pos: CellPos) -> bool {
    cells.get(pos.row).is_some_and(|r| pos.col < r.len())
}

/// Empty the cell but leave the row and column structure alone.
pub fn clear_cell(cells: &[Vec<String>], pos: CellPos) -> Cells {
    if !in_bounds(cells, pos) {
        return cells.to_vec();
    }
    set_cell(cells, pos, "")
}

/// Toggle a `<br>` at the end of a cell — the only way markdown holds two lines
/// in one cell. Toggling rather than appending keeps a second press from
/// stacking a run of breaks the user did not ask for.
pub fn toggle_cell_newline(cells: &[Vec<String>], pos: CellPos) -> Cells {
    if !in_bounds(cells, pos) {
        return cells.to_vec();
    }
    let value = &cells[pos.row][pos.col];
    let next = if TRAILING_BREAK.is_match(value) {
        TRAILING_BREAK.replace(value, "").into_owned()
    } else {
        format!("{value}<br>")
    };
    set_cell(cells, pos, &next)
}

/// One row as a standalone, pasteable table with a real header row.
pub fn row_as_table(cells: &[Vec<String>], row: usize, aligns: &[Align]) -> String {
    let Some(line) = cells.get(row) else { return String::new() };
    // The header is its own header; anything below it needs the real one above,
    // or the pasted markdown would not parse as a table at all.
    if row == 0 {
        return serialize_table(&[line.clone()], aligns, false);
    }
    serialize_table(&[padded(&cells[0], col_count(cells)), line.clone()], aligns, false)
}

/// The row's markdown and the matrix with that row removed.
pub fn cut_row(cells: &[Vec<String>], row: usize, aligns: &[Align]) -> (String, Cells) {
    (row_as_table(cells, row, aligns), delete_row(cells, row))
}

pub fn set_column_align(aligns: &[Align], col: usize, align: Align) -> Vec<Align> {
    let mut out = aligns.to_vec();
    if out.len() <= col {
        out.resize(col + 1, Align::None);
    }
    out[col] = align;
    out
}

/// Toggle the first row between header and body. Markdown has no way to hold a
/// body row above the header, so a "no header" table keeps an empty header row:
/// turning it on drops that blank row, turning it off inserts one and demotes
/// the old header into the body.
pub fn set_header_row(cells: &[Vec<String>], on: bool) -> Cells {
    let blank = cells.len() > 1 && cells[0].iter().all(String::is_empty);
    match (on, blank) {
        (true, true) => cells.to_vec(),
        (false, true) => cells[1..].to_vec(),
        _ => [vec![empty_row(col_count(cells))], cells.to_vec()].concat(),
    }
}

pub fn is_merged(cell: &str) -> bool {
    MERGE.is_match(cell)
}

/// Merge `span` cells rightwards into `pos`, blanking the covered cells.
pub fn merge_right(cells: &[Vec<String>], pos: CellPos, span: usize) -> Cells {
    let width = col_count(cells);
    let Some(row) = cells.get(pos.row) else { return cells.to_vec() };
    let end = (pos.col + span.max(1) - 1).min(width.saturating_sub(1));
    if span < 2 || end <= pos.col {
        return cells.to_vec();
    }
    let content = row.iter().skip(pos.col).take(end - pos.col + 1).map(String::as_str).collect::<Vec<_>>().join(" ");
    let mut out = cells.to_vec();
    let target = &mut out[pos.row];
    target.resize(target.len().max(width), String::new());
    target[pos.col] = format!(r#"<td colspan="{}">{content}</td>"#, end - pos.col + 1);
    for c in &mut target[pos.col + 1..=end] {
        c.clear();
    }
    out
}

/// Merge `span` cells downwards into `pos`, blanking the covered cells.
pub fn merge_down(cells: &[Vec<String>], pos: CellPos, span: usize) -> Cells {
    let end = (pos.row + span.max(1) - 1).min(cells.len().saturating_sub(1));
    if span < 2 || end <= pos.row {
        return cells.to_vec();
    }
    let width = col_count(cells);
    let content = cells[pos.row..=end]
        .iter()
        .filter_map(|r| r.get(pos.col).filter(|v| !v.is_empty()).map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    cells
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut out = r.clone();
            out.resize(out.len().max(width), String::new());
            if i == pos.row {
                out[pos.col] = format!(r#"<td rowspan="{}">{content}</td>"#, end - pos.row + 1);
            } else if i > pos.row && i <= end {
                out[pos.col].clear();
            }
            out
        })
        .collect()
}

/// Reverse a merge, dropping the span attribute but keeping the content.
pub fn split_cell(cells: &[Vec<String>], pos: CellPos) -> Cells {
    let mut out = cells.to_vec();
    let inner = cells.get(pos.row).and_then(|r| r.get(pos.col)).and_then(|v| MERGE.captures(v));
    if let Some(m) = inner {
        out[pos.row][pos.col] = m[3].to_string();
    }
    out
}

/// A number as a person writes one: thousands separators and a trailing `%`
/// are allowed, units are not.
fn to_number(v: &str) -> Option<f64> {
    let s = v.replace(',', "");
    let s = s.trim();
    if !NUMBER.is_match(s) {
        return None;
    }
    s.strip_suffix('%').unwrap_or(s).parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Days from 1970-01-01. Out-of-month days roll over into the next month, as
/// JavaScript's `Date` does for `2024-02-30`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

/// Milliseconds since the epoch, read as UTC. Only used as a sort key, so a
/// time zone would shift every value alike.
fn to_time(v: &str) -> Option<f64> {
    let s = v.trim();
    let c = ISO_DATE.captures(s).or_else(|| SLASH_DATE.captures(s))?;
    let n = |i: usize| c.get(i).map_or(0, |m| m.as_str().parse::<i64>().unwrap());
    let (month, day, h, min, sec) = (n(2), n(3), n(4), n(5), n(6));
    // Only the first three fraction digits are milliseconds.
    let ms = c.get(7).map_or(0, |m| format!("{:0<3}", &m.as_str()[..m.len().min(3)]).parse::<i64>().unwrap());
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && min <= 59
        && sec <= 59
        && (h < 24 || h == 24 && min == 0 && sec == 0 && ms == 0);
    valid.then(|| (days_from_civil(n(1), month, day) * 86_400_000 + ((h * 60 + min) * 60 + sec) * 1000 + ms) as f64)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Date,
    Text,
}

fn column_kind(values: &[&str]) -> Kind {
    let filled: Vec<&str> = values.iter().copied().filter(|v| !v.trim().is_empty()).collect();
    if filled.is_empty() {
        Kind::Text
    } else if filled.iter().all(|v| to_number(v).is_some()) {
        Kind::Number
    } else if filled.iter().all(|v| to_time(v).is_some()) {
        Kind::Date
    } else {
        Kind::Text
    }
}

/// Case-insensitive, lowercase before uppercase on ties. Approximates the
/// `zh-Hans-CN` collation the TypeScript used; Han characters fall in code
/// point order rather than pinyin, which would need ICU collation data.
fn collate(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

enum Key<'a> {
    Blank,
    Num(f64),
    Text(&'a str),
}

/// Sort by one column, detecting number/date/string from the values. Empty
/// cells always sink to the bottom, and ties keep their original order.
pub fn sort_by_column(cells: &[Vec<String>], col: usize, dir: SortDir, has_header: bool) -> Cells {
    let (header, body) = cells.split_at(usize::from(has_header && !cells.is_empty()));
    let values: Vec<&str> = body.iter().map(|r| r.get(col).map_or("", String::as_str)).collect();
    let kind = column_kind(&values);
    let keys: Vec<Key> = values
        .iter()
        .map(|&v| match kind {
            _ if v.trim().is_empty() => Key::Blank,
            Kind::Number => to_number(v).map_or(Key::Blank, Key::Num),
            Kind::Date => to_time(v).map_or(Key::Blank, Key::Num),
            Kind::Text => Key::Text(v),
        })
        .collect();
    let mut order: Vec<usize> = (0..body.len()).collect();
    // A stable sort keeps ties in their original order.
    order.sort_by(|&a, &b| {
        let cmp = match (&keys[a], &keys[b]) {
            // Blanks stay last in both directions.
            (Key::Blank, Key::Blank) => return Ordering::Equal,
            (Key::Blank, _) => return Ordering::Greater,
            (_, Key::Blank) => return Ordering::Less,
            (Key::Num(x), Key::Num(y)) => x.total_cmp(y),
            (Key::Text(x), Key::Text(y)) => collate(x, y),
            _ => Ordering::Equal,
        };
        if dir == SortDir::Desc { cmp.reverse() } else { cmp }
    });
    header.iter().cloned().chain(order.into_iter().map(|i| body[i].clone())).collect()
}

/// `String(n)` in JavaScript, so a statistic reads the same in either app.
pub(crate) fn js_number(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if n.abs() >= 1e21 || n.abs() < 1e-6 {
        let s = format!("{n:e}");
        return if s.contains("e-") { s } else { s.replace('e', "e+") };
    }
    n.to_string()
}

/// True for a row this module previously appended. `append_stats_row` only
/// labels its row when there is a second column to put the value in, so a
/// one-column table cannot be recognised and keeps the old folding behaviour.
/// A data row that happens to be labelled `合计` is indistinguishable from a
/// summary row and will be skipped too — the accepted cost of not asking the
/// user to confirm.
fn is_stats_row(row: &[String], cols: usize) -> bool {
    cols >= 2 && row.first().is_some_and(|v| STAT_LABELS.iter().flatten().any(|l| *l == v.trim()))
}

/// Append a summary row for one column.
///
/// Every row is scanned — the header row is included, so a table with no header
/// still works; its text just never parses as a number. `Count` counts the
/// values that also feed `Sum`/`Avg` (numeric cells), not non-empty cells, so
/// the five kinds agree on what a value is.
///
/// A stats row already in the table is left out of the scan, so running the
/// same statistic twice reports the data rather than its own previous answer.
/// The rows below it stay: statistics are appended, never reordered to the end.
pub fn append_stats_row(cells: &[Vec<String>], col: usize, kind: StatsKind, language: Locale) -> Cells {
    let cols = col_count(cells);
    if cols == 0 {
        return cells.to_vec();
    }
    let values: Vec<f64> = cells
        .iter()
        .filter(|r| !is_stats_row(r, cols))
        .filter_map(|r| to_number(r.get(col).map_or("", String::as_str)))
        .collect();
    let sum: f64 = values.iter().sum();
    let result = match kind {
        StatsKind::Count => values.len().to_string(),
        _ if values.is_empty() => String::new(),
        StatsKind::Sum => js_number(sum),
        StatsKind::Avg => js_number(sum / values.len() as f64),
        StatsKind::Min => js_number(values.iter().copied().fold(f64::INFINITY, f64::min)),
        StatsKind::Max => js_number(values.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
    };
    let mut row = empty_row(cols);
    if cols > 1 {
        row[0] = STAT_LABELS[usize::from(language == Locale::En)][kind as usize].to_string();
        row[1] = result;
    } else {
        row[0] = result;
    }
    let mut out = cells.to_vec();
    out.push(row);
    out
}

/// Re-align the source; a CJK cell counts as two columns wide.
pub fn pad_table(cells: &[Vec<String>], aligns: &[Align]) -> String {
    serialize_table(cells, aligns, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{cells, ctx_of};
    use crate::model::{display_width, find_table_at, parse_table};

    const N: Align = Align::None;

    fn p(row: usize, col: usize) -> CellPos {
        CellPos { row, col }
    }

    fn base() -> Cells {
        cells(&[&["h1", "h2"], &["a", "b"], &["c", "d"]])
    }

    // ---- structural ops

    #[test]
    fn insert_row_after_an_index() {
        assert_eq!(insert_row(&base(), 1, false), cells(&[&["h1", "h2"], &["a", "b"], &["", ""], &["c", "d"]]));
    }

    #[test]
    fn insert_row_before_an_index() {
        assert_eq!(insert_row(&base(), 1, true), cells(&[&["h1", "h2"], &["", ""], &["a", "b"], &["c", "d"]]));
    }

    #[test]
    fn insert_row_clamps_out_of_bounds_indices() {
        assert_eq!(insert_row(&base(), 99, false).len(), 4);
        assert_eq!(insert_row(&base(), 0, true)[0], ["", ""]);
    }

    #[test]
    fn insert_col_at_the_end_and_start() {
        assert_eq!(insert_col(&base(), 1, false).iter().map(Vec::len).collect::<Vec<_>>(), [3, 3, 3]);
        assert_eq!(insert_col(&base(), 0, true)[0], ["", "h1", "h2"]);
    }

    #[test]
    fn delete_row_removes_one_row() {
        assert_eq!(delete_row(&base(), 1), cells(&[&["h1", "h2"], &["c", "d"]]));
    }

    #[test]
    fn delete_row_on_the_only_row_returns_a_sane_one_row_table() {
        assert_eq!(delete_row(&cells(&[&["h1", "h2"]]), 0), cells(&[&["", ""]]));
    }

    #[test]
    fn delete_row_out_of_bounds_is_a_no_op() {
        assert_eq!(delete_row(&base(), 9), base());
    }

    #[test]
    fn delete_col_removes_one_column_from_every_row() {
        assert_eq!(delete_col(&base(), 0), cells(&[&["h2"], &["b"], &["d"]]));
    }

    #[test]
    fn delete_col_refuses_to_remove_the_last_column() {
        assert_eq!(delete_col(&cells(&[&["only"], &["x"]]), 0), cells(&[&[""], &[""]]));
    }

    #[test]
    fn move_row_moves_down_and_up() {
        assert_eq!(move_row(&base(), 1, 2)[1], ["c", "d"]);
        assert_eq!(move_row(&base(), 2, 1)[1], ["c", "d"]);
    }

    #[test]
    fn move_row_out_of_bounds_is_a_no_op_and_identity_when_equal() {
        assert_eq!(move_row(&base(), 1, 99), base());
        assert_eq!(move_row(&base(), 99, 0), base());
        assert_eq!(move_row(&base(), 1, 1), base());
    }

    #[test]
    fn move_col_moves_between_columns() {
        assert_eq!(move_col(&base(), 0, 1), cells(&[&["h2", "h1"], &["b", "a"], &["d", "c"]]));
    }

    #[test]
    fn move_col_out_of_bounds_is_a_no_op() {
        assert_eq!(move_col(&base(), 0, 99), base());
        assert_eq!(move_col(&base(), 5, 0), base());
        assert_eq!(move_col(&base(), 0, 1), cells(&[&["h2", "h1"], &["b", "a"], &["d", "c"]]));
    }

    #[test]
    fn transpose_flips_rows_and_columns() {
        assert_eq!(transpose(&base()), cells(&[&["h1", "a", "c"], &["h2", "b", "d"]]));
    }

    #[test]
    fn transpose_of_an_empty_matrix_stays_empty() {
        assert_eq!(transpose(&[]), Cells::new());
    }

    #[test]
    fn transpose_twice_is_the_identity() {
        assert_eq!(transpose(&transpose(&base())), base());
    }

    // "every op leaves the input untouched" holds by construction: every op
    // takes `&[Vec<String>]`.

    // ---- cell content ops

    #[test]
    fn set_cell_writes_one_cell() {
        assert_eq!(set_cell(&cells(&[&["h1", "h2"], &["a", "b"]]), p(1, 1), "z")[1], ["a", "z"]);
    }

    #[test]
    fn set_column_align_extends_the_align_list_as_needed() {
        assert_eq!(set_column_align(&[], 1, Align::Right), [N, Align::Right]);
    }

    #[test]
    fn set_header_row_on_inserts_an_empty_header_when_needed() {
        assert_eq!(set_header_row(&cells(&[&["a", "b"]]), true)[0], ["", ""]);
    }

    #[test]
    fn set_header_row_on_is_a_no_op_when_the_header_is_already_empty() {
        let c = cells(&[&["", ""], &["a", "b"]]);
        assert_eq!(set_header_row(&c, true), c);
    }

    #[test]
    fn set_header_row_off_drops_an_empty_header_into_the_body() {
        assert_eq!(set_header_row(&cells(&[&["", ""], &["a", "b"]]), false), cells(&[&["a", "b"]]));
    }

    #[test]
    fn set_header_row_off_keeps_a_blank_header_row_for_validity() {
        assert_eq!(
            set_header_row(&cells(&[&["h", "i"], &["a", "b"]]), false),
            cells(&[&["", ""], &["h", "i"], &["a", "b"]])
        );
    }

    #[test]
    fn merge_right_emits_colspan_and_blanks_the_covered_cells() {
        let merged = merge_right(&cells(&[&["a", "b", "c"], &["1", "2", "3"]]), p(0, 0), 2);
        assert_eq!(merged[0], [r#"<td colspan="2">a b</td>"#, "", "c"]);
    }

    #[test]
    fn merge_right_clamps_to_the_last_column_and_ignores_span_below_2() {
        let c = cells(&[&["a", "b"], &["1", "2"]]);
        assert_eq!(merge_right(&c, p(0, 1), 5)[0], ["a", "b"]);
        assert_eq!(merge_right(&c, p(0, 0), 1), c);
    }

    #[test]
    fn merge_down_emits_rowspan_and_blanks_the_covered_cells() {
        let merged = merge_down(&cells(&[&["a", "b"], &["c", "d"], &["e", "f"]]), p(0, 0), 2);
        assert_eq!(merged[0][0], r#"<td rowspan="2">a c</td>"#);
        assert_eq!(merged[1][0], "");
        assert_eq!(merged[2][0], "e");
    }

    #[test]
    fn merge_down_clamps_to_the_last_row() {
        assert_eq!(merge_down(&cells(&[&["a"], &["b"]]), p(1, 0), 3), cells(&[&["a"], &["b"]]));
    }

    #[test]
    fn split_cell_reverses_a_colspan() {
        let c = cells(&[&[r#"<td colspan="2">a b</td>"#, ""], &["1", "2"]]);
        assert_eq!(split_cell(&c, p(0, 0))[0], ["a b", ""]);
    }

    #[test]
    fn split_cell_reverses_a_rowspan() {
        let c = cells(&[&[r#"<td rowspan="2">a c</td>"#, "b"], &[""], &["e"]]);
        assert_eq!(split_cell(&c, p(0, 0))[0][0], "a c");
    }

    #[test]
    fn split_cell_on_a_plain_cell_is_a_no_op() {
        let c = cells(&[&["plain"]]);
        assert_eq!(split_cell(&c, p(0, 0)), c);
    }

    #[test]
    fn is_merged_recognizes_the_emitted_forms_only() {
        assert!(is_merged(r#"<td colspan="2">x</td>"#));
        assert!(is_merged(r#"<td rowspan="3">x</td>"#));
        assert!(!is_merged("plain"));
        assert!(!is_merged(r#"<td colspan="2">x"#));
    }

    // ---- sortByColumn

    fn sorted_body(c: &[&[&str]], dir: SortDir) -> Vec<String> {
        sort_by_column(&cells(c), 0, dir, true)[1..].iter().map(|r| r[0].clone()).collect()
    }

    #[test]
    fn sorts_numbers_numerically() {
        assert_eq!(sorted_body(&[&["n"], &["10"], &["9"], &["100"], &["2"]], SortDir::Asc), ["2", "9", "10", "100"]);
    }

    #[test]
    fn sorts_dates_chronologically() {
        let c: &[&[&str]] = &[&["d"], &["2024-03-01"], &["2023-12-31"], &["2024-01-15"]];
        assert_eq!(sorted_body(c, SortDir::Asc), ["2023-12-31", "2024-01-15", "2024-03-01"]);
    }

    #[test]
    fn sorts_slash_dates_as_dates_too() {
        assert_eq!(sorted_body(&[&["d"], &["2024/2/1"], &["2023/10/05"]], SortDir::Asc), ["2023/10/05", "2024/2/1"]);
    }

    #[test]
    fn falls_back_to_string_order_when_the_column_mixes_types() {
        assert_eq!(sorted_body(&[&["v"], &["b"], &["a"], &["10"]], SortDir::Asc), ["10", "a", "b"]);
    }

    #[test]
    fn sorts_descending() {
        assert_eq!(sorted_body(&[&["n"], &["1"], &["3"], &["2"]], SortDir::Desc), ["3", "2", "1"]);
    }

    #[test]
    fn always_sinks_empty_cells_last_in_both_directions() {
        let c = cells(&[&["n"], &["3"], &[""], &["1"]]);
        let col0 = |c: Cells| c.into_iter().map(|r| r[0].clone()).collect::<Vec<_>>();
        assert_eq!(col0(sort_by_column(&c, 0, SortDir::Asc, true)), ["n", "1", "3", ""]);
        assert_eq!(col0(sort_by_column(&c, 0, SortDir::Desc, true)), ["n", "3", "1", ""]);
    }

    #[test]
    fn keeps_ties_in_their_original_order() {
        let c = cells(&[&["n", "tag"], &["1", "first"], &["1", "second"], &["1", "third"]]);
        let tags: Vec<_> = sort_by_column(&c, 0, SortDir::Asc, true)[1..].iter().map(|r| r[1].clone()).collect();
        assert_eq!(tags, ["first", "second", "third"]);
    }

    #[test]
    fn sorts_the_whole_matrix_without_a_header() {
        assert_eq!(sort_by_column(&cells(&[&["2", "b"], &["1", "a"]]), 0, SortDir::Asc, false)[0], ["1", "a"]);
    }

    #[test]
    fn tolerates_short_rows_when_reading_the_key_column() {
        let c = vec![vec!["n".to_string()], vec!["2".to_string()], vec![]];
        assert_eq!(sort_by_column(&c, 0, SortDir::Asc, true).len(), 3);
    }

    #[test]
    fn reads_dates_like_javascript() {
        assert_eq!(to_time("1970-01-02"), Some(86_400_000.0));
        assert_eq!(to_time("2024-02-30"), to_time("2024-03-01"));
        assert_eq!(to_time("2024-02-29T24:00"), to_time("2024-03-01"));
        assert_eq!(to_time("1970-01-01T00:00:01.5Z"), Some(1500.0));
        for bad in ["2024-13-01", "2024-01-32", "2024-01-00", "2024-01-01T25:00", "2024-01-01T10:60", "2024-01-01T24:30"] {
            assert_eq!(to_time(bad), None, "{bad}");
        }
    }

    // ---- formatting

    #[test]
    fn pad_table_produces_aligned_pipes() {
        let out = pad_table(&cells(&[&["a", "bbbb"], &["ccc", "d"]]), &[N, N]);
        assert_eq!(out.split('\n').collect::<Vec<_>>(), ["| a   | bbbb |", "| --- | ---- |", "| ccc | d    |"]);
    }

    #[test]
    fn pad_table_output_re_parses_to_the_same_cells() {
        let c = cells(&[&["名称", "qty"], &["苹果", "3"]]);
        let src = pad_table(&c, &[N, Align::Right]) + "\n";
        assert_eq!(parse_table(&ctx_of(&src, 0)), c);
    }

    #[test]
    fn every_padded_column_is_the_same_display_width() {
        let out = pad_table(&cells(&[&["名称", "q"], &["a", "bbbbb"]]), &[N, N]);
        let widths: std::collections::HashSet<_> = out.split('\n').map(display_width).collect();
        assert_eq!(widths.len(), 1);
    }

    fn last(c: Cells) -> Vec<String> {
        c.last().unwrap().clone()
    }

    #[test]
    fn append_stats_row_sums_a_column() {
        let c = cells(&[&["name", "n"], &["a", "2"], &["b", "3"]]);
        assert_eq!(last(append_stats_row(&c, 1, StatsKind::Sum, Locale::ZhCn)), ["合计", "5"]);
        let english = append_stats_row(&c, 1, StatsKind::Sum, Locale::En);
        assert_eq!(last(english.clone()), ["Sum", "5"]);
        assert_eq!(last(append_stats_row(&english, 1, StatsKind::Sum, Locale::ZhCn)), ["合计", "5"]);
        let twice = append_stats_row(&append_stats_row(&c, 1, StatsKind::Sum, Locale::ZhCn), 1, StatsKind::Sum, Locale::En);
        assert_eq!(last(twice), ["Sum", "5"]);
    }

    #[test]
    fn append_stats_row_averages_counts_mins_and_maxes() {
        let c = cells(&[&["name", "n"], &["a", "2"], &["b", "4"]]);
        let value = |k| last(append_stats_row(&c, 1, k, Locale::ZhCn))[1].clone();
        assert_eq!(value(StatsKind::Avg), "3");
        assert_eq!(value(StatsKind::Count), "2");
        assert_eq!(value(StatsKind::Min), "2");
        assert_eq!(value(StatsKind::Max), "4");
    }

    #[test]
    fn append_stats_row_ignores_non_numeric_cells() {
        let c = cells(&[&["name", "n"], &["a", "2"], &["b", "oops"], &["c", ""]]);
        assert_eq!(last(append_stats_row(&c, 1, StatsKind::Sum, Locale::ZhCn))[1], "2");
    }

    #[test]
    fn append_stats_row_counts_rows_it_can_read_a_number_from() {
        let c = cells(&[&["name", "n"], &["a", "2"]]);
        assert_eq!(last(append_stats_row(&c, 1, StatsKind::Count, Locale::ZhCn)), ["计数", "1"]);
    }

    #[test]
    fn append_stats_row_counts_numeric_cells_not_non_empty_ones() {
        let c = cells(&[&["name", "n"], &["a", "2"], &["b", "oops"], &["c", ""]]);
        assert_eq!(last(append_stats_row(&c, 1, StatsKind::Count, Locale::ZhCn))[1], "1");
        assert_eq!(last(append_stats_row(&c, 1, StatsKind::Sum, Locale::ZhCn))[1], "2");
    }

    #[test]
    fn append_stats_row_leaves_a_blank_label_for_a_single_column_table() {
        assert_eq!(last(append_stats_row(&cells(&[&["n"], &["2"]]), 0, StatsKind::Sum, Locale::ZhCn)), ["2"]);
    }

    #[test]
    fn formats_numbers_like_javascript() {
        assert_eq!(js_number(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(js_number(1e21), "1e+21");
        assert_eq!(js_number(1e-7), "1e-7");
        assert_eq!(js_number(-0.0), "0");
        assert_eq!(js_number(1.2345678901234568e20), "123456789012345680000");
    }

    // ---- clearCell

    #[test]
    fn clear_cell_empties_one_cell_and_keeps_the_structure() {
        let b = cells(&[&["h1", "h2"], &["a", "b"]]);
        assert_eq!(clear_cell(&b, p(1, 0)), cells(&[&["h1", "h2"], &["", "b"]]));
        assert_eq!(clear_cell(&b, p(0, 1))[0], ["h1", ""]);
    }

    #[test]
    fn clear_cell_on_an_empty_cell_is_a_no_op() {
        assert_eq!(clear_cell(&cells(&[&[""]]), p(0, 0)), cells(&[&[""]]));
    }

    #[test]
    fn clear_cell_ignores_an_out_of_bounds_row_or_column() {
        let b = cells(&[&["h1", "h2"], &["a", "b"]]);
        assert_eq!(clear_cell(&b, p(9, 0)), b);
        assert_eq!(clear_cell(&b, p(0, 9)), b);
    }

    #[test]
    fn clear_cell_drops_a_merged_cell_wrapper() {
        let merged = cells(&[&[r#"<td colspan="2">a</td>"#, ""], &["1", "2"]]);
        assert_eq!(clear_cell(&merged, p(0, 0))[0], ["", ""]);
    }

    // ---- toggleCellNewline

    #[test]
    fn toggle_cell_newline_appends_a_br() {
        assert_eq!(toggle_cell_newline(&cells(&[&["h1"], &["a"]]), p(1, 0)), cells(&[&["h1"], &["a<br>"]]));
    }

    #[test]
    fn toggle_cell_newline_removes_it_on_the_second_press() {
        let b = cells(&[&["h1"], &["a"]]);
        assert_eq!(toggle_cell_newline(&toggle_cell_newline(&b, p(1, 0)), p(1, 0)), b);
    }

    #[test]
    fn toggle_cell_newline_collapses_a_hand_typed_run_of_breaks() {
        assert_eq!(toggle_cell_newline(&cells(&[&["x<br><br>"]]), p(0, 0)), cells(&[&["x"]]));
    }

    #[test]
    fn toggle_cell_newline_treats_a_self_closing_br_as_a_break() {
        assert_eq!(toggle_cell_newline(&cells(&[&["x<br/>"]]), p(0, 0)), cells(&[&["x"]]));
    }

    #[test]
    fn toggle_cell_newline_adds_a_break_to_an_empty_cell() {
        assert_eq!(toggle_cell_newline(&cells(&[&[""]]), p(0, 0)), cells(&[&["<br>"]]));
    }

    #[test]
    fn toggle_cell_newline_ignores_an_out_of_bounds_position() {
        let b = cells(&[&["h1"], &["a"]]);
        assert_eq!(toggle_cell_newline(&b, p(5, 0)), b);
    }

    // ---- rowAsTable

    fn fruit() -> Cells {
        cells(&[&["名称", "数量"], &["苹果", "3"], &["梨", "5"]])
    }

    #[test]
    fn row_as_table_writes_the_row_under_a_real_header() {
        assert_eq!(row_as_table(&fruit(), 2, &[N, N]), "| 名称 | 数量 |\n| --- | --- |\n| 梨 | 5 |");
    }

    #[test]
    fn row_as_table_carries_the_column_alignments() {
        let out = row_as_table(&fruit(), 2, &[Align::Left, Align::Right]);
        assert_eq!(out.split('\n').nth(1).unwrap(), "| :-- | --: |");
    }

    #[test]
    fn row_as_table_re_parses_with_the_alignments_intact() {
        let src = row_as_table(&fruit(), 2, &[Align::Left, Align::Right]);
        assert_eq!(find_table_at(&src, 0).unwrap().aligns, [Align::Left, Align::Right]);
    }

    #[test]
    fn row_as_table_re_parses_to_exactly_the_header_plus_that_row() {
        let src = row_as_table(&fruit(), 1, &[N, N]);
        assert_eq!(parse_table(&find_table_at(&src, 0).unwrap()), cells(&[&["名称", "数量"], &["苹果", "3"]]));
    }

    #[test]
    fn row_as_table_survives_a_cell_containing_an_escaped_pipe() {
        let src = row_as_table(&cells(&[&["h"], &["a \\| b"]]), 1, &[N]);
        assert_eq!(parse_table(&find_table_at(&src, 0).unwrap())[1], ["a \\| b"]);
    }

    #[test]
    fn row_as_table_returns_nothing_for_an_out_of_bounds_row() {
        assert_eq!(row_as_table(&fruit(), 9, &[N, N]), "");
    }

    #[test]
    fn row_as_table_copies_the_only_row_of_a_one_row_table() {
        assert_eq!(row_as_table(&cells(&[&["solo"]]), 0, &[N]), "| solo |\n| --- |");
    }

    #[test]
    fn row_as_table_copies_the_header_as_a_header_only_table() {
        assert_eq!(row_as_table(&fruit(), 0, &[N, N]), "| 名称 | 数量 |\n| --- | --- |");
    }

    // ---- cutRow

    #[test]
    fn cut_row_returns_the_markdown_and_the_table_without_the_row() {
        let (markdown, rest) = cut_row(&base(), 1, &[N, N]);
        assert!(markdown.contains("| a | b |"));
        assert_eq!(rest, cells(&[&["h1", "h2"], &["c", "d"]]));
    }

    #[test]
    fn cut_row_leaves_a_valid_one_row_table_when_cutting_the_last_body_row() {
        assert_eq!(cut_row(&cells(&[&["h"], &["only"]]), 1, &[N]).1, cells(&[&["h"]]));
    }

    #[test]
    fn cut_row_never_drops_the_table_to_zero_rows() {
        assert_eq!(cut_row(&cells(&[&["h1", "h2"]]), 0, &[N, N]).1.len(), 1);
    }

    #[test]
    fn cut_row_ignores_an_out_of_bounds_row() {
        let (markdown, rest) = cut_row(&base(), 9, &[N, N]);
        assert_eq!(markdown, "");
        assert_eq!(rest, base());
    }
}
