//! `$inline$` and `$$display$$` math detection, shared by export and the live
//! editor so both agree on what counts as a formula.

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

/// Pandoc's rule: the opener hugs its content and the closer is not followed
/// by a digit, so prices such as "$5 and $10" stay plain text. Returns the
/// byte index of the closing `$`.
pub fn inline_math_end(src: &str, start: usize) -> Option<usize> {
    let b = src.as_bytes();
    if b[start] != b'$' || src[start + 1..].chars().next().is_none_or(|c| c == '$' || c.is_whitespace()) {
        return None;
    }
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 1;
        } else if b[i] == b'$'
            && src[..i].chars().next_back().is_some_and(|c| !c.is_whitespace())
            && !b.get(i + 1).is_some_and(u8::is_ascii_digit)
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Every inline formula in `text`, as byte ranges including the dollars.
pub fn find_inline_math(text: &str, skip: impl Fn(usize) -> bool) -> Vec<Range<usize>> {
    let b = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 1;
        } else if b[i] == b'$' {
            match inline_math_end(text, i) {
                Some(end) => {
                    if !skip(i) {
                        found.push(i..end + 1);
                    }
                    i = end;
                }
                // Consume `$$` whole, so it never opens at its second dollar.
                None if b.get(i + 1) == Some(&b'$') => i += 1,
                None => {}
            }
        }
        i += 1;
    }
    found
}

/// The formula of a paragraph that is a `$$ … $$` block.
pub fn block_math(text: &str) -> Option<&str> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ {0,3}\$\$([\s\S]*?)\$\$[ \t]*$").unwrap());
    let tex = RE.captures(text)?.get(1)?.as_str().trim();
    (!tex.is_empty()).then_some(tex)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tex(text: &str) -> Vec<&str> {
        find_inline_math(text, |_| false).into_iter().map(|r| &text[r.start + 1..r.end - 1]).collect()
    }

    #[test]
    fn inline_formulas_skip_prices_and_escapes() {
        assert_eq!(tex("a $x^2$ b $y$"), ["x^2", "y"]);
        assert!(tex("costs $5 and $10 today").is_empty());
        assert!(tex("\\$x$ and $ x$").is_empty());
        assert_eq!(find_inline_math("$a\\$b$", |_| false), [0..6]);
        let skipped = find_inline_math("$x$ $y$", |at| at == 0);
        assert_eq!(skipped, [4..7]);
        assert!(tex("say $$x$$ inline").is_empty());
        assert_eq!(tex("中文 $数$ 后"), ["数"]);
    }

    #[test]
    fn display_block() {
        assert_eq!(block_math("$$\nE = mc^2\n$$"), Some("E = mc^2"));
        assert_eq!(block_math("$$a+b$$"), Some("a+b"));
        assert_eq!(block_math("$$ $$"), None);
        assert_eq!(block_math("$$a$$ and more"), None);
    }
}
