//! Markdown → HTML for export, shaped like the markdown-it output the app's
//! stylesheet was written against: heading anchors, footnote section, task
//! list labels, `==mark==`, linkified bare URLs, the code-block frame, and math as MathML so the
//! exported file needs no script and no font download.

use std::collections::{HashMap, HashSet};

use math_core::{LatexToMathML, MathDisplay};
use mdwisp_core::i18n::t;
use mdwisp_text::{inline_span, links};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, TextMergeStream, html};

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub fn render_markdown(source: &str) -> String {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_MATH
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let events: Vec<Event> = TextMergeStream::new(Parser::new_ext(source, opts)).collect();
    let math = LatexToMathML::default();

    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    let mut slugs = HashSet::new();
    let mut notes: HashMap<CowStr, Vec<Event>> = HashMap::new();
    let mut note_order: Vec<CowStr> = Vec::new();
    let mut note_refs: HashMap<CowStr, usize> = HashMap::new();
    let mut note_buf: Option<(CowStr, Vec<Event>)> = None;
    let mut label_open = false;
    // Alt text is written as an attribute; markup inside it would be escaped into view.
    let mut in_image = false;
    let mut in_link = false;
    let mut in_code = false;

    let mut i = 0;
    while i < events.len() {
        let ev = events[i].clone();
        i += 1;
        let sink = match &mut note_buf {
            Some((_, buf)) => buf,
            None => &mut out,
        };

        if label_open && matches!(ev, Event::End(TagEnd::Paragraph | TagEnd::Item) | Event::Start(Tag::List(_))) {
            sink.push(Event::InlineHtml("</label>".into()));
            label_open = false;
        }

        match ev {
            Event::Start(Tag::MetadataBlock(_)) => {
                while !matches!(events[i], Event::End(TagEnd::MetadataBlock(_))) {
                    i += 1;
                }
                i += 1;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let mut text = String::new();
                for e in &events[i..] {
                    match e {
                        Event::End(TagEnd::Heading(_)) => break,
                        Event::Text(s) | Event::Code(s) => text.push_str(s),
                        _ => {}
                    }
                }
                let slug = unique(slugify(&text), &mut slugs);
                sink.push(Event::Html(
                    format!(r##"<{level} id="{slug}" tabindex="-1"><a class="heading-anchor" href="#{slug}" aria-hidden="true">#</a> "##).into(),
                ));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let mut code = String::new();
                while let Event::Text(s) = &events[i] {
                    code.push_str(s);
                    i += 1;
                }
                i += 1; // End(CodeBlock)
                let lang = info.split_whitespace().next().unwrap_or("");
                sink.push(Event::Html(code_block(lang, &code).into()));
            }
            Event::Start(Tag::Item)
                if matches!(events.get(i), Some(Event::TaskListMarker(_)))
                    || matches!(events.get(i..i + 2), Some([Event::Start(Tag::Paragraph), Event::TaskListMarker(_)])) =>
            {
                sink.push(Event::Html("<li class=\"task-list-item\">".into()));
            }
            Event::TaskListMarker(checked) => {
                let checked = if checked { " checked=\"\"" } else { "" };
                sink.push(Event::InlineHtml(
                    format!(r#"<label><input class="task-list-item-checkbox"{checked} disabled="" type="checkbox"> "#).into(),
                ));
                label_open = true;
            }
            Event::FootnoteReference(label) => {
                if !note_order.contains(&label) {
                    note_order.push(label.clone());
                }
                let n = note_order.iter().position(|l| *l == label).unwrap() + 1;
                let seen = note_refs.entry(label).or_insert(0);
                let id = if *seen == 0 { format!("fnref{n}") } else { format!("fnref{n}:{seen}") };
                *seen += 1;
                sink.push(Event::InlineHtml(
                    format!(r##"<sup class="footnote-ref"><a href="#fn{n}" id="{id}">[{n}]</a></sup>"##).into(),
                ));
            }
            Event::Start(Tag::FootnoteDefinition(label)) => note_buf = Some((label, Vec::new())),
            Event::End(TagEnd::FootnoteDefinition) => {
                let (label, buf) = note_buf.take().unwrap();
                notes.insert(label, buf);
            }
            // A paragraph that is only a display formula would leave an empty `<p>` around a block.
            Event::Start(Tag::Paragraph)
                if matches!(events.get(i..i + 2), Some([Event::DisplayMath(_), Event::End(TagEnd::Paragraph)])) =>
            {
                if let Event::DisplayMath(tex) = &events[i] {
                    sink.push(Event::Html(display_math(&math, tex).into()));
                }
                i += 2;
            }
            Event::DisplayMath(tex) => sink.push(Event::Html(display_math(&math, &tex).into())),
            Event::InlineMath(tex) => sink.push(Event::InlineHtml(render_math(&math, &tex, MathDisplay::Inline).into())),
            Event::Text(text) if !in_image && !in_code => push_text(&text, !in_link, sink),
            Event::Start(Tag::Image { .. }) => {
                in_image = true;
                sink.push(ev);
            }
            Event::End(TagEnd::Image) => {
                in_image = false;
                sink.push(ev);
            }
            Event::Start(Tag::Link { .. }) | Event::End(TagEnd::Link) => {
                in_link = matches!(ev, Event::Start(_));
                sink.push(ev);
            }
            // Fenced blocks are taken whole above; this is an indented one.
            Event::Start(Tag::CodeBlock(_)) | Event::End(TagEnd::CodeBlock) => {
                in_code = matches!(ev, Event::Start(_));
                sink.push(ev);
            }
            ev => sink.push(ev),
        }
    }

    let mut body = String::new();
    html::push_html(&mut body, out.into_iter());
    if note_order.iter().any(|l| notes.contains_key(l)) {
        body.push_str("<hr class=\"footnotes-sep\">\n<section class=\"footnotes\">\n<ol class=\"footnotes-list\">\n");
        for (k, label) in note_order.iter().enumerate() {
            let Some(mut events) = notes.remove(label) else { continue };
            let n = k + 1;
            let backrefs: String = (0..note_refs[label])
                .map(|r| {
                    let id = if r == 0 { format!("fnref{n}") } else { format!("fnref{n}:{r}") };
                    format!(r##" <a href="#{id}" class="footnote-backref">↩︎</a>"##)
                })
                .collect();
            let at = if matches!(events.last(), Some(Event::End(TagEnd::Paragraph))) { events.len() - 1 } else { events.len() };
            events.insert(at, Event::InlineHtml(backrefs.into()));
            body.push_str(&format!("<li id=\"fn{n}\" class=\"footnote-item\">"));
            html::push_html(&mut body, events.into_iter());
            body.push_str("</li>\n");
        }
        body.push_str("</ol>\n</section>\n");
    }
    body
}

/// markdown-it-anchor's slug as the app configured it: letters, digits, `-`
/// and `_`, then `encodeURIComponent`.
fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in text.trim().to_lowercase().chars() {
        if c.is_whitespace() {
            if !space {
                out.push('-');
            }
            space = true;
            continue;
        }
        space = false;
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c.is_alphanumeric() {
            for b in c.encode_utf8(&mut [0; 4]).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn unique(slug: String, used: &mut HashSet<String>) -> String {
    let mut candidate = slug.clone();
    let mut n = 1;
    while used.contains(&candidate) {
        candidate = format!("{slug}-{n}");
        n += 1;
    }
    used.insert(candidate.clone());
    candidate
}

/// The app's code-block frame, so the exported block looks like the one in the editor.
fn code_block(lang: &str, code: &str) -> String {
    // An explicit hint wins; otherwise the local detector picks, and its score
    // is written onto the block so a reader can see how sure it was.
    let (lang, confidence) = if lang.is_empty() {
        let guess = mdwisp_text::detect::detect_language(code);
        (guess.lang.unwrap_or(""), Some(guess.confidence))
    } else {
        (lang, None)
    };
    let label = escape(if lang.is_empty() { "text" } else { lang });
    let conf = confidence.map_or(String::new(), |c| format!(r#" data-confidence="{c}""#));
    let conf_badge = confidence.map_or(String::new(), |c| format!(r#"<span class="codeblock__conf">{c}%</span>"#));
    format!(
        concat!(
            r#"<div class="codeblock" data-lang="{label}"{conf}><div class="codeblock__bar">"#,
            r#"<button class="codeblock__lang" type="button" data-action="pick-language">{label}{conf_badge}</button>"#,
            r#"<button class="codeblock__copy" type="button" data-action="copy-code" aria-label="{aria}"></button></div>"#,
            r#"<pre class="codeblock__pre"><code class="hljs language-{label}">{body}</code></pre></div>"#,
            "\n"
        ),
        label = label,
        conf = conf,
        conf_badge = conf_badge,
        aria = escape(t("复制代码")),
        body = highlight_code(lang, code),
    )
}

/// Escaped code with highlight.js class spans, so the bundled stylesheet colours it.
fn highlight_code(lang: &str, code: &str) -> String {
    use mdwisp_text::highlight::Token as K;
    let mut out = String::with_capacity(code.len() * 2);
    let mut last = 0;
    for (r, tok) in mdwisp_text::highlight::highlight(lang, code) {
        if r.start < last {
            continue;
        }
        out.push_str(&escape(&code[last..r.start]));
        let class = match tok {
            K::Comment => "comment",
            K::Quote => "quote",
            K::Keyword => "keyword",
            K::Literal => "literal",
            K::Section => "section",
            K::Name => "name",
            K::Meta => "meta",
            K::String => "string",
            K::Attribute => "attribute",
            K::Number => "number",
            K::Symbol => "symbol",
            K::Bullet => "bullet",
            K::Link => "link",
            K::Title => "title",
            K::Type => "type",
            K::BuiltIn => "built_in",
            K::Attr => "attr",
            K::Variable => "variable",
            K::Emphasis => "emphasis",
            K::Strong => "strong",
        };
        out.push_str(&format!(r#"<span class="hljs-{class}">{}</span>"#, escape(&code[r.clone()])));
        last = r.end;
    }
    out.push_str(&escape(&code[last..]));
    out
}

fn render_math(math: &LatexToMathML, tex: &str, display: MathDisplay) -> String {
    match math.convert_with_local_state(tex, display) {
        Ok(r) => r.mathml,
        // Like KaTeX's throwOnError: false — the source stays readable.
        Err(e) => format!(r#"<code class="math-error" title="{}">{}</code>"#, escape(&e.to_string()), escape(tex)),
    }
}

fn display_math(math: &LatexToMathML, tex: &str) -> String {
    format!("<div class=\"math-block\">{}</div>\n", render_math(math, tex.trim(), MathDisplay::Block))
}

/// One text run: bare URLs and e-mail addresses become links (not inside a
/// link label), the rest goes through `push_inline`.
fn push_text<'a>(text: &str, linkify: bool, out: &mut Vec<Event<'a>>) {
    let mut at = 0;
    for (r, href) in if linkify { links::bare_links(text) } else { vec![] } {
        push_inline(&text[at..r.start], out);
        out.push(Event::Start(Tag::Link { link_type: LinkType::Autolink, dest_url: href.into(), title: "".into(), id: "".into() }));
        out.push(Event::Text(text[r.clone()].to_string().into()));
        out.push(Event::End(TagEnd::Link));
        at = r.end;
    }
    push_inline(&text[at..], out);
}

/// `==mark==`, `^sup^` and intraword `~sub~` within one text run.
fn push_inline<'a>(text: &str, out: &mut Vec<Event<'a>>) {
    if !text.contains(['=', '^', '~']) {
        out.push(Event::Text(text.to_string().into()));
        return;
    }
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        if let Some((tag, body, len)) = inline_span(&text[i..]) {
            out.push(Event::Text(std::mem::take(&mut plain).into()));
            out.push(Event::InlineHtml(format!("<{tag}>").into()));
            out.push(Event::Text(body.to_string().into()));
            out.push(Event::InlineHtml(format!("</{tag}>").into()));
            i += len;
            continue;
        }
        let c = text[i..].chars().next().unwrap();
        plain.push(c);
        i += c.len_utf8();
    }
    out.push(Event::Text(plain.into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_carries_a_unique_slug_and_an_anchor() {
        let html = render_markdown("# Hi there\n\n## Hi there\n\n## 标题");
        assert!(html.contains(r##"<h1 id="hi-there" tabindex="-1"><a class="heading-anchor" href="#hi-there" aria-hidden="true">#</a> Hi there</h1>"##));
        assert!(html.contains(r#"<h2 id="hi-there-1""#));
        assert!(html.contains(r#"<h2 id="%E6%A0%87%E9%A2%98""#));
    }

    #[test]
    fn footnotes_collect_into_one_section_in_reference_order() {
        let html = render_markdown("a[^n] b[^1]\n\n[^1]: One.\n[^n]: En.\n");
        assert!(html.contains(r##"<sup class="footnote-ref"><a href="#fn1" id="fnref1">[1]</a></sup>"##));
        assert!(html.contains(r#"<section class="footnotes">"#));
        let first = html.find(r#"<li id="fn1""#).unwrap();
        assert!(html[first..].starts_with(r##"<li id="fn1" class="footnote-item"><p>En. <a href="#fnref1" class="footnote-backref">↩︎</a></p>"##));
    }

    #[test]
    fn task_items_wrap_their_text_in_a_label() {
        let html = render_markdown("- [x] done\n- [ ] todo\n");
        assert!(html.contains(r#"<li class="task-list-item"><label><input class="task-list-item-checkbox" checked="" disabled="" type="checkbox"> done</label></li>"#));
        assert!(html.contains(r#"disabled="" type="checkbox"> todo</label>"#));
    }

    #[test]
    fn marks_highlights_but_not_comparisons() {
        assert!(render_markdown("a ==hot== b").contains("a <mark>hot</mark> b"));
        assert!(render_markdown("x == y").contains("x == y"));
    }

    #[test]
    fn renders_math_as_mathml_and_drops_frontmatter() {
        let html = render_markdown("---\ntitle: x\n---\n\n$a^2$\n\n$$\nE = mc^2\n$$\n");
        assert!(!html.contains("title"));
        assert!(html.contains("<math"));
        assert!(html.contains("<div class=\"math-block\"><math display=\"block\">"));
        assert!(!html.contains("<p><div"));
    }

    #[test]
    fn keeps_sub_sup_deflist_and_strikethrough() {
        let html = render_markdown("H~2~O x^2^ ~~gone~~\n\nTerm\n: Def\n");
        assert!(html.contains("<sub>2</sub>") && html.contains("<sup>2</sup>") && html.contains("<del>gone</del>"));
        assert!(html.contains("<dt>Term</dt>") && html.contains("<dd>Def</dd>"));
    }

    #[test]
    fn links_bare_urls_and_emails_outside_links_and_code() {
        let html = render_markdown("网站：https://a.cn/~x。邮件 me@a.cn\n\n[https://b.cn](https://b.cn) `https://c.cn`\n\n    https://d.cn");
        assert!(html.contains(r#"网站：<a href="https://a.cn/~x">https://a.cn/~x</a>。"#), "{html}");
        assert!(html.contains(r#"<a href="mailto:me@a.cn">me@a.cn</a>"#));
        assert!(html.contains(r#"<a href="https://b.cn">https://b.cn</a>"#) && !html.contains("<a href=\"https://c.cn\"") && !html.contains("<a href=\"https://d.cn\""));
    }
}
