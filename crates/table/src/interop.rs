//! Moving a table in and out of other formats: CSV/TSV, pasted HTML, JSON
//! and the `- 名称：值` bullet list.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::Value;

use crate::model::Cells;

static HTML_TABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<table.*?</table>").unwrap());
static HTML_ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<tr.*?</tr>").unwrap());
static HTML_CELL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<t[dh][^>]*>(.*?)</t[dh]>").unwrap());
static HTML_BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-*+]\s*(.*)$").unwrap());

const FULLWIDTH_COLON: char = '：';

/// Lines split on `\r\n`, `\r` or `\n`.
fn lines_any(text: &str) -> impl Iterator<Item = &str> {
    text.split("\r\n").flat_map(|p| p.split(['\r', '\n']))
}

/// Normalize ragged rows to a rectangle (row 0 = header).
pub fn normalize_rows(rows: &[Vec<String>]) -> Cells {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    rows.iter()
        .map(|r| {
            let mut out = r.clone();
            out.resize(cols, String::new());
            out
        })
        .collect()
}

/// The candidate that splits every line into the same number of fields wins;
/// among equals, the one that splits most.
fn sniff_delimiter(text: &str) -> char {
    let lines: Vec<&str> = lines_any(text).filter(|l| !l.trim().is_empty()).collect();
    let mut best = (',', -1);
    for d in ['\t', ',', '|', ';'] {
        if !lines.iter().any(|l| l.contains(d)) {
            continue;
        }
        let counts: Vec<usize> = lines.iter().map(|l| l.matches(d).count()).collect();
        let consistent = counts.iter().all(|&c| c == counts[0]);
        let score = if consistent { 1000 } else { 0 } + counts[0] as i64;
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

fn split_pipe_line(line: &str) -> Vec<String> {
    let line = line.trim_start();
    let line = line.strip_prefix('|').unwrap_or(line).trim_end();
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(|c| c.trim().to_string()).collect()
}

/// Minimal CSV/TSV reader: handles quotes, escaped quotes and embedded newlines.
fn parse_delimited(text: &str, delim: char) -> Cells {
    let mut rows: Cells = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(ch),
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' => {}
            _ if ch == delim => row.push(std::mem::take(&mut field)),
            _ => field.push(ch),
        }
    }
    row.push(field);
    rows.push(row);
    if rows.len() > 1 && rows.last().unwrap().iter().all(String::is_empty) {
        rows.pop();
    }
    rows.into_iter().map(|r| r.into_iter().map(|c| c.trim().to_string()).collect()).collect()
}

fn decode_entities(s: &str) -> String {
    // `&amp;` last, so `&amp;lt;` decodes to `&lt;` and not to `<`.
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Excel and web tables paste as HTML — pull the grid out before sniffing.
fn from_html_table(html: &str) -> Option<Cells> {
    let table = HTML_TABLE.find(html)?.as_str();
    let rows: Cells = HTML_ROW
        .find_iter(table)
        .map(|tr| {
            HTML_CELL
                .captures_iter(tr.as_str())
                .map(|td| {
                    // `<br>` is the only tag that carries meaning into a cell —
                    // it is how a line break survives. Park it out of reach of
                    // the tag stripper.
                    let parked = HTML_BR.replace_all(&td[1], "\0");
                    let text = decode_entities(&HTML_TAG.replace_all(&parked, ""));
                    text.replace('\0', "<br>").replace('\u{a0}', " ").trim().to_string()
                })
                .collect::<Vec<_>>()
        })
        .filter(|r| !r.is_empty())
        .collect();
    (!rows.is_empty()).then_some(rows)
}

/// A grid from pasted text: an HTML table when there is one, else delimited
/// text split on `delimiter` or the sniffed one.
pub fn from_delimited(text: &str, delimiter: Option<char>) -> Cells {
    if let Some(rows) = from_html_table(text) {
        return normalize_rows(&rows);
    }
    match delimiter.unwrap_or_else(|| sniff_delimiter(text)) {
        '|' => normalize_rows(&lines_any(text).filter(|l| !l.trim().is_empty()).map(split_pipe_line).collect::<Vec<_>>()),
        d => normalize_rows(&parse_delimited(text, d)),
    }
}

fn quote_field(v: &str, delim: char) -> String {
    if v.contains(delim) || v.contains(['"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

pub fn to_csv(cells: &[Vec<String>], delim: char) -> String {
    let rows: Vec<String> = cells
        .iter()
        .map(|row| row.iter().map(|v| quote_field(v, delim)).collect::<Vec<_>>().join(&delim.to_string()))
        .collect();
    rows.join("\n")
}

/// Body rows as an array of objects keyed by the header row, pretty-printed
/// with two-space indent like `JSON.stringify(rows, null, 2)`.
pub fn to_json(cells: &[Vec<String>]) -> String {
    let Some((headers, body)) = cells.split_first() else { return "[]".into() };
    if body.is_empty() {
        return "[]".into();
    }
    let q = |s: &str| serde_json::to_string(s).unwrap();
    let records: Vec<String> = body
        .iter()
        .map(|row| {
            // A repeated header keeps its first position and its last value,
            // as assigning to an object key does.
            let mut pairs: Vec<(&str, &str)> = Vec::new();
            for (c, h) in headers.iter().enumerate() {
                let v = row.get(c).map_or("", String::as_str);
                match pairs.iter_mut().find(|(k, _)| k == h) {
                    Some(p) => p.1 = v,
                    None => pairs.push((h, v)),
                }
            }
            if pairs.is_empty() {
                return "  {}".to_string();
            }
            let fields: Vec<String> = pairs.iter().map(|(k, v)| format!("    {}: {}", q(k), q(v))).collect();
            format!("  {{\n{}\n  }}", fields.join(",\n"))
        })
        .collect();
    format!("[\n{}\n]", records.join(",\n"))
}

/// An object with its keys in source order, which decides the column order.
struct Record(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Record;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Record, A::Error> {
                let mut out = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    out.push(entry);
                }
                Ok(Record(out))
            }
        }
        d.deserialize_map(V)
    }
}

fn stringify(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => crate::ops::js_number(n.as_f64().unwrap()),
        _ => v.to_string(),
    }
}

/// A JSON array of objects as a table; the header is every key in order of
/// first appearance.
pub fn from_json(text: &str) -> Result<Cells, serde_json::Error> {
    let data: Vec<Record> = serde_json::from_str(text)?;
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut headers: Vec<String> = Vec::new();
    for Record(fields) in &data {
        for (k, _) in fields {
            if !headers.contains(k) {
                headers.push(k.clone());
            }
        }
    }
    // A duplicated key reads its last value, as `JSON.parse` keeps it.
    let value = |r: &Record, h: &str| r.0.iter().rev().find(|(k, _)| k == h).map_or(String::new(), |(_, v)| stringify(v));
    let body: Cells = data.iter().map(|r| headers.iter().map(|h| value(r, h)).collect()).collect();
    Ok([vec![headers], body].concat())
}

/// `- 名称：值` one line per field, a blank line between body rows — the shape
/// this app's users expect.
pub fn to_list(cells: &[Vec<String>]) -> String {
    let Some((headers, body)) = cells.split_first() else { return String::new() };
    let rows: Vec<String> = body
        .iter()
        .map(|row| {
            let fields: Vec<String> = headers
                .iter()
                .enumerate()
                .map(|(c, h)| format!("- {h}{FULLWIDTH_COLON}{}", row.get(c).map_or("", String::as_str)))
                .collect();
            fields.join("\n")
        })
        .collect();
    rows.join("\n\n")
}

/// Bullets back into a table. Without `headers` the columns are the keys in
/// order of first appearance.
pub fn from_list(text: &str, headers: Option<&[String]>) -> Cells {
    let mut entries: Vec<(&str, &str)> = Vec::new();
    for line in lines_any(text) {
        let Some(m) = LIST.captures(line) else { continue };
        let body = m.get(1).unwrap().as_str().trim();
        if body.is_empty() {
            continue;
        }
        entries.push(match body.split_once(FULLWIDTH_COLON) {
            Some((k, v)) => (k.trim(), v.trim()),
            None => ("", body),
        });
    }

    let cols: Vec<String> = match headers {
        Some(h) => h.to_vec(),
        None => {
            let mut cols: Vec<String> = Vec::new();
            for &(key, _) in &entries {
                if !key.is_empty() && !cols.iter().any(|c| c == key) {
                    cols.push(key.to_string());
                }
            }
            cols
        }
    };
    if cols.is_empty() {
        return Vec::new();
    }

    // A repeated first field starts a new record — `- 名称：…` twice over.
    let first = cols[0].clone();
    let mut rows = vec![cols.clone()];
    for (key, value) in entries {
        if rows.len() == 1 || (!first.is_empty() && key == first) {
            rows.push(vec![String::new(); cols.len()]);
        }
        if let Some(c) = cols.iter().position(|h| h == key) {
            rows.last_mut().unwrap()[c] = value.to_string();
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::cells;

    // ---- CSV interop

    #[test]
    fn quotes_fields_containing_the_delimiter_quotes_or_newlines() {
        assert_eq!(to_csv(&cells(&[&["a,b", "say \"hi\"", "x\ny"]]), ','), "\"a,b\",\"say \"\"hi\"\"\",\"x\ny\"");
    }

    #[test]
    fn leaves_plain_fields_unquoted() {
        assert_eq!(to_csv(&cells(&[&["a", "b"]]), ','), "a,b");
    }

    #[test]
    fn round_trips_through_from_delimited() {
        let c = cells(&[&["a,b", "q\"q"], &["1", "2"]]);
        assert_eq!(from_delimited(&to_csv(&c, ','), None), c);
    }

    #[test]
    fn round_trips_a_field_with_an_embedded_newline() {
        let c = cells(&[&["x\ny", "z"]]);
        assert_eq!(from_delimited(&to_csv(&c, ','), None), c);
    }

    #[test]
    fn sniffs_tab_separated_text() {
        assert_eq!(from_delimited("a\tb\tc\n1\t2\t3", None), cells(&[&["a", "b", "c"], &["1", "2", "3"]]));
    }

    #[test]
    fn sniffs_commas() {
        assert_eq!(from_delimited("a,b\n1,2", None), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn sniffs_semicolons() {
        assert_eq!(from_delimited("a;b\n1;2", None), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn sniffs_pipes_and_strips_the_outer_ones() {
        assert_eq!(from_delimited("| a | b |\n| 1 | 2 |", None), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn prefers_the_delimiter_with_a_consistent_per_line_count() {
        assert_eq!(from_delimited("a,b;c\n1,2;3", None), cells(&[&["a", "b;c"], &["1", "2;3"]]));
    }

    #[test]
    fn honors_an_explicit_delimiter() {
        assert_eq!(from_delimited("a|b", Some('|')), cells(&[&["a", "b"]]));
    }

    #[test]
    fn pads_ragged_input_into_a_rectangle() {
        assert_eq!(from_delimited("a,b,c\n1,2", None), cells(&[&["a", "b", "c"], &["1", "2", ""]]));
    }

    #[test]
    fn drops_a_trailing_empty_line() {
        assert_eq!(from_delimited("a,b\n1,2\n", None), cells(&[&["a", "b"], &["1", "2"]]));
    }

    #[test]
    fn extracts_a_pasted_html_table() {
        let html = "<table><tr><td>Name</td><td>Age</td></tr><tr><td>Ann</td><td>30</td></tr></table>";
        assert_eq!(from_delimited(html, None), cells(&[&["Name", "Age"], &["Ann", "30"]]));
    }

    #[test]
    fn extracts_th_cells_and_strips_nested_markup_plus_entities() {
        let html = "<table><thead><tr><th>a &amp; b</th></tr></thead><tbody><tr><td><b>x</b><br>y</td></tr></tbody></table>";
        assert_eq!(from_delimited(html, None), cells(&[&["a & b"], &["x<br>y"]]));
    }

    #[test]
    fn falls_back_to_sniffing_when_html_has_no_table() {
        assert_eq!(from_delimited("<p>a</p>,b", None), cells(&[&["<p>a</p>", "b"]]));
    }

    // ---- JSON interop

    fn people() -> Cells {
        cells(&[&["name", "age"], &["Ann", "30"], &["Bo", "25"]])
    }

    #[test]
    fn keys_body_rows_by_the_header_row() {
        assert_eq!(
            to_json(&people()),
            "[\n  {\n    \"name\": \"Ann\",\n    \"age\": \"30\"\n  },\n  {\n    \"name\": \"Bo\",\n    \"age\": \"25\"\n  }\n]"
        );
    }

    #[test]
    fn returns_nothing_for_a_header_only_table() {
        assert_eq!(to_json(&cells(&[&["a", "b"]])), "[]");
    }

    #[test]
    fn round_trips_through_from_json() {
        assert_eq!(from_json(&to_json(&people())).unwrap(), people());
    }

    #[test]
    fn accepts_a_json_string() {
        assert_eq!(from_json(r#"[{"a":"1"}]"#).unwrap(), cells(&[&["a"], &["1"]]));
    }

    #[test]
    fn unions_keys_across_rows() {
        assert_eq!(from_json(r#"[{"a":"1"},{"b":"2"}]"#).unwrap(), cells(&[&["a", "b"], &["1", ""], &["", "2"]]));
    }

    #[test]
    fn stringifies_nested_values() {
        assert_eq!(from_json(r#"[{"a":{"b":1}}]"#).unwrap(), cells(&[&["a"], &[r#"{"b":1}"#]]));
    }

    #[test]
    fn treats_null_as_empty() {
        assert_eq!(from_json(r#"[{"a":null,"b":null}]"#).unwrap(), cells(&[&["a", "b"], &["", ""]]));
    }

    #[test]
    fn returns_an_empty_matrix_for_an_empty_array() {
        assert_eq!(from_json("[]").unwrap(), Cells::new());
    }

    // ---- list interop

    fn fruit() -> Cells {
        cells(&[&["名称", "数量"], &["苹果", "3"], &["梨", "5"]])
    }

    #[test]
    fn writes_one_bullet_per_field_with_a_fullwidth_colon() {
        assert_eq!(to_list(&fruit()), "- 名称：苹果\n- 数量：3\n\n- 名称：梨\n- 数量：5");
    }

    #[test]
    fn round_trips_through_from_list_with_explicit_headers() {
        let headers = fruit()[0].clone();
        assert_eq!(from_list(&to_list(&fruit()), Some(&headers)), fruit());
    }

    #[test]
    fn discovers_headers_and_rows_when_none_are_supplied() {
        assert_eq!(from_list(&to_list(&fruit()), None), fruit());
    }

    #[test]
    fn ignores_non_list_lines() {
        assert_eq!(from_list("prose\n- a：1", Some(&["a".into()])), cells(&[&["a"], &["1"]]));
    }

    #[test]
    fn returns_an_empty_matrix_when_nothing_parses() {
        assert_eq!(from_list("no bullets here", None), Cells::new());
    }

    #[test]
    fn keeps_a_value_that_itself_contains_a_colon() {
        assert_eq!(from_list("- k：a：b", Some(&["k".into()])), cells(&[&["k"], &["a：b"]]));
    }

    // ---- normalizeRows

    #[test]
    fn rectangles_ragged_rows() {
        assert_eq!(normalize_rows(&cells(&[&["a"], &["b", "c"]])), cells(&[&["a", ""], &["b", "c"]]));
    }
}
