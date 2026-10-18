//! Markdown analysis shared by the editor, outline and actions.

use std::sync::LazyLock;

use pulldown_cmark::{DefaultBrokenLinkCallback, OffsetIter, Options, Parser};
use regex::Regex;

pub mod detect;
pub mod highlight;
pub mod history;
pub mod keymap;
pub mod links;
pub mod math;
pub mod ops;
pub mod outline;

/// The one set of syntax extensions. The editor, the outline and link lookup
/// all parse with these, so they agree on what a heading or a link is.
pub const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_SUPERSCRIPT)
    .union(Options::ENABLE_SUBSCRIPT)
    .union(Options::ENABLE_DEFINITION_LIST);

/// Events with UTF-8 byte ranges into `src`.
pub fn parser(src: &str) -> OffsetIter<'_, DefaultBrokenLinkCallback> {
    Parser::new_ext(src, OPTIONS).into_offset_iter()
}

/// Length of a leading YAML front matter block, 0 when there is none.
///
/// The parser does not know front matter: it reads the closing `---` as a
/// setext underline and turns the last metadata line into a heading.
pub fn frontmatter_end(src: &str) -> usize {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^---[ \t]*\r?\n(?:[\s\S]*?\r?\n)?---[ \t]*(?:\r?\n|$)").unwrap());
    RE.find(src).map_or(0, |m| m.end())
}

#[cfg(test)]
mod tests {
    use super::frontmatter_end;

    #[test]
    fn frontmatter() {
        assert_eq!(frontmatter_end("---\ntitle: a\n---\n# x"), 17);
        assert_eq!(frontmatter_end("---\n---"), 7);
        assert_eq!(frontmatter_end("# x\n---\n"), 0);
    }
}
