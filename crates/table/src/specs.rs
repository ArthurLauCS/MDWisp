//! Table actions, keyed by the action ids of the app's action registry.
//!
//! The registry is the single list of operations; a shortcut, a menu entry
//! and a toolbar button all name the same id, and `resolve` is the one place
//! where an id becomes an engine call.

use std::sync::LazyLock;

use mdwisp_core::i18n::{Locale, t};
use regex::Regex;

use crate::interop::{from_delimited, to_csv, to_json, to_list};
use crate::model::{
    Align, CellPos, Cells, TableContext, TableEdit, cell_at, escape_pipes, find_table_at, parse_table, replace_table,
    serialize_table,
};
use crate::ops::*;
use crate::style::{TableStyle, set_table_style, table_region_end, table_styles};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Row,
    Col,
    Align,
    Data,
    Cell,
    Struct,
}

pub struct Spec {
    pub id: &'static str,
    /// Chinese source string; show it through `t`.
    pub label: &'static str,
    pub group: Group,
    /// Restructures the table, so the toolbar marks the band it touches first.
    pub structural: bool,
}

/// Toolbar group labels, in toolbar order.
pub const GROUPS: [(Group, &str); 6] = [
    (Group::Row, "行"),
    (Group::Col, "列"),
    (Group::Align, "对齐"),
    (Group::Data, "数据"),
    (Group::Cell, "单元格"),
    (Group::Struct, "结构"),
];

const fn spec(id: &'static str, label: &'static str, group: Group, structural: bool) -> Spec {
    Spec { id, label, group, structural }
}

pub const SPECS: &[Spec] = &[
    spec("table.row.insertAbove", "上行", Group::Row, true),
    spec("table.row.insertBelow", "下行", Group::Row, true),
    spec("table.row.delete", "删行", Group::Row, true),
    spec("table.row.moveUp", "上移", Group::Row, true),
    spec("table.row.moveDown", "下移", Group::Row, true),
    spec("table.row.copy", "复制行", Group::Row, false),
    spec("table.row.cut", "剪切行", Group::Row, true),
    spec("table.col.insertLeft", "左列", Group::Col, true),
    spec("table.col.insertRight", "右列", Group::Col, true),
    spec("table.col.delete", "删列", Group::Col, true),
    spec("table.col.moveLeft", "左移", Group::Col, true),
    spec("table.col.moveRight", "右移", Group::Col, true),
    spec("table.col.alignLeft", "左", Group::Align, false),
    spec("table.col.alignCenter", "中", Group::Align, false),
    spec("table.col.alignRight", "右", Group::Align, false),
    spec("table.sort.asc", "升序", Group::Data, true),
    spec("table.sort.desc", "降序", Group::Data, true),
    spec("table.stats.sum", "求和", Group::Data, true),
    spec("table.stats.avg", "均值", Group::Data, true),
    spec("table.stats.count", "计数", Group::Data, true),
    spec("table.stats.min", "最小", Group::Data, true),
    spec("table.stats.max", "最大", Group::Data, true),
    spec("table.select.all", "选中表格", Group::Data, false),
    spec("table.paste.fromClipboard", "粘贴建表", Group::Data, false),
    spec("table.paste.fromExcel", "从 Excel", Group::Data, false),
    spec("table.cell.merge", "右合并", Group::Cell, true),
    spec("table.cell.mergeDown", "下合并", Group::Cell, true),
    spec("table.cell.split", "拆分", Group::Cell, true),
    spec("table.cell.clear", "清空", Group::Cell, false),
    spec("table.cell.newline", "换行", Group::Cell, false),
    spec("table.style.pad", "美化", Group::Struct, false),
    spec("table.struct.transpose", "转置", Group::Struct, true),
    spec("table.struct.headerOn", "设表头", Group::Struct, true),
    spec("table.struct.headerOff", "取消表头", Group::Struct, true),
    spec("table.struct.toCsv", "转 CSV", Group::Struct, false),
    spec("table.struct.fromCsv", "CSV 转表", Group::Struct, false),
    spec("table.struct.toJson", "转 JSON", Group::Struct, false),
    spec("table.struct.toList", "转列表", Group::Struct, false),
    spec("table.struct.delete", "删除表格", Group::Struct, true),
    spec("table.style.zebra", "斑马纹", Group::Struct, false),
    spec("table.style.compact", "紧凑", Group::Struct, false),
    spec("table.style.borderless", "无边框", Group::Struct, false),
    spec("table.style.card", "卡片", Group::Struct, false),
    spec("table.style.center", "居中", Group::Struct, false),
];

pub struct Input<'a> {
    pub cells: &'a [Vec<String>],
    pub aligns: &'a [Align],
    /// Matrix position of the caret; row 0 is the header.
    pub cell: Option<CellPos>,
    pub has_header: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Effect {
    /// The whole new document and caret.
    pub edit: Option<TableEdit>,
    /// Text to put on the clipboard.
    pub clipboard: Option<String>,
    /// Range to select instead of editing.
    pub select: Option<(usize, usize)>,
}

static FENCE_BELOW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^\n(?:```|~~~)([0-9A-Za-z_]*)\n(.*?)\n(?:```|~~~)").unwrap());

/// Whether the first row is a real header. The matrix always has a header
/// row, so a header and a body row are distinguishable only by content: an
/// all-blank first row is what turning the header off leaves behind.
pub fn has_header(cells: &[Vec<String>]) -> bool {
    cells.len() > 1 && cells[0].iter().any(|c| !c.is_empty())
}

/// The clipboard text `resolve` needs for this action, read by the caller
/// first because reading it is asynchronous.
pub fn needs_clipboard(id: &str) -> bool {
    matches!(id, "table.paste.fromClipboard" | "table.paste.fromExcel")
}

/// Why the action cannot run here, translated; None when it can.
pub fn disabled_reason(id: &str, input: &Input) -> Option<&'static str> {
    let at = input.cell.unwrap_or_default();
    let last_row = at.row + 1 >= input.cells.len();
    let reason = match id {
        "table.row.delete" | "table.row.cut" if input.cells.len() <= 1 => "至少保留一行",
        "table.row.moveUp" if at.row == 0 => "表头不能上移",
        "table.row.moveDown" if last_row => "已在最后一行",
        "table.col.delete" if input.aligns.len() <= 1 => "至少保留一列",
        "table.col.moveLeft" if at.col == 0 => "已是第一列",
        "table.col.moveRight" if at.col + 1 >= input.aligns.len() => "已是最后一列",
        "table.cell.merge" if input.aligns.len() < 2 => "至少需要两列",
        "table.cell.mergeDown" if last_row => "下方没有可合并的行",
        "table.cell.split"
            if !input.cells.get(at.row).and_then(|r| r.get(at.col)).is_some_and(|v| is_merged(v)) =>
        {
            "该单元格没有合并"
        }
        "table.struct.headerOn" if input.has_header => "已有表头",
        "table.struct.headerOff" if !input.has_header => "没有表头",
        _ => return None,
    };
    Some(t(reason))
}

/// Matrix from clipboard text. `from_delimited` prefers the HTML table when
/// there is one — Excel puts the grid in the HTML flavour and a coarser
/// tab-separated copy in the text flavour.
fn cells_from_text(text: &str) -> Cells {
    from_delimited(text, None)
        .into_iter()
        .map(|row| row.iter().map(|c| escape_pipes(c).replace("\r\n", "<br>").replace(['\r', '\n'], "<br>")).collect())
        .collect()
}

/// `src` with `[ctx.start, end)` replaced by `text`.
fn splice(src: &str, ctx: &TableContext, end: usize, text: &str, cursor: usize) -> TableEdit {
    TableEdit { text: format!("{}{text}{}", &src[..ctx.start], &src[end..]), cursor }
}

/// The action `id` run against the table under the caret. None when the
/// caret is not in a table, the id is unknown, or the action is disabled
/// here — the three cases a menu greys out identically.
pub fn resolve(id: &str, src: &str, cursor: usize, clipboard: &str, language: Locale) -> Option<Effect> {
    let ctx = find_table_at(src, cursor)?;
    let cells = parse_table(&ctx);
    let cell = cell_at(&ctx, cursor);
    let input = Input { cells: &cells, aligns: &ctx.aligns, cell, has_header: true };
    if disabled_reason(id, &input).is_some() {
        return None;
    }
    let at = cell.unwrap_or_default();
    let matrix = |next: Cells| Some(replace_table(src, &ctx, &next, &ctx.aligns, false, Some(cursor)));
    let aligned = |a: Align| {
        Some(replace_table(src, &ctx, &cells, &set_column_align(&ctx.aligns, at.col, a), false, Some(cursor)))
    };
    let stats = |kind| matrix(append_stats_row(&cells, at.col, kind, language));
    let block = |text: &str| Some(splice(src, &ctx, ctx.end, text, ctx.start));
    let fence = |lang: &str, body: String| block(&format!("```{lang}\n{body}\n```"));
    let paste = || {
        let next = cells_from_text(clipboard);
        if next.is_empty() {
            return None;
        }
        let text = serialize_table(&next, &ctx.aligns, false);
        Some(splice(src, &ctx, ctx.end, &text, ctx.start + text.len()))
    };
    // Toggle one rendering style; the engine enforces the exclusive ones.
    let style = |s: TableStyle| {
        let on = !table_styles(src, ctx.end).contains(&s);
        let text = set_table_style(src, ctx.end, s, on);
        // Measured after the write: adding or dropping a marker moves the end.
        let cursor = table_region_end(&text, ctx.end);
        Some(TableEdit { text, cursor })
    };

    let mut effect = Effect::default();
    effect.edit = match id {
        "table.row.insertAbove" => matrix(insert_row(&cells, at.row, true)),
        "table.row.insertBelow" => matrix(insert_row(&cells, at.row, false)),
        "table.row.delete" => matrix(delete_row(&cells, at.row)),
        "table.row.moveUp" => matrix(move_row(&cells, at.row, at.row - 1)),
        "table.row.moveDown" => matrix(move_row(&cells, at.row, at.row + 1)),
        "table.row.copy" => {
            effect.clipboard = Some(row_as_table(&cells, at.row, &ctx.aligns));
            None
        }
        "table.row.cut" => {
            let (markdown, rest) = cut_row(&cells, at.row, &ctx.aligns);
            effect.clipboard = Some(markdown);
            matrix(rest)
        }
        "table.col.insertLeft" => matrix(insert_col(&cells, at.col, true)),
        "table.col.insertRight" => matrix(insert_col(&cells, at.col, false)),
        "table.col.delete" => matrix(delete_col(&cells, at.col)),
        "table.col.moveLeft" => matrix(move_col(&cells, at.col, at.col - 1)),
        "table.col.moveRight" => matrix(move_col(&cells, at.col, at.col + 1)),
        "table.col.alignLeft" => aligned(Align::Left),
        "table.col.alignCenter" => aligned(Align::Center),
        "table.col.alignRight" => aligned(Align::Right),
        "table.sort.asc" => matrix(sort_by_column(&cells, at.col, SortDir::Asc, true)),
        "table.sort.desc" => matrix(sort_by_column(&cells, at.col, SortDir::Desc, true)),
        "table.stats.sum" => stats(StatsKind::Sum),
        "table.stats.avg" => stats(StatsKind::Avg),
        "table.stats.count" => stats(StatsKind::Count),
        "table.stats.min" => stats(StatsKind::Min),
        "table.stats.max" => stats(StatsKind::Max),
        "table.select.all" => {
            // The marker line is part of the table region, so a copy of the
            // selection carries the style with it.
            effect.select = Some((ctx.start, table_region_end(src, ctx.end)));
            None
        }
        "table.paste.fromClipboard" => Some(paste()?),
        // Excel always puts a real `<table>` on the clipboard; without one this
        // is a plain-text paste and the other action is the right one.
        "table.paste.fromExcel" if clipboard.to_ascii_lowercase().contains("<table") => Some(paste()?),
        "table.paste.fromExcel" => return None,
        "table.cell.merge" => matrix(merge_right(&cells, at, 2)),
        "table.cell.mergeDown" => matrix(merge_down(&cells, at, 2)),
        "table.cell.split" => matrix(split_cell(&cells, at)),
        "table.cell.clear" => matrix(clear_cell(&cells, at)),
        "table.cell.newline" => matrix(toggle_cell_newline(&cells, at)),
        "table.style.pad" => Some(replace_table(src, &ctx, &cells, &ctx.aligns, true, Some(cursor))),
        "table.struct.transpose" => matrix(transpose(&cells)),
        "table.struct.headerOn" => matrix(set_header_row(&cells, true)),
        "table.struct.headerOff" => matrix(set_header_row(&cells, false)),
        "table.struct.toCsv" => fence("csv", to_csv(&cells, ',')),
        "table.struct.toJson" => fence("json", to_json(&cells)),
        "table.struct.toList" => block(&to_list(&cells)),
        "table.struct.fromCsv" => {
            // Converts the csv fence directly under the table, which the
            // table and the fence are both replaced by.
            let m = FENCE_BELOW.captures(&src[ctx.end..]).filter(|m| &m[1] == "csv")?;
            let next = from_delimited(&m[2], Some(','));
            if next.is_empty() {
                return None;
            }
            let end = ctx.end + m.get(0).unwrap().end();
            Some(splice(src, &ctx, end, &serialize_table(&next, &ctx.aligns, false), ctx.start))
        }
        // Takes the style marker with it, so no marker is left styling nothing.
        "table.struct.delete" => Some(splice(src, &ctx, table_region_end(src, ctx.end), "", ctx.start)),
        "table.style.zebra" => style(TableStyle::Zebra),
        "table.style.compact" => style(TableStyle::Compact),
        "table.style.borderless" => style(TableStyle::Borderless),
        "table.style.card" => style(TableStyle::Card),
        "table.style.center" => style(TableStyle::Center),
        _ => return None,
    };
    Some(effect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::cells;

    const DOC: &str = "前\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |\n\n后";

    fn run(id: &str, needle: &str) -> Option<Effect> {
        resolve(id, DOC, DOC.find(needle).unwrap(), "", Locale::ZhCn)
    }

    fn text(id: &str, needle: &str) -> String {
        run(id, needle).unwrap().edit.unwrap().text
    }

    #[test]
    fn every_spec_resolves_or_is_disabled_without_panicking() {
        for s in SPECS {
            for needle in ["a |", "| 1", "4 |"] {
                resolve(s.id, DOC, DOC.find(needle).unwrap(), "x,y", Locale::ZhCn);
            }
        }
    }

    #[test]
    fn outside_a_table_or_with_an_unknown_id_nothing_runs() {
        assert_eq!(resolve("table.row.delete", DOC, 0, "", Locale::ZhCn), None);
        assert_eq!(run("table.nope", "| 1"), None);
    }

    #[test]
    fn row_ops_act_on_the_caret_row() {
        assert_eq!(text("table.row.moveDown", "| 1"), "前\n\n| a | b |\n| --- | --- |\n| 3 | 4 |\n| 1 | 2 |\n\n后");
        assert_eq!(run("table.row.moveUp", "a |"), None);
        assert_eq!(run("table.row.moveDown", "| 3"), None);
    }

    #[test]
    fn copy_only_fills_the_clipboard_and_cut_also_edits() {
        let copy = run("table.row.copy", "| 3").unwrap();
        assert_eq!(copy.clipboard.as_deref(), Some("| a | b |\n| --- | --- |\n| 3 | 4 |"));
        assert_eq!(copy.edit, None);
        let cut = run("table.row.cut", "| 3").unwrap();
        assert!(!cut.edit.unwrap().text.contains("| 3 | 4 |"));
    }

    #[test]
    fn align_rewrites_only_the_delimiter_row() {
        assert!(text("table.col.alignRight", "2 |").contains("| --- | --: |"));
    }

    #[test]
    fn stats_use_the_requested_language() {
        let en = resolve("table.stats.sum", DOC, DOC.find("2 |").unwrap(), "", Locale::En).unwrap();
        assert!(en.edit.unwrap().text.contains("| Sum | 6 |"));
    }

    #[test]
    fn select_covers_the_table_and_its_marker() {
        let src = format!("{}\n<!-- mdview:table style=zebra -->", &DOC[..DOC.find("\n\n后").unwrap()]);
        let effect = resolve("table.select.all", &src, src.find("| 1").unwrap(), "", Locale::ZhCn).unwrap();
        let (s, e) = effect.select.unwrap();
        assert!(src[s..e].starts_with("| a") && src[s..e].ends_with("-->"));
        assert_eq!(effect.edit, None);
    }

    #[test]
    fn paste_replaces_only_the_table_block() {
        let effect = resolve("table.paste.fromClipboard", DOC, DOC.find("| 1").unwrap(), "x|y\tz", Locale::ZhCn);
        let edit = effect.unwrap().edit.unwrap();
        assert_eq!(edit.text, "前\n\n| x\\|y | z |\n| --- | --- |\n\n后");
        assert_eq!(&edit.text[edit.cursor..], "\n\n后");
        assert_eq!(resolve("table.paste.fromExcel", DOC, DOC.find("| 1").unwrap(), "a,b", Locale::ZhCn), None);
    }

    #[test]
    fn conversions_replace_only_the_table_block() {
        assert_eq!(text("table.struct.toCsv", "| 1"), "前\n\n```csv\na,b\n1,2\n3,4\n```\n\n后");
        assert_eq!(text("table.struct.toList", "| 1"), "前\n\n- a：1\n- b：2\n\n- a：3\n- b：4\n\n后");
        assert_eq!(text("table.struct.delete", "| 1"), "前\n\n\n\n后");
    }

    #[test]
    fn from_csv_replaces_the_table_and_the_fence_below_it() {
        let src = "| a |\n| --- |\n```csv\nx,y\n1,2\n```\nend";
        let edit = resolve("table.struct.fromCsv", src, 0, "", Locale::ZhCn).unwrap().edit.unwrap();
        assert_eq!(edit.text, "| x | y |\n| --- | --- |\n| 1 | 2 |\nend");
    }

    #[test]
    fn style_toggles_the_marker() {
        let on = text("table.style.card", "| 1");
        assert!(on.contains("| 3 | 4 |\n<!-- mdview:table style=card -->\n"));
        let off = resolve("table.style.card", &on, on.find("| 1").unwrap(), "", Locale::ZhCn).unwrap();
        assert_eq!(off.edit.unwrap().text, DOC);
    }

    #[test]
    fn split_is_disabled_on_a_plain_cell() {
        let c = cells(&[&["a", "b"], &["1", "2"]]);
        let input = Input { cells: &c, aligns: &[Align::None, Align::None], cell: None, has_header: true };
        assert!(disabled_reason("table.cell.split", &input).is_some());
        assert!(disabled_reason("table.struct.headerOn", &input).is_some());
        assert!(disabled_reason("table.struct.headerOff", &input).is_none());
        assert!(has_header(&c));
        assert!(!has_header(&cells(&[&["", ""], &["1", "2"]])));
    }
}
