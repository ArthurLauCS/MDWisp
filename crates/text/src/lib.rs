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

/// The `==mark==`, `^sup^` or `~sub~` span starting at `s`, as the markdown-it
/// plugins read them: (html tag, content, byte length). pulldown-cmark has no
/// highlight syntax, and its sub/superscript refuse intraword use, so `H~2~O`
/// would stay literal; export and the editor both fill that in with this.
pub fn inline_span(s: &str) -> Option<(&'static str, &str, usize)> {
    if let Some(body) = s.strip_prefix("==") {
        let c = body.find("==")?;
        let content = &body[..c];
        let edge = |x: Option<char>| x.is_some_and(|x| !x.is_whitespace());
        return (edge(content.chars().next()) && edge(content.chars().last())).then_some(("mark", content, c + 4));
    }
    let (d, tag) = match s.as_bytes().first()? {
        b'^' => ('^', "sup"),
        b'~' => ('~', "sub"),
        _ => return None,
    };
    let body = &s[1..];
    if body.starts_with(d) {
        return None;
    }
    let c = body.find(d)?;
    let content = &body[..c];
    (c > 0 && !content.contains(char::is_whitespace) && !body[c + 1..].starts_with(d)).then_some((tag, content, c + 2))
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
