//! Document outline: headings as the editor's parser sees them.

use std::collections::HashSet;
use std::sync::LazyLock;

use pulldown_cmark::{Event, Tag};
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// 1-based line number in the source.
    pub line: usize,
    /// Byte offset of the heading's first character.
    pub offset: usize,
    /// Unique outline identity; duplicate titles remain separately collapsible.
    pub id: String,
    /// Actual ancestor count, including documents that skip heading levels.
    pub depth: usize,
}

/// The anchor slug the renderer gives a heading: `encodeURIComponent` of the
/// lowercased letters, digits, `-` and `_`.
pub fn slugify(text: &str) -> String {
    static DROP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\-_]").unwrap());
    let joined = text.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-");
    let mut out = String::new();
    for b in DROP.replace_all(&joined, "").bytes() {
        if b.is_ascii() {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn extract_headings(src: &str) -> Vec<Heading> {
    static ATX_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{1,6}[ \t]*").unwrap());
    static ATX_CLOSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+#+[ \t]*$").unwrap());
    let body = crate::frontmatter_end(src);
    let mut out: Vec<Heading> = Vec::new();
    let mut ancestors: Vec<u8> = Vec::new();
    let mut ids = HashSet::new();
    let mut line = 1;
    let mut previous = 0;
    for (event, range) in crate::parser(src) {
        let Event::Start(Tag::Heading { level, .. }) = event else { continue };
        let raw = src[range].trim_start_matches([' ', '\t']).trim_end_matches(['\r', '\n']);
        let offset = raw.as_ptr() as usize - src.as_ptr() as usize;
        if offset < body {
            continue;
        }
        let label = if raw.starts_with('#') {
            ATX_CLOSE.replace(&ATX_OPEN.replace(raw, ""), "").into_owned()
        } else {
            raw[..raw.rfind('\n').unwrap_or(0)].to_string()
        };
        let text = strip_inline(&label).split_whitespace().collect::<Vec<_>>().join(" ");
        let base = slugify(if text.is_empty() { "未命名标题" } else { &text });
        let mut id = base.clone();
        for suffix in 1.. {
            if ids.insert(id.clone()) {
                break;
            }
            id = format!("{base}-{suffix}");
        }
        let level = level as u8;
        while ancestors.last().is_some_and(|a| *a >= level) {
            ancestors.pop();
        }
        line += src[previous..offset].matches('\n').count();
        previous = offset;
        out.push(Heading { level, text, line, offset, id, depth: ancestors.len() });
        ancestors.push(level);
    }
    out
}

/// `## **粗体** 标题` should read as `粗体 标题` in the outline.
fn strip_inline(text: &str) -> String {
    static RULES: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        [
            (r"!\[([^\]]*)\]\([^)]*\)", "$1"),
            (r"\[([^\]]*)\]\([^)]*\)", "$1"),
            (r"`([^`]*)`", "$1"),
            (r"\*\*(.*?)\*\*|__(.*?)__", "$1$2"),
            (r"\*(.*?)\*|_(.*?)_", "$1$2"),
            (r"~~(.*?)~~", "$1"),
            (r"==(.*?)==", "$1"),
            (r"<[^>]+>", ""),
        ]
        .into_iter()
        .map(|(re, to)| (Regex::new(re).unwrap(), to))
        .collect()
    });
    let mut s = text.to_string();
    for (re, to) in RULES.iter() {
        s = re.replace_all(&s, *to).into_owned();
    }
    s.trim().to_string()
}

/// The heading containing an offset — the section a caret sits in.
pub fn heading_at(headings: &[Heading], offset: usize) -> Option<&Heading> {
    headings.iter().take_while(|h| h.offset <= offset).last()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<String> {
        extract_headings(src).into_iter().map(|h| h.text).collect()
    }

    #[test]
    fn front_matter_is_not_a_setext_heading() {
        let h = extract_headings("---\ntitle: 欢迎\n---\n\n# 正文\n");
        assert_eq!(h.iter().map(|h| (h.text.as_str(), h.line)).collect::<Vec<_>>(), [("正文", 5)]);
    }

    #[test]
    fn atx_with_line_numbers() {
        let h = extract_headings("# 一\n正文\n## 二\n### 三");
        assert_eq!(
            h.iter().map(|h| (h.level, h.text.as_str(), h.line)).collect::<Vec<_>>(),
            [(1, "一", 1), (2, "二", 3), (3, "三", 4)]
        );
    }

    #[test]
    fn fences_hide_headings() {
        assert_eq!(texts("# 真标题\n\n```\n# 这是代码注释\n```\n\n## 另一个真标题"), ["真标题", "另一个真标题"]);
        assert_eq!(texts("~~~\n# 注释\n~~~\n# 真标题"), ["真标题"]);
        assert_eq!(texts("```\n# 注释\n```\n# 之后"), ["之后"]);
        assert_eq!(texts("````md\n```\n# 示例\n```\n````\n\n# 正文"), ["正文"]);
        assert_eq!(texts("    # 代码\n\n<div>\n# HTML 内容\n</div>\n\n# 正文"), ["正文"]);
    }

    #[test]
    fn setext() {
        let h = extract_headings("一级标题\n===\n二级标题\n---");
        assert_eq!(h.iter().map(|h| (h.level, h.text.as_str())).collect::<Vec<_>>(), [(1, "一级标题"), (2, "二级标题")]);
        assert!(extract_headings("正文\n\n---\n\n更多正文").is_empty());
        let single = &extract_headings("正文\n-")[0];
        assert_eq!((single.text.as_str(), single.level), ("正文", 2));
    }

    #[test]
    fn labels() {
        assert_eq!(texts("## **加粗** 与 `代码` 与 [链接](x.md)"), ["加粗 与 代码 与 链接"]);
        assert_eq!(texts("## ~~删除~~ ==高亮=="), ["删除 高亮"]);
        assert_eq!(texts("## 标题 ##"), ["标题"]);
        assert!(texts("#没有空格").is_empty());
        assert!(texts("####### 七级").is_empty());
        assert!(texts("只有正文，没有任何标题。").is_empty());
        assert!(texts("").is_empty());
    }

    #[test]
    fn offsets_and_depth() {
        let src = "前言\n# 标题";
        assert_eq!(&src[extract_headings(src)[0].offset..][..2], "# ");
        let depths = |src: &str| extract_headings(src).into_iter().map(|h| h.depth).collect::<Vec<_>>();
        assert_eq!(depths("## 甲\n### 乙\n#### 丙"), [0, 1, 2]);
        assert_eq!(depths("## 开始\n#### 细节\n### 旁支\n# 新章\n###### 跳级"), [0, 1, 1, 0, 1]);
    }

    #[test]
    fn repeated_headings_stay_distinct() {
        let h = extract_headings("# 重复\n## 重复\n# 重复");
        let mut ids: Vec<_> = h.iter().map(|h| &h.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), 3);
        // TS offsets 0, 5, 11 in UTF-16; `重复` is 6 bytes in UTF-8.
        assert_eq!(h.iter().map(|h| h.offset).collect::<Vec<_>>(), [0, 9, 19]);
    }

    #[test]
    fn indented_and_crlf() {
        let src = "前言\r\n\r\n  ## C#\r\n\r\n### 标题###\r\n";
        let h = extract_headings(src);
        assert_eq!(h.iter().map(|h| (h.text.as_str(), h.line)).collect::<Vec<_>>(), [("C#", 3), ("标题###", 5)]);
        assert_eq!(&src[h[0].offset..][..2], "##");
    }

    #[test]
    fn slugs() {
        assert_eq!(extract_headings("## 排版层级")[0].id, slugify("排版层级"));
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("中 文!"), "%E4%B8%AD-%E6%96%87");
    }

    #[test]
    fn section_at_offset() {
        let src = "# 甲\na\n## 乙\nb\n## 丙\nc";
        let h = extract_headings(src);
        assert_eq!(heading_at(&h, src.find('b').unwrap()).unwrap().text, "乙");
        assert_eq!(heading_at(&h, src.len() - 1).unwrap().text, "丙");
        let pre = extract_headings("前言\n# 甲");
        assert!(heading_at(&pre, 0).is_none());
        assert_eq!(heading_at(&pre, pre[0].offset).unwrap().text, "甲");
        assert!(heading_at(&[], 100).is_none());
    }
}
