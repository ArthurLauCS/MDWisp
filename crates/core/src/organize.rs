//! Turn a loose markdown file into a document folder.
//!
//!   下载/报告.md  →  下载/报告/报告.md + 下载/报告/报告_img/
//!
//! A loose file's images can be anywhere: beside it, on another drive, behind
//! a URL. The folder is only portable once every image it shows lives inside
//! it, so each one is copied in and its link rewritten.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::i18n::{t, tf};
use crate::io_err;
use crate::paths::{decode_uri, resolve, to_link};
use crate::workspace::asset_dir_name;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedImage {
    pub from: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizePlan {
    /// The folder that would be created.
    pub target_dir: PathBuf,
    /// Why the folder cannot be created, e.g. the name is taken.
    pub blocked: Option<String>,
    /// Local images that would be copied in.
    pub images: Vec<PlannedImage>,
    /// Remote image URLs, brought in only when asked.
    pub remote: Vec<String>,
    /// Local links that point at nothing.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct OrganizeOptions {
    /// Send the original file to the recycle bin instead of keeping it.
    pub r#move: bool,
    /// Download remote images into the folder.
    pub download: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Organized {
    pub doc_path: PathBuf,
    /// Remote images that could not be downloaded.
    pub failed: Vec<String>,
}

/// Node's `extname`: `.png` for `a.png`, empty for `.gitignore` and `a`.
pub(crate) fn ext(name: &str) -> &str {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rfind('.') {
        Some(i) if i > 0 => &base[i..],
        _ => "",
    }
}

fn is_remote(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("//") || lower.starts_with("http://") || lower.starts_with("https://")
}

fn locate(doc_dir: &Path, target: &str) -> PathBuf {
    if target.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        return file_url_path(&target[5..]);
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    resolve(doc_dir, decode_uri(path))
}

/// `fileURLToPath` for the part after `file:`: `///C:/a%20b.png` or `//host/share/x.png`.
fn file_url_path(rest: &str) -> PathBuf {
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = decode_uri(path.split(['?', '#']).next().unwrap_or(""));
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return PathBuf::from(format!(r"\\{host}{}", path.replace('/', "\\")));
    }
    PathBuf::from(path.strip_prefix('/').unwrap_or(&path).replace('/', "\\"))
}

/// Never let two different images land on one filename.
fn claim(name: &str, taken: &mut HashSet<String>) -> String {
    let e = ext(name);
    let stem = &name[..name.len() - e.len()];
    let mut candidate = name.to_string();
    let mut n = 2;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("{stem}-{n}{e}");
        n += 1;
    }
    taken.insert(candidate.to_lowercase());
    candidate
}

struct Target {
    stem: String,
    dir: PathBuf,
    doc: PathBuf,
    assets: PathBuf,
}

fn target_for(doc_path: &Path) -> Target {
    let stem = stem_of(doc_path);
    let dir = doc_path.parent().unwrap_or(Path::new("")).join(&stem);
    Target { doc: dir.join(format!("{stem}.md")), assets: dir.join(asset_dir_name(&stem)), stem, dir }
}

/// `Notes` for `Notes.md`.
pub(crate) fn stem_of(doc_path: &Path) -> String {
    let file = doc_path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = file.to_lowercase();
    let cut = [".markdown", ".mdx", ".md"].iter().find(|e| lower.ends_with(*e)).map_or(0, |e| e.len());
    file[..file.len() - cut].to_string()
}

/// A document is in its folder when the folder carries its name.
pub fn is_document_folder(doc_path: &Path) -> bool {
    let parent = doc_path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    parent.as_deref() == Some(stem_of(doc_path).as_str())
}

#[derive(Default)]
struct Collected {
    /// Source path → filename inside the new asset folder, in link order.
    local: Vec<(PathBuf, String)>,
    remote: Vec<String>,
    missing: Vec<String>,
    taken: HashSet<String>,
}

impl Collected {
    fn local_name(&self, path: &Path) -> Option<&str> {
        self.local.iter().find(|(p, _)| p == path).map(|(_, n)| n.as_str())
    }
}

fn collect(doc_path: &Path, text: &str) -> Collected {
    let doc_dir = doc_path.parent().unwrap_or(Path::new(""));
    let mut found = Collected::default();
    let mut targets: Vec<String> = vec![];
    map_image_links(text, |t| {
        if !targets.iter().any(|x| x == t) {
            targets.push(t.to_string());
        }
        None
    });
    for target in targets {
        if is_remote(&target) {
            found.remote.push(target);
            continue;
        }
        let abs = locate(doc_dir, &target);
        if found.local_name(&abs).is_some() {
            continue;
        }
        if fs::metadata(&abs).is_ok_and(|m| m.is_file()) {
            let name = claim(&abs.file_name().unwrap_or_default().to_string_lossy(), &mut found.taken);
            found.local.push((abs, name));
        } else {
            found.missing.push(target);
        }
    }
    found
}

fn blocked(doc_path: &Path, target_dir: &Path) -> Option<String> {
    if is_document_folder(doc_path) {
        Some(t("这份文档已经在自己的文档文件夹里").to_string())
    } else if target_dir.exists() {
        Some(tf("同名文件夹已存在：{0}", &[&target_dir.display()]))
    } else {
        None
    }
}

pub fn plan_organize(doc_path: &Path, text: &str) -> OrganizePlan {
    let target = target_for(doc_path);
    let found = collect(doc_path, text);
    OrganizePlan {
        blocked: blocked(doc_path, &target.dir),
        target_dir: target.dir,
        images: found.local.into_iter().map(|(from, name)| PlannedImage { from, name }).collect(),
        remote: found.remote,
        missing: found.missing,
    }
}

fn download(url: &str, asset_dir: &Path, taken: &mut HashSet<String>) -> Result<String, String> {
    use ureq::ResponseExt;
    let url = if url.starts_with("//") { format!("https:{url}") } else { url.to_string() };
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
    let mut res = agent.get(&url).call().map_err(|e| e.to_string())?;
    let kind = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim().to_string();
    let path = decode_uri(res.get_uri().path());
    let base = path.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name: String = base.chars().map(|c| if "\\/:*?\"<>|#%".contains(c) { '-' } else { c }).collect();
    if name.is_empty() {
        name = "image".into();
    }
    if ext(&name).is_empty() {
        name += match kind.as_str() {
            "image/png" => ".png",
            "image/jpeg" => ".jpg",
            "image/gif" => ".gif",
            "image/webp" => ".webp",
            "image/svg+xml" => ".svg",
            "image/avif" => ".avif",
            _ => "",
        };
    }
    let bytes = res.body_mut().read_to_vec().map_err(|e| e.to_string())?;
    let name = claim(&name, taken);
    fs::write(asset_dir.join(&name), bytes).map_err(io_err)?;
    Ok(name)
}

/// Build the folder. `trash` is injected so the originals go to the recycle
/// bin in the app while tests can watch what would have been binned.
pub fn run_organize(
    doc_path: &Path,
    text: &str,
    options: OrganizeOptions,
    mut trash: impl FnMut(&Path) -> Result<(), String>,
) -> Result<Organized, String> {
    let target = target_for(doc_path);
    if let Some(why) = blocked(doc_path, &target.dir) {
        return Err(why);
    }
    let doc_dir = doc_path.parent().unwrap_or(Path::new(""));
    let mut found = collect(doc_path, text);
    let mut failed = vec![];

    fs::create_dir(&target.dir).map_err(io_err)?;
    if !found.local.is_empty() || (options.download && !found.remote.is_empty()) {
        fs::create_dir(&target.assets).map_err(io_err)?;
    }
    for (from, name) in &found.local {
        fs::copy(from, target.assets.join(name)).map_err(io_err)?;
    }

    let mut remote: Vec<(String, String)> = vec![];
    if options.download {
        for url in found.remote.clone() {
            match download(&url, &target.assets, &mut found.taken) {
                Ok(name) => remote.push((url, name)),
                Err(_) => failed.push(url),
            }
        }
    }

    let rewritten = map_image_links(text, |t| {
        let name = if is_remote(t) { remote.iter().find(|(u, _)| u == t).map(|(_, n)| n.as_str()) } else { found.local_name(&locate(doc_dir, t)) };
        name.and_then(|n| to_link(&target.doc, &target.assets.join(n)).ok())
    });
    fs::write(&target.doc, rewritten).map_err(io_err)?;

    if options.r#move {
        trash(doc_path)?;
        // The old sibling asset folder goes too, but only when nothing in it is
        // left behind — it may hold images this document never referenced.
        let old = doc_dir.join(asset_dir_name(&target.stem));
        if let Ok(entries) = fs::read_dir(&old) {
            let names: Vec<_> = entries.filter_map(Result::ok).map(|e| e.file_name()).collect();
            if names.iter().all(|n| found.local_name(&old.join(n)).is_some()) {
                trash(&old)?;
            }
        }
    }

    Ok(Organized { doc_path: target.doc, failed })
}

/// Opening fence marker (` ``` ` or `~~~`, up to three spaces of indent).
fn fence_mark(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = rest.len() - rest.trim_start_matches(ch).len();
    (n >= 3).then(|| &rest[..n])
}

/// Visit every image target outside code, replacing those `f` returns a
/// string for. Line by line: images shown inside fences and inline code are
/// examples for the reader, and rewriting them would corrupt the example.
///
/// The scan rules are duplicated on purpose in `plugin/.../check.mjs`; change both together.
pub fn map_image_links(text: &str, mut f: impl FnMut(&str) -> Option<String>) -> String {
    let mut fence: Option<String> = None;
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mark = fence_mark(line);
        if let Some(open) = &fence {
            if mark.is_some_and(|m| m.as_bytes()[0] == open.as_bytes()[0] && m.len() >= open.len()) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if let Some(m) = mark {
            fence = Some(m.to_string());
            out.push_str(line);
            continue;
        }
        map_line(line, &mut f, &mut out);
    }
    out
}

/// One line: inline code is consumed first so an image written inside it stays untouched.
fn map_line(line: &str, f: &mut impl FnMut(&str) -> Option<String>, out: &mut String) {
    let b = line.as_bytes();
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        let found = match b[i] {
            b'`' => {
                let run = b[i..].iter().take_while(|c| **c == b'`').count();
                let ticks = &line[i..i + run];
                match line[i + run..].find(ticks) {
                    Some(end) => {
                        i += run + end + run;
                        continue;
                    }
                    None => {
                        i += run;
                        continue;
                    }
                }
            }
            b'!' => md_image(line, i),
            b'<' => html_image(line, i),
            _ => None,
        };
        match found {
            Some((end, target)) => {
                let (ts, te) = target;
                let t = &line[ts..te];
                if !t.starts_with("data:") {
                    if let Some(next) = f(t) {
                        out.push_str(&line[copied..ts]);
                        out.push_str(&next);
                        copied = te;
                    }
                }
                i = end;
            }
            None => i += line[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    out.push_str(&line[copied..]);
}

/// `![alt](target "title")` at `at`; returns the end of the match and the target span.
fn md_image(line: &str, at: usize) -> Option<(usize, (usize, usize))> {
    let rest = line.get(at..)?.strip_prefix("![")?;
    let close = rest.find(']')?;
    let after = at + 2 + close + 1;
    let rest = line[after..].strip_prefix('(')?;
    let ts = after + 1;
    let tlen = rest.find(|c: char| c == ')' || c.is_whitespace())?;
    if tlen == 0 {
        return None;
    }
    let te = ts + tlen;
    let tail = &line[te..];
    if tail.starts_with(')') {
        return Some((te + 1, (ts, te)));
    }
    // Optional `"title"` after whitespace.
    let trimmed = tail.trim_start();
    let ws = tail.len() - trimmed.len();
    let title = trimmed.strip_prefix('"')?;
    let q = title.find('"')?;
    let end = te + ws + 1 + q + 1;
    line[end..].starts_with(')').then_some((end + 1, (ts, te)))
}

/// `<img ... src="target"`, case-insensitive like the original regex.
fn html_image(line: &str, at: usize) -> Option<(usize, (usize, usize))> {
    let head = line.get(at..at + 4)?;
    if !head.eq_ignore_ascii_case("<img") {
        return None;
    }
    let tag_end = line[at..].find('>').map_or(line.len(), |e| at + e);
    let lower = line[at..tag_end].to_ascii_lowercase();
    // `[^>]+` before `src=` needs at least one character after `<img`.
    let src = lower[5.min(lower.len())..].find("src=").map(|p| p + 5)?;
    let quote_at = at + src + 4;
    let quote = *line.as_bytes().get(quote_at)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let ts = quote_at + 1;
    let len = line[ts..].find(['"', '\''])?;
    if len == 0 {
        return None;
    }
    Some((ts + len + 1, (ts, ts + len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(text: &str) -> Vec<String> {
        let mut v = vec![];
        map_image_links(text, |t| {
            v.push(t.to_string());
            None
        });
        v
    }

    #[test]
    fn skips_fences_closed_by_longer_or_different_markers_correctly() {
        assert_eq!(seen("````\n```\n![a](x.png)\n````\n~~~\n![b](y.png)\n~~~\n![c](z.png)"), ["z.png"]);
    }

    #[test]
    fn finds_markdown_and_html_images_but_not_inline_code_or_data() {
        let text = "![近](./a%20b.png \"标题\") 和 ![远](C:/x/shot.png)\n<IMG width=1 src='pics/shot.png'>\n行内 `![例](./c.png)` 不动 ![内嵌](data:image/png;base64,AAAA)";
        assert_eq!(seen(text), ["./a%20b.png", "C:/x/shot.png", "pics/shot.png"]);
    }

    #[test]
    fn rewrites_only_the_target() {
        let out = map_image_links("x ![a](old.png \"t\") y <img src=\"old.png\">", |t| Some(format!("./n/{t}")));
        assert_eq!(out, "x ![a](./n/old.png \"t\") y <img src=\"./n/old.png\">");
    }

    struct Loose {
        _root: tempfile::TempDir,
        root: PathBuf,
        doc: PathBuf,
        text: String,
        elsewhere: PathBuf,
    }

    fn loose() -> Loose {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let elsewhere = root.join("elsewhere").join("shot.png");
        fs::create_dir(root.join("elsewhere")).unwrap();
        fs::create_dir_all(root.join("inbox").join("报告_img")).unwrap();
        fs::create_dir(root.join("inbox").join("pics")).unwrap();
        fs::write(&elsewhere, "far").unwrap();
        fs::write(root.join("inbox").join("报告_img").join("a b.png"), "near").unwrap();
        fs::write(root.join("inbox").join("pics").join("shot.png"), "same name, different image").unwrap();
        let text = [
            "# 报告".to_string(),
            format!("![近](./报告_img/a%20b.png \"标题\") 和 ![远]({})", elsewhere.to_string_lossy().replace('\\', "/")),
            "<img src=\"pics/shot.png\" width=\"40\">".into(),
            "![网](https://example.com/x.png) ![丢](./gone.png) ![内嵌](data:image/png;base64,AAAA)".into(),
            "行内 `![例](./报告_img/a%20b.png)` 不动".into(),
            "```md".into(),
            "![例](./报告_img/a%20b.png)".into(),
            "```".into(),
        ]
        .join("\n");
        let doc = root.join("inbox").join("报告.md");
        fs::write(&doc, &text).unwrap();
        Loose { _root: dir, root, doc, text, elsewhere }
    }

    fn binning(trashed: &mut Vec<PathBuf>) -> impl FnMut(&Path) -> Result<(), String> + '_ {
        |p| {
            trashed.push(p.to_path_buf());
            if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) }.map_err(io_err)
        }
    }

    const KEEP: OrganizeOptions = OrganizeOptions { r#move: false, download: false };
    const MOVE: OrganizeOptions = OrganizeOptions { r#move: true, download: false };

    #[test]
    fn plans_without_touching_the_disk() {
        let l = loose();
        let plan = plan_organize(&l.doc, &l.text);
        assert_eq!(plan.blocked, None);
        assert_eq!(plan.images.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["a b.png", "shot.png", "shot-2.png"]);
        assert_eq!(plan.remote, ["https://example.com/x.png"]);
        assert_eq!(plan.missing, ["./gone.png"]);
        assert!(!plan.target_dir.exists());
    }

    #[test]
    fn copies_every_image_in_and_rewrites_only_real_links() {
        let l = loose();
        let mut trashed = vec![];
        let result = run_organize(&l.doc, &l.text, KEEP, binning(&mut trashed)).unwrap();
        assert_eq!(result.doc_path, l.root.join("inbox").join("报告").join("报告.md"));
        assert!(is_document_folder(&result.doc_path));

        let out = fs::read_to_string(&result.doc_path).unwrap();
        let out: Vec<_> = out.split('\n').collect();
        assert_eq!(out[1], "![近](./报告_img/a%20b.png \"标题\") 和 ![远](./报告_img/shot.png)");
        assert_eq!(out[2], "<img src=\"./报告_img/shot-2.png\" width=\"40\">");
        assert_eq!(out[3..], l.text.split('\n').collect::<Vec<_>>()[3..]);

        let assets = l.root.join("inbox").join("报告").join("报告_img");
        assert_eq!(fs::read_to_string(assets.join("shot.png")).unwrap(), "far");
        assert_eq!(fs::read_to_string(assets.join("shot-2.png")).unwrap(), "same name, different image");
        assert_eq!(fs::read_to_string(&l.doc).unwrap(), l.text);
        assert!(trashed.is_empty());
    }

    #[test]
    fn moving_bins_the_original_and_an_asset_folder_with_nothing_left_in_it() {
        let l = loose();
        let mut trashed = vec![];
        run_organize(&l.doc, &l.text, MOVE, binning(&mut trashed)).unwrap();
        assert_eq!(trashed, [l.doc.clone(), l.root.join("inbox").join("报告_img")]);
        assert!(l.elsewhere.exists());
        assert!(l.root.join("inbox").join("pics").join("shot.png").exists());
    }

    #[test]
    fn moving_keeps_an_old_asset_folder_that_still_holds_unreferenced_files() {
        let l = loose();
        fs::write(l.root.join("inbox").join("报告_img").join("unused.png"), "x").unwrap();
        let mut trashed = vec![];
        run_organize(&l.doc, &l.text, MOVE, binning(&mut trashed)).unwrap();
        assert_eq!(trashed, [l.doc.clone()]);
    }

    #[test]
    fn refuses_to_overwrite_an_existing_folder_or_redo_a_finished_one() {
        let _lang = crate::LANG.lock().unwrap_or_else(|e| e.into_inner());
        let l = loose();
        fs::create_dir(l.root.join("inbox").join("报告")).unwrap();
        assert!(plan_organize(&l.doc, &l.text).blocked.unwrap().contains("同名文件夹已存在"));
        let mut trashed = vec![];
        assert!(run_organize(&l.doc, &l.text, MOVE, binning(&mut trashed)).unwrap_err().contains("同名文件夹已存在"));
        assert!(trashed.is_empty());

        let inside = l.root.join("inbox").join("报告").join("报告.md");
        fs::write(&inside, "").unwrap();
        assert!(plan_organize(&inside, "").blocked.unwrap().contains("已经在"));
    }

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(locate(Path::new(r"C:\d"), "file:///C:/a%20b/x.png"), PathBuf::from(r"C:\a b\x.png"));
        assert_eq!(locate(Path::new(r"C:\d"), "file://host/share/x.png"), PathBuf::from(r"\\host\share\x.png"));
    }
}
