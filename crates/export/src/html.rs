//! Build one self-contained HTML file from a document.
//!
//! Self-contained means what it says: after this runs, opening the file makes
//! no network requests and needs no sibling folder. Every local image is
//! base64 in the markup, the stylesheet is inline, and a script would be an
//! admission of failure — there is none.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::Engine;
use mdwisp_core::i18n::{Locale, current_language, t, tf};
use mdwisp_core::paths::{decode_uri, resolve};
use regex::{Captures, Regex};

use crate::markdown::{escape, render_markdown};
use crate::{Omitted, Theme, stem};

pub struct HtmlBuild {
    pub html: String,
    /// Local images that were embedded, by document-relative path.
    pub inlined: Vec<String>,
    /// Local images that could not be read, with a reason for the UI.
    pub missing: Vec<Omitted>,
    /// Images left pointing at a remote URL.
    pub external: Vec<String>,
    pub bytes: usize,
}

fn mime(path: &Path) -> &'static str {
    let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// A missing asset is a designed state, not a broken-image glyph. The frame
/// carries the app's own `is-missing` class so the two renderings agree.
/// `alt` is already escaped: it comes out of the rendered markup.
fn placeholder(alt: &str, src: &str) -> String {
    let label = if alt.is_empty() {
        let decoded = decode_uri(src).replace('\\', "/");
        let base = decoded.rsplit('/').next().unwrap_or("");
        escape(if base.is_empty() { t("图片") } else { base })
    } else {
        alt.to_string()
    };
    format!(
        r#"<figure class="img-missing"><div class="img-missing__frame is-missing" role="img" aria-label="{label}"></div>{}</figure>"#,
        tf("<figcaption>缺失：{0}</figcaption>", &[&label])
    )
}

/// What the standalone file can do with an image source: `None` for a real
/// URL or `data:` the browser handles itself, otherwise a local candidate.
/// A relative path resolves against the document folder; an absolute one is
/// used as written. Either way it is *attempted*, because failing to try
/// leaves a dead reference in the file.
fn classify(src: &str, doc_dir: &Path) -> Option<PathBuf> {
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("data:") {
        return None;
    }
    if lower.starts_with("file://") {
        let rest = &src[7..];
        // `file:///C:/x` → `C:/x`; `file://host/share` → `//host/share`.
        let path = match rest.strip_prefix('/') {
            Some(p) if p.as_bytes().get(1) == Some(&b':') => p.to_string(),
            Some(_) => rest.to_string(),
            None => format!("//{rest}"),
        };
        return Some(PathBuf::from(path));
    }
    // A drive letter looks exactly like a URL scheme (`C:\...`), so single-letter
    // schemes are never treated as remote.
    let b = src.as_bytes();
    if (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || src.starts_with("\\\\") {
        return Some(PathBuf::from(src));
    }
    static SCHEME: OnceLock<Regex> = OnceLock::new();
    if SCHEME.get_or_init(|| Regex::new(r"^(?i)[a-z][a-z0-9+.-]*:").unwrap()).is_match(src) || src.starts_with("//") {
        return None;
    }
    Some(resolve(doc_dir, src))
}

fn embed_images(html: &str, doc_dir: &Path) -> (String, Vec<String>, Vec<Omitted>, Vec<String>) {
    static IMG: OnceLock<Regex> = OnceLock::new();
    static ALT: OnceLock<Regex> = OnceLock::new();
    let img = IMG.get_or_init(|| Regex::new(r#"(?i)<img\b[^>]*?\bsrc=(?:"([^"']+)"|'([^"']+)')[^>]*>"#).unwrap());
    let alt_re = ALT.get_or_init(|| Regex::new(r#"(?i)\balt=(?:"([^"]*)"|'([^']*)')"#).unwrap());

    let (mut inlined, mut missing, mut external) = (Vec::new(), Vec::new(), Vec::new());
    let out = img.replace_all(html, |c: &Captures| {
        let whole = &c[0];
        let src = c.get(1).or(c.get(2)).unwrap().as_str();
        // A query or fragment belongs to the URL rather than the file.
        let path_part = &src[..src.find(['?', '#']).unwrap_or(src.len())];
        let Some(abs) = classify(&decode_uri(path_part), doc_dir) else {
            external.push(src.to_string());
            return whole.to_string();
        };
        let alt = alt_re.captures(whole).and_then(|a| a.get(1).or(a.get(2))).map_or("", |m| m.as_str());
        match std::fs::read(&abs) {
            Ok(data) => {
                inlined.push(src.to_string());
                format!(
                    r#"<img src="data:{};base64,{}" alt="{alt}" data-source="{}">"#,
                    mime(&abs),
                    base64::engine::general_purpose::STANDARD.encode(data),
                    escape(src)
                )
            }
            Err(_) => {
                missing.push(Omitted { path: abs, reason: t("本地图片读不到，已用占位符代替").into() });
                placeholder(alt, src)
            }
        }
    });
    (out.into_owned(), inlined, missing, external)
}

pub fn build_html(doc_path: &Path, source: &str, theme: Theme) -> io::Result<HtmlBuild> {
    let doc_dir = doc_path.parent().unwrap_or(Path::new(""));
    let (body, inlined, missing, external) = embed_images(&render_markdown(source), doc_dir);
    let lang = match current_language() {
        Locale::En => "en",
        Locale::ZhCn => "zh-CN",
    };
    let theme_attr = if theme == Theme::Light { r#" data-theme="light""# } else { "" };
    let html = format!(
        r#"<!doctype html>
<html lang="{lang}"{theme_attr}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="MDWisp">
<title>{title}</title>
<style>
{css}
</style>
</head>
<body>
<article class="md">
{body}
</article>
</body>
</html>
"#,
        title = escape(&stem(doc_path)),
        css = crate::css::document_styles(source)?,
    );
    let bytes = html.len();
    Ok(HtmlBuild { html, inlined, missing, external, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A real 1×1 PNG — a fake byte string would not prove the mime mapping.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63,
        0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb1, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
        0x42, 0x60, 0x82,
    ];

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        // No leading dot: `\.` in a markdown link target is an escape, so `.tmpXXXX` would lose its dot.
        let root = tempfile::Builder::new().prefix("mdview-html-").tempdir().unwrap();
        let img_dir = root.path().join("发布说明_img");
        fs::create_dir_all(&img_dir).unwrap();
        fs::write(img_dir.join("架构草图.png"), PNG).unwrap();
        let doc = root.path().join("发布说明.md");
        fs::write(&doc, "# 标题\n").unwrap();
        (root, doc)
    }

    fn build(source: &str) -> HtmlBuild {
        let (_root, doc) = fixture();
        build_html(&doc, source, Theme::Dark).unwrap()
    }

    #[test]
    fn embeds_a_relative_image_as_a_base64_data_uri() {
        let r = build("![架构草图](./发布说明_img/架构草图.png)");
        assert_eq!(r.inlined.len(), 1);
        assert!(r.html.contains("data:image/png;base64,"));
        assert!(r.html.contains(r#"alt="架构草图""#));
        assert!(!r.html.contains(r#"src="./发布说明_img/架构草图.png""#));
    }

    #[test]
    fn leaves_a_remote_image_pointing_at_its_url() {
        let r = build("![远程](https://example.com/a.png)");
        assert_eq!(r.external, ["https://example.com/a.png"]);
        assert!(r.html.contains(r#"src="https://example.com/a.png""#));
        assert!(!r.html.contains("data:image/png;base64,"));
    }

    #[test]
    fn leaves_a_data_uri_untouched() {
        let uri = "data:image/gif;base64,R0lGOD";
        let r = build(&format!("![内联]({uri})"));
        assert!(r.html.contains(uri));
        assert!(r.inlined.is_empty());
    }

    #[test]
    fn replaces_a_missing_image_with_a_styled_placeholder_carrying_the_alt() {
        let r = build("![找不到的图](./发布说明_img/不存在.png)");
        assert!(r.html.contains("img-missing__frame"));
        assert!(r.html.contains("缺失：找不到的图"));
        assert_eq!(r.missing.len(), 1);
    }

    #[test]
    fn falls_back_to_the_filename_when_a_missing_image_has_no_alt() {
        assert!(build("![](./发布说明_img/不存在.png)").html.contains("缺失：不存在.png"));
    }

    #[test]
    fn treats_a_windows_absolute_path_as_local_not_as_a_url_scheme() {
        let r = build(r"![截图](C:\Users\me\Pictures\shot.png)");
        assert!(r.external.is_empty());
        assert_eq!(r.missing.len(), 1);
        assert!(!r.html.contains(r"C:\Users"));
    }

    #[test]
    fn resolves_an_absolute_path_that_does_exist() {
        let (_root, doc) = fixture();
        let abs = doc.parent().unwrap().join("发布说明_img").join("架构草图.png");
        let r = build_html(&doc, &format!("![草图]({})", abs.display()), Theme::Dark).unwrap();
        assert_eq!(r.inlined.len(), 1);
    }

    #[test]
    fn tolerates_a_percent_encoded_chinese_relative_path() {
        let r = build("![草图](./%E5%8F%91%E5%B8%83%E8%AF%B4%E6%98%8E_img/%E6%9E%B6%E6%9E%84%E8%8D%89%E5%9B%BE.png)");
        assert_eq!(r.inlined.len(), 1);
    }

    #[test]
    fn produces_a_document_that_stands_alone() {
        let r = build("# 标题\n\n正文");
        assert!(r.html.starts_with("<!doctype html>"));
        assert!(r.html.contains("<style>"));
        assert!(r.html.contains("--accent:"));
        // No script, no external stylesheet, no font request.
        assert!(!r.html.contains("<script"));
        assert!(!r.html.contains("<link"));
        assert!(!Regex::new(r#"https?://[^"' ]*\.(css|js)"#).unwrap().is_match(&r.html));
        assert!(!r.html.contains("url('./fonts/"));
        // The bundled faces are embedded when fetched (scripts/fonts.ps1); the CJK ones are cut
        // down to this document's characters, so the file stays small.
        if crate::css::fonts_fetched() {
            assert!(r.html.contains("data:font/otf;base64,"));
            assert!(r.html.contains("data:font/ttf;base64,"));
            assert!(r.html.len() < 2_000_000, "{} bytes", r.html.len());
        }
        assert!(!r.html.contains("*:hover >"));
        assert!(!Regex::new(r"body,\s*\{").unwrap().is_match(&r.html));
    }

    #[test]
    fn renders_formulas_as_mathml_without_math_fonts_or_scripts() {
        let math = build("$$\nE = mc^2\n$$");
        assert!(math.html.contains(r#"<div class="math-block"><math display="block">"#));
        assert!(!math.html.contains("katex"));
        assert!(!math.html.contains("data:font/woff2"));
    }

    #[test]
    fn keeps_the_app_code_block_markup_so_the_two_renderings_agree() {
        let r = build("```\ninterface X { a: string }\n```");
        assert!(r.html.contains("codeblock__lang"));
        assert!(r.html.contains(r#"data-lang="typescript""#));
        assert!(r.html.contains(r#"<span class="hljs-keyword">interface</span>"#));
    }

    #[test]
    fn switches_theme_by_attribute_with_dark_values_left_as_the_default() {
        let (_root, doc) = fixture();
        let dark = build_html(&doc, "# 标题", Theme::Dark).unwrap();
        let light = build_html(&doc, "# 标题", Theme::Light).unwrap();
        assert!(light.html.contains(r#"<html lang="zh-CN" data-theme="light">"#));
        assert!(dark.html.contains(r#"<html lang="zh-CN">"#));
        assert!(dark.html.contains("[data-theme='light']"));
    }
}
