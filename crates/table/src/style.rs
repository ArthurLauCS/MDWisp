//! Rendering styles. Plain markdown has nowhere to store how a table should
//! look, so the style rides along in an HTML comment on the line after the
//! block. `find_table_at` stops before it, which keeps it out of
//! `parse_table` and lets `serialize_table` round-trip the table without
//! touching it.

use crate::model::MARKER;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStyle {
    Zebra,
    Compact,
    Borderless,
    Card,
    Center,
}

impl TableStyle {
    pub const ALL: [TableStyle; 5] = [Self::Zebra, Self::Compact, Self::Borderless, Self::Card, Self::Center];

    pub fn name(self) -> &'static str {
        match self {
            Self::Zebra => "zebra",
            Self::Compact => "compact",
            Self::Borderless => "borderless",
            Self::Card => "card",
            Self::Center => "center",
        }
    }

    /// Visual weight is one slot: a row cannot be striped and a card at once.
    /// The rest are independent flags, so they compose freely.
    fn exclusive(self) -> bool {
        matches!(self, Self::Zebra | Self::Card)
    }
}

fn styles_from_marker(line: &str) -> Vec<TableStyle> {
    let Some(m) = MARKER.captures(line) else { return Vec::new() };
    let found: Vec<TableStyle> =
        m[1].split(',').filter_map(|s| TableStyle::ALL.into_iter().find(|t| t.name() == s.trim())).collect();
    // A hand-edited `style=zebra,card` keeps the first and drops the second,
    // so reading is as exclusive as writing.
    let visual = [TableStyle::Zebra, TableStyle::Card].into_iter().find(|s| found.contains(s));
    visual.into_iter().chain(found.into_iter().filter(|s| !s.exclusive())).collect()
}

/// Byte range of the marker line when the table carries one. `end` sits at
/// the end of the table's own last line, so the marker — if it exists — is
/// the line after that, never the first thing past `end`.
fn marker_span(src: &str, end: usize) -> Option<(usize, usize)> {
    let start = end + src[end..].find('\n')? + 1;
    let line_end = src[start..].find('\n').map_or(src.len(), |i| start + i);
    MARKER.is_match(&src[start..line_end]).then_some((start, line_end))
}

/// Offset just past the table block, taking a trailing marker line with it.
/// Deliberately not folded into `TableContext::end`: `end` marks where the
/// table's own text stops, and moving it would change `raw` for every
/// document that has a style.
pub fn table_region_end(src: &str, end: usize) -> usize {
    marker_span(src, end).map_or(end, |(_, e)| e)
}

/// The styles the table ending at `end` carries, empty when never styled.
pub fn table_styles(src: &str, end: usize) -> Vec<TableStyle> {
    marker_span(src, end).map_or_else(Vec::new, |(s, e)| styles_from_marker(&src[s..e]))
}

/// Apply or clear one style on the table ending at `end`, dropping the marker
/// line once no style is left.
pub fn set_table_style(src: &str, end: usize, style: TableStyle, on: bool) -> String {
    let span = marker_span(src, end);
    let present = span.map_or_else(Vec::new, |(s, e)| styles_from_marker(&src[s..e]));

    let next: Vec<TableStyle> = if on {
        // Choosing one visual weight replaces the other rather than sitting
        // beside it, so a marker can never read `style=zebra,card`.
        let mut next: Vec<_> = present.into_iter().filter(|s| !(style.exclusive() && s.exclusive())).collect();
        if !next.contains(&style) {
            next.push(style);
        }
        next
    } else {
        present.into_iter().filter(|&s| s != style).collect()
    };

    if next.is_empty() {
        // Dropping the last style takes the whole line with its line break, so
        // the blank line that followed the table is what remains.
        let Some((s, e)) = span else { return src.to_string() };
        let tail = &src[e..];
        return format!("{}{}", &src[..s], tail.strip_prefix('\n').unwrap_or(tail));
    }

    let names: Vec<&str> = next.iter().map(|s| s.name()).collect();
    let line = format!("<!-- mdview:table style={} -->", names.join(","));
    if let Some((s, e)) = span {
        return format!("{}{line}{}", &src[..s], &src[e..]);
    }
    // A table that ends the document has no line to insert before, so the
    // marker is appended; otherwise it takes the slot directly under the table
    // and whatever followed keeps its own line.
    match src[end..].find('\n') {
        None => format!("{src}\n{line}"),
        Some(i) => {
            let at = end + i + 1;
            format!("{}{line}\n{}", &src[..at], &src[at..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TableStyle::*;
    use super::*;
    use crate::model::tests::{cells, ctx_of};
    use crate::model::{TableContext, find_table_at, parse_table, replace_table};

    const DOC: &str = "# 标题\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n结尾";

    fn ctx(src: &str) -> TableContext {
        ctx_of(src, src.find("a | b").unwrap())
    }

    /// The document with `styles` applied, in order.
    fn styled(src: &str, styles: &[TableStyle]) -> String {
        styles.iter().fold(src.to_string(), |out, &s| set_table_style(&out, ctx(&out).end, s, true))
    }

    fn styles_of(src: &str) -> Vec<TableStyle> {
        table_styles(src, ctx(src).end)
    }

    // ---- tableStyles

    #[test]
    fn reports_nothing_for_a_plain_table() {
        assert_eq!(styles_of(DOC), []);
    }

    #[test]
    fn reads_a_marker_written_after_the_table() {
        assert_eq!(styles_of(&styled(DOC, &[Zebra])), [Zebra]);
    }

    #[test]
    fn composes_a_visual_style_with_the_flags() {
        assert_eq!(styles_of(&styled(DOC, &[Zebra, Compact, Center])), [Zebra, Compact, Center]);
    }

    #[test]
    fn keeps_only_the_first_of_two_hand_written_visual_styles() {
        let src = DOC.replace("| 1 | 2 |", "| 1 | 2 |\n<!-- mdview:table style=zebra,card -->");
        assert_eq!(styles_of(&src), [Zebra]);
    }

    #[test]
    fn ignores_an_unknown_style_name() {
        let src = DOC.replace("| 1 | 2 |", "| 1 | 2 |\n<!-- mdview:table style=sparkle -->");
        assert_eq!(styles_of(&src), []);
    }

    // ---- setTableStyle

    #[test]
    fn adds_a_marker_on_the_line_after_the_table() {
        assert_eq!(
            styled(DOC, &[Zebra]),
            "# 标题\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n<!-- mdview:table style=zebra -->\n\n结尾"
        );
    }

    #[test]
    fn replaces_the_visual_style_rather_than_accumulating_it() {
        let src = styled(DOC, &[Zebra, Card]);
        assert_eq!(styles_of(&src), [Card]);
        assert!(!src.contains("zebra"));
    }

    #[test]
    fn composes_flags_with_a_visual_style() {
        let src = styled(DOC, &[Card, Borderless, Compact, Center]);
        assert_eq!(styles_of(&src), [Card, Borderless, Compact, Center]);
    }

    #[test]
    fn cannot_produce_zebra_and_card_together() {
        let forwards = styled(DOC, &[Zebra, Card]);
        let backwards = styled(DOC, &[Card, Zebra]);
        assert_eq!(styles_of(&forwards), [Card]);
        assert_eq!(styles_of(&backwards), [Zebra]);
        assert!(!forwards.contains("zebra,"));
        assert!(!backwards.contains("card,"));
    }

    #[test]
    fn turns_a_style_off_and_drops_an_empty_marker() {
        let on = styled(DOC, &[Zebra]);
        assert_eq!(set_table_style(&on, ctx(&on).end, Zebra, false), DOC);
    }

    #[test]
    fn keeps_the_other_styles_when_one_is_turned_off() {
        let on = styled(DOC, &[Zebra, Compact]);
        let off = set_table_style(&on, ctx(&on).end, Zebra, false);
        assert_eq!(styles_of(&off), [Compact]);
    }

    #[test]
    fn is_idempotent_when_a_style_is_already_on() {
        let once = styled(DOC, &[Compact]);
        assert_eq!(set_table_style(&once, ctx(&once).end, Compact, true), once);
    }

    #[test]
    fn writes_the_marker_even_when_the_table_ends_the_document() {
        let tail = "| a | b |\n| --- | --- |\n| 1 | 2 |";
        let src = set_table_style(tail, ctx(tail).end, Zebra, true);
        assert_eq!(src, format!("{tail}\n<!-- mdview:table style=zebra -->"));
    }

    #[test]
    fn does_not_disturb_the_text_around_the_table() {
        let src = styled(DOC, &[Zebra]);
        assert!(src.starts_with("# 标题\n\n| a | b |"));
        assert!(src.ends_with("\n\n结尾"));
    }

    // ---- style marker round trip

    #[test]
    fn survives_find_table_at_with_the_table_unchanged() {
        let src = styled(DOC, &[Zebra, Compact]);
        let c = ctx(&src);
        assert_eq!(parse_table(&c), cells(&[&["a", "b"], &["1", "2"]]));
        assert_eq!(c.raw, "| a | b |\n| --- | --- |\n| 1 | 2 |");
        assert_eq!(table_styles(&src, c.end), [Zebra, Compact]);
    }

    #[test]
    fn never_lets_the_marker_become_a_body_row() {
        let one = "| a |\n| --- |\n| 1 |";
        let src = set_table_style(one, find_table_at(one, 0).unwrap().end, Zebra, true);
        let c = find_table_at(&src, 0).unwrap();
        assert_eq!(c.body_rows, 1);
        assert_eq!(parse_table(&c), cells(&[&["a"], &["1"]]));
    }

    #[test]
    fn serializes_back_byte_identical_after_an_unrelated_edit() {
        let src = styled(DOC, &[Zebra]);
        let c = ctx(&src);
        assert_eq!(replace_table(&src, &c, &parse_table(&c), &c.aligns, false, None).text, src);
    }

    #[test]
    fn leaves_a_document_with_no_marker_without_one() {
        let c = ctx(DOC);
        let text = replace_table(DOC, &c, &parse_table(&c), &c.aligns, false, None).text;
        assert_eq!(text, DOC);
        assert!(!text.contains("mdview:table"));
    }

    #[test]
    fn keeps_the_marker_when_a_row_is_edited() {
        let src = styled(DOC, &[Card]);
        let c = ctx(&src);
        let mut rows = parse_table(&c);
        rows[1][0] = "9".into();
        let text = replace_table(&src, &c, &rows, &c.aligns, false, None).text;
        assert!(text.contains("| 9 | 2 |"));
        assert_eq!(styles_of(&text), [Card]);
    }

    // ---- tableRegionEnd

    #[test]
    fn region_includes_a_trailing_marker_line() {
        let src = styled(DOC, &[Zebra]);
        let c = ctx(&src);
        assert_eq!(
            &src[c.start..table_region_end(&src, c.end)],
            "| a | b |\n| --- | --- |\n| 1 | 2 |\n<!-- mdview:table style=zebra -->"
        );
    }

    #[test]
    fn region_stops_at_the_table_when_there_is_no_marker() {
        let c = ctx(DOC);
        assert_eq!(table_region_end(DOC, c.end), c.end);
    }

    #[test]
    fn region_stops_at_the_table_when_the_next_line_is_ordinary_text() {
        let src = DOC.replace("\n\n结尾", "\n\n| not a marker |");
        let c = ctx(&src);
        assert_eq!(table_region_end(&src, c.end), c.end);
    }
}
