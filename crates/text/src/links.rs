//! Links as the renderer resolves them: inline, reference, autolinks and bare
//! URLs, with the renderer's escaping and protocol filter applied.

use std::ops::Range;
use std::sync::LazyLock;

use pulldown_cmark::{Event, LinkType, Tag, TagEnd, TextMergeWithOffset};
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub range: Range<usize>,
    /// `None` for images and for targets the renderer refuses (`javascript:` …).
    pub href: Option<String>,
    /// The `[label]` text of inline and reference links, without brackets.
    pub label: Option<Range<usize>>,
}

/// Every link and image outside front matter, in source order. An image
/// nested in a link label follows its link.
pub fn links(src: &str) -> Vec<Link> {
    let fm = crate::frontmatter_end(src);
    let mut out: Vec<Link> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut code = false;
    for (event, r) in TextMergeWithOffset::new(crate::parser(&src[fm..])) {
        let range = r.start + fm..r.end + fm;
        // Everything inside a link before its end event is label content.
        if !matches!(event, Event::End(TagEnd::Link | TagEnd::Image))
            && let Some(&i) = open.last()
            && let Some(label) = &mut out[i].label
        {
            label.end = label.end.max(range.end);
        }
        match event {
            Event::Start(Tag::Link { link_type, dest_url, .. }) => {
                // pulldown-cmark ends a collapsed reference `[label][]` before its `[]`.
                let mut range = range;
                if link_type == LinkType::Collapsed && src[range.end..].starts_with("[]") {
                    range.end += 2;
                }
                let href = match link_type {
                    LinkType::Email => format!("mailto:{dest_url}"),
                    _ => dest_url.into_string(),
                };
                let bracketed = !matches!(link_type, LinkType::Autolink | LinkType::Email);
                let label = bracketed.then_some(range.start + 1..range.start + 1);
                open.push(out.len());
                out.push(Link { range, href: normalize(&href), label });
            }
            Event::Start(Tag::Image { .. }) => {
                open.push(out.len());
                out.push(Link { range, href: None, label: None });
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                open.pop();
            }
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => code = false,
            Event::Text(_) if open.is_empty() && !code => {
                for (r, href) in bare_links(&src[range.clone()]) {
                    out.push(Link { range: range.start + r.start..range.start + r.end, href: normalize(&href), label: None });
                }
            }
            _ => {}
        }
    }
    out
}

/// Linkify: `https://…`, `www.…` and e-mail addresses in plain text, as
/// (range in `text`, href). The editor, link lookup and export all use this.
/// Chinese text sits right against a URL, so a URL may start after any
/// non-ASCII character and ends at the first one: `网站：https://a.cn。` links
/// `https://a.cn`. A browser copies a URL with its non-ASCII path percent-encoded.
pub fn bare_links(text: &str) -> Vec<(Range<usize>, String)> {
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        // `[!-;=-~]`: printable ASCII except `<`.
        Regex::new(r"(?i)(?:https?://|www\.)[!-;=-~]+|[a-z0-9.+_-]+@[a-z0-9-]+(?:\.[a-z0-9_-]+)+").unwrap()
    });
    let mut out = Vec::new();
    for m in URL.find_iter(text) {
        let mut url = m.as_str().trim_end_matches(['?', '!', '.', ',', ':', '*', '_', '~', '\'', '"']);
        // A closing parenthesis belongs to the URL only when it is balanced inside it.
        while url.ends_with(')') && url.matches(')').count() > url.matches('(').count() {
            url = url[..url.len() - 1].trim_end_matches(['?', '!', '.', ',', ':', '*', '_', '~']);
        }
        let lower = url.to_ascii_lowercase();
        let href = if ["http://", "https://", "www."].iter().any(|p| lower.starts_with(p)) {
            // GFM's left edge (line start, whitespace, `*_~(`), plus any non-ASCII character.
            if text[..m.start()].chars().next_back().is_some_and(|c| c.is_ascii() && !c.is_ascii_whitespace() && !"*_~(".contains(c)) {
                continue;
            }
            match lower.strip_prefix("www.") {
                Some(host) if !host.contains('.') => continue,
                Some(_) => format!("http://{url}"),
                None if url.ends_with("//") => continue,
                None => url.to_string(),
            }
        } else {
            format!("mailto:{url}")
        };
        out.push((m.start()..m.start() + url.len(), href));
    }
    out
}

/// markdown-it's `validateLink` + `normalizeLink`: refuse script-capable
/// protocols, then percent-encode what a URL may not contain literally.
fn normalize(url: &str) -> Option<String> {
    let lower = url.trim().to_lowercase();
    let bad = ["vbscript:", "javascript:", "file:", "data:"].iter().any(|p| lower.starts_with(p));
    let image = ["gif", "png", "jpeg", "webp"].iter().any(|t| lower.starts_with(&format!("data:image/{t};")));
    if bad && !image {
        return None;
    }
    let b = url.as_bytes();
    let mut out = String::with_capacity(url.len());
    for (i, &c) in b.iter().enumerate() {
        let escaped = c == b'%' && b.len() > i + 2 && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit();
        if c.is_ascii_alphanumeric() || b";/?:@&=+$,-_.!~*'()#".contains(&c) || escaped {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    Some(out)
}

/// The innermost link or image under the caret.
fn link_under(src: &str, offset: usize) -> Option<Link> {
    links(src).into_iter().filter(|l| l.range.contains(&offset)).last()
}

/// The href of the link under the caret, as the reading view would open it.
pub fn link_at(src: &str, offset: usize) -> Option<String> {
    link_under(src, offset)?.href
}

/// Remove the `[label](target)` around the caret, keeping the label:
/// `(replaced range, label text)`. Autolinks and bare URLs have no label.
pub fn unlink_at(src: &str, offset: usize) -> Option<(Range<usize>, &str)> {
    let link = link_under(src, offset)?;
    link.href?;
    Some((link.range, &src[link.label?]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(src: &str, label: &str) -> Option<String> {
        link_at(src, src.find(label).unwrap() + 1)
    }

    #[test]
    fn unlink_keeps_label_and_neighbors() {
        for link in ["[**read**](a_(b).md \"title\")", "[**read**][ref]", "[**read**][]", "[**read**]"] {
            let src = format!("before {link} after [other](other.md)\n\n[ref]: README.md\n[**read**]: README.md");
            let (range, text) = unlink_at(&src, src.find("read").unwrap() + 1).unwrap();
            assert_eq!(format!("{}{text}{}", &src[..range.start], &src[range.end..]), src.replacen(link, "**read**", 1));
        }
        assert!(unlink_at("https://example.com then [other](x.md)", 5).is_none());
        assert!(unlink_at("plain text then [other](x.md)", 5).is_none());
    }

    #[test]
    fn resolves() {
        let cases = [
            ("[English](README.md) · [简体中文](README.zh-CN.md)", "English", "README.md"),
            ("[English](README.md) · [简体中文](README.zh-CN.md)", "简体中文", "README.zh-CN.md"),
            ("[title](https://example.com/a_(b) \"description\")", "title", "https://example.com/a_(b)"),
            ("[文件](<./中文 文档.md>)", "文件", "./%E4%B8%AD%E6%96%87%20%E6%96%87%E6%A1%A3.md"),
            ("[**bold**](README.md)", "bold", "README.md"),
            ("[read][doc]\n\n[doc]: README.md \"title\"", "read", "README.md"),
            ("[read][]\n\n[read]: README.md", "read", "README.md"),
            ("[read]\n\n[read]: README.md", "read", "README.md"),
            ("<https://example.com>", "example", "https://example.com"),
            ("https://example.com", "example", "https://example.com"),
            ("www.example.com", "example", "http://www.example.com"),
            ("**https://example.com**", "example", "https://example.com"),
            ("<writer@example.com>", "writer", "mailto:writer@example.com"),
            ("[标题](#中文标题)", "标题", "#%E4%B8%AD%E6%96%87%E6%A0%87%E9%A2%98"),
            ("> - [link](README.md)", "link", "README.md"),
            ("| Doc |\n| --- |\n| [link](README.md) |", "link", "README.md"),
            ("see https://example.com/a_(b).", "example", "https://example.com/a_(b)"),
            ("网站：https://example.com/b", "example", "https://example.com/b"),
            ("访问https://example.com/c了解更多", "example", "https://example.com/c"),
            ("见 https://example.com/d。下一句", "example", "https://example.com/d"),
            ("邮件someone@example.com。", "someone", "mailto:someone@example.com"),
            ("httpd@example.com", "httpd", "mailto:httpd@example.com"),
        ];
        for (src, label, href) in cases {
            assert_eq!(at(src, label).as_deref(), Some(href), "{src}");
        }
    }

    #[test]
    fn does_not_invent_links() {
        for src in [
            "`[link](README.md)`",
            "```md\n[link](README.md)\n```",
            "    [link](README.md)",
            "![link](image.png)",
            "[link]\\(README.md)",
            "[link](javascript:alert(1))",
            "---\ntitle: [link](README.md)\n---",
            "[link]: README.md",
            "[link][missing]",
        ] {
            assert_eq!(at(src, "link"), None, "{src}");
        }
        for src in ["xhttps://link.com", "www.link", "https://"] {
            assert!(bare_links(src).is_empty(), "{src}");
        }
    }

    #[test]
    fn lists_links_for_decoration() {
        let src = "[**English**](README.md) · [中文][zh]\n\n[zh]: README.zh-CN.md\n\nhttps://example.com";
        let hrefs: Vec<_> = links(src).into_iter().filter_map(|l| l.href).collect();
        assert_eq!(hrefs, ["README.md", "README.zh-CN.md", "https://example.com"]);
    }
}
