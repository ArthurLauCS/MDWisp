//! The one place a path written into markdown is produced, plus the lexical
//! path helpers Node's `path` module gave the TypeScript version for free.

use std::path::{Component, Path, PathBuf};

use crate::i18n::t;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    External { url: String },
    Document { path: PathBuf, fragment: String },
}

/// `base.join(rel)` with `.` and `..` folded away, like Node's `path.resolve`.
pub fn resolve(base: &Path, rel: impl AsRef<Path>) -> PathBuf {
    normalize(&base.join(rel))
}

pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.components().next_back(), Some(Component::RootDir | Component::Prefix(_)) | None) {
                    out.pop();
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Node's `path.relative`. Windows paths compare case-insensitively; a path
/// on another drive comes back absolute, which callers must refuse.
pub fn relative(from: &Path, to: &Path) -> PathBuf {
    let from = normalize(from);
    let to = normalize(to);
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = to.components().collect();
    let same = |x: &Component, y: &Component| x.as_os_str().to_string_lossy().to_lowercase() == y.as_os_str().to_string_lossy().to_lowercase();
    if a.first().zip(b.first()).is_some_and(|(x, y)| !same(x, y)) {
        return to;
    }
    let common = a.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    let mut out = PathBuf::new();
    for _ in common..a.len() {
        out.push("..");
    }
    for c in &b[common..] {
        out.push(c);
    }
    out
}

pub fn has_md_ext(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".md", ".markdown", ".mdx"].iter().any(|e| lower.ends_with(e))
}

pub fn resolve_document_link(doc_path: &str, href: &str) -> Result<LinkTarget, String> {
    let lower = href.to_lowercase();
    if lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("mailto:") {
        return Ok(LinkTarget::External { url: href.into() });
    }
    if let Some(rest) = href.strip_prefix("//") {
        return Ok(LinkTarget::External { url: format!("https://{rest}") });
    }
    if has_scheme(href) || href.starts_with("\\\\") {
        return Err(t("不支持的链接类型").into());
    }
    let (path, fragment) = match href.find('#') {
        Some(i) => (&href[..i], decode_uri(&href[i + 1..])),
        None => (href, String::new()),
    };
    let path = decode_uri(path.split('?').next().unwrap_or(""));
    if path.is_empty() {
        return Ok(LinkTarget::Document { path: doc_path.into(), fragment });
    }
    if doc_path.is_empty() {
        return Err(t("请先保存文档，再打开相对链接").into());
    }
    if !has_md_ext(&path) || path.contains('\0') {
        return Err(t("只能在应用内打开 Markdown 文档").into());
    }
    let dir = Path::new(doc_path).parent().unwrap_or(Path::new(""));
    Ok(LinkTarget::Document { path: resolve(dir, &path), fragment })
}

/// `scheme:` per RFC 3986, but a single letter is a Windows drive, not a scheme.
fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else { return false };
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}

/// `decodeURIComponent`, but malformed escapes stay as written instead of throwing.
pub fn decode_uri(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap());
        if let (b'%', Some(b)) = (bytes[i], hex) {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// The one and only place a markdown image path is produced.
///
/// Every path written into a document MUST be relative and POSIX-separated,
/// otherwise the folder stops being portable to another machine.
pub fn build_relative_path(from_dir: &Path, to_file: &Path) -> Result<String, String> {
    let rel = relative(from_dir, to_file);
    if rel.is_absolute() || rel.has_root() {
        return Err(format!("refusing to write an absolute asset path: {}", rel.display()));
    }
    // Windows separators break markdown links on other platforms.
    let posix = rel.to_string_lossy().replace('\\', "/");
    Ok(if posix.starts_with('.') { posix } else { format!("./{posix}") })
}

/// True when the text is safe to embed in a markdown link target.
pub fn is_portable_link(target: &str) -> bool {
    let b = target.as_bytes();
    !(target.starts_with('/')
        || target.starts_with('\\')
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\'))
        || target.starts_with("file://"))
}

/// Percent-encode only the characters that would break a markdown link.
/// CJK filenames stay readable on purpose — encoded names are unusable.
pub fn encode_link_target(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if !ch.is_ascii() || ch.is_ascii_alphanumeric() || "-._~!$&'()*+,;=@/".contains(ch) {
            out.push(ch);
        } else {
            let mut buf = [0; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// Resolve an on-disk asset back into a link, encoded and relative.
pub fn to_link(editor_file_path: &Path, asset_abs_path: &Path) -> Result<String, String> {
    let from_dir = editor_file_path.parent().unwrap_or(Path::new(""));
    Ok(encode_link_target(&build_relative_path(from_dir, asset_abs_path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> PathBuf {
        std::env::current_dir().unwrap().join("notes").join("README.md")
    }

    #[test]
    fn resolves_relative_paths_encoding_queries_and_fragments() {
        let doc = doc();
        let d = doc.to_str().unwrap();
        assert_eq!(
            resolve_document_link(d, "../%E4%B8%AD%E6%96%87%20file.md?view=1#%E7%AB%A0%E8%8A%82").unwrap(),
            LinkTarget::Document { path: std::env::current_dir().unwrap().join("中文 file.md"), fragment: "章节".into() }
        );
        assert_eq!(resolve_document_link(d, "#section").unwrap(), LinkTarget::Document { path: doc.clone(), fragment: "section".into() });
        assert_eq!(resolve_document_link("", "#section").unwrap(), LinkTarget::Document { path: "".into(), fragment: "section".into() });
    }

    #[test]
    fn only_allows_browser_and_mail_protocols_or_markdown_files() {
        let doc = doc();
        let d = doc.to_str().unwrap();
        for url in ["https://example.com", "http://example.com", "mailto:test@example.com"] {
            assert_eq!(resolve_document_link(d, url).unwrap(), LinkTarget::External { url: url.into() });
        }
        assert_eq!(resolve_document_link(d, "//example.com").unwrap(), LinkTarget::External { url: "https://example.com".into() });
        for url in ["javascript:alert(1)", "data:text/html,x", "file:///C:/x.md", "ms-settings:defaultapps", "cmd.exe", "./script.bat", "\\\\host\\a.md"] {
            assert!(resolve_document_link(d, url).is_err(), "{url}");
        }
        assert!(resolve_document_link("", "README.md").is_err());
    }

    #[test]
    fn relative_paths_are_posix_and_dot_prefixed() {
        let dir = Path::new(r"C:\docs\Notes");
        assert_eq!(build_relative_path(dir, Path::new(r"C:\docs\Notes\Notes_img\a.png")).unwrap(), "./Notes_img/a.png");
        assert_eq!(build_relative_path(dir, Path::new(r"C:\docs\other\a.png")).unwrap(), "../other/a.png");
        assert_eq!(build_relative_path(dir, Path::new(r"c:\DOCS\notes\x.png")).unwrap(), "./x.png");
        assert!(build_relative_path(dir, Path::new(r"D:\a.png")).is_err());
    }

    #[test]
    fn encodes_only_link_breaking_characters() {
        assert_eq!(encode_link_target("./图 片/a b(1).png"), "./图%20片/a%20b(1).png");
        assert_eq!(to_link(Path::new(r"C:\d\N\N.md"), Path::new(r"C:\d\N\N_img\猫 1.png")).unwrap(), "./N_img/猫%201.png");
    }

    #[test]
    fn decode_keeps_malformed_escapes() {
        assert_eq!(decode_uri("a%20b%zz%"), "a b%zz%");
    }
}
