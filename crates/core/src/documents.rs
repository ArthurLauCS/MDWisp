use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::i18n::t;
use crate::io_err;
use crate::organize::{is_document_folder, stem_of};
use crate::paths::{build_relative_path, normalize, relative, to_link};
use crate::workspace::{asset_dir_name, doc_id_for};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    /// Stable id: sha1 of the absolute path, lowercased.
    pub id: String,
    pub path: PathBuf,
    /// Directory that holds the document folder.
    pub parent_dir: PathBuf,
    /// `Notes` for `Notes.md` — the folder stem and the `_img` prefix.
    pub stem: String,
    /// Where images for this document go.
    pub asset_dir: PathBuf,
    /// False for a loose file: its folder is not its own, so it cannot be zipped or shared as one.
    pub in_folder: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentContent {
    pub meta: DocumentMeta,
    pub text: String,
}

/// Absolute and normalized, like Node's `path.resolve(p)`.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    normalize(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

pub fn meta_for(path: &Path) -> DocumentMeta {
    let path = absolute(path);
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    let stem = stem_of(&path);
    DocumentMeta {
        id: doc_id_for(&path),
        asset_dir: dir.join(asset_dir_name(&stem)),
        in_folder: is_document_folder(&path),
        parent_dir: dir,
        stem,
        path,
    }
}

pub fn read(path: &Path) -> Result<DocumentContent, String> {
    let meta = meta_for(path);
    let text = fs::read_to_string(&meta.path).map_err(io_err)?;
    Ok(DocumentContent { meta, text })
}

/// Byte length of a leading YAML front matter block, 0 when there is none.
///
/// Markdown parsers don't know front matter: they read the closing `---` as a
/// setext underline and turn the last metadata line into a heading.
pub fn frontmatter_end(source: &str) -> usize {
    let fence = |line: &str| line.strip_prefix("---").is_some_and(|r| r.trim_start_matches([' ', '\t']).is_empty());
    let mut at = 0;
    for (i, line) in source.split_inclusive('\n').enumerate() {
        at += line.len();
        let closed = line.ends_with('\n');
        let body = line.strip_suffix('\n').map_or(line, |l| l.strip_suffix('\r').unwrap_or(l));
        if i == 0 {
            if !closed || !fence(body) {
                return 0;
            }
        } else if fence(body) {
            return at;
        }
    }
    0
}

/// Writes are serialized so two windows saving the same file can't both pass
/// the `expected` check and the second silently overwrite the first.
#[derive(Default)]
pub struct Documents {
    lock: Mutex<()>,
}

impl Documents {
    /// Write `text`; with `expected`, only when the file still holds exactly that.
    pub fn write(&self, path: &Path, text: &str, expected: Option<&str>) -> Result<(), String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let path = absolute(path);
        if let Some(expected) = expected {
            if fs::read_to_string(&path).map_err(io_err)? != expected {
                return Err(t("文件已被其他窗口或程序修改。请复制当前内容后重新打开，避免覆盖已有修改。").into());
            }
        }
        fs::write(&path, text).map_err(io_err)
    }
}

fn safe_stem(stem: &str) -> String {
    stem.chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect::<String>().trim().to_string()
}

/// Create `Notes.md` and, when `with_asset_folder` is set, its asset folder too,
/// so the very first image insert has somewhere portable to land.
pub fn create(dir: &Path, stem: &str, with_asset_folder: bool) -> Result<DocumentMeta, String> {
    let mut safe = safe_stem(stem);
    if safe.is_empty() {
        safe = "Untitled".into();
    }
    let path = dir.join(format!("{safe}.md"));
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(format!("already exists: {}", path.display())),
        r => r.map_err(io_err)?,
    };
    file.write_all(format!("# {safe}\n\n").as_bytes()).map_err(io_err)?;
    let meta = meta_for(&path);
    if with_asset_folder {
        fs::create_dir_all(&meta.asset_dir).map_err(io_err)?;
    }
    Ok(meta)
}

/// Rename the document and carry its asset folder + every in-document link
/// along with it. Without the rewrite the folder stops being self-contained.
pub fn rename(path: &Path, next_stem: &str) -> Result<DocumentMeta, String> {
    let from = meta_for(path);
    let safe = safe_stem(next_stem);
    if safe.is_empty() || safe == from.stem {
        return Ok(from);
    }
    let next_path = from.parent_dir.join(format!("{safe}.md"));
    fs::rename(&from.path, &next_path).map_err(io_err)?;
    let next = meta_for(&next_path);
    if fs::rename(&from.asset_dir, &next.asset_dir).is_err() {
        // No asset folder yet — nothing to move.
        return Ok(next);
    }
    let text = fs::read_to_string(&next.path).map_err(io_err)?;
    let rewritten = text.replace(&format!("{}/", asset_dir_name(&from.stem)), &format!("{}/", asset_dir_name(&next.stem)));
    if rewritten != text {
        fs::write(&next.path, rewritten).map_err(io_err)?;
    }
    Ok(next)
}

/// To the recycle bin, never a hard delete.
pub fn delete(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Point a document at an asset that already exists on disk, going through
/// the one path builder so it can never emit an absolute path.
pub fn relink_for(doc_path: &Path, asset_path: &Path) -> Result<String, String> {
    to_link(doc_path, asset_path)
}

pub fn asset_link_prefix(doc_path: &Path) -> Result<String, String> {
    let meta = meta_for(doc_path);
    build_relative_path(&meta.parent_dir, &meta.asset_dir)
}

/// Make a bundled document usable as a real document; None when the bundle lacks it.
///
/// It cannot be opened where it lives: the install directory is read-only,
/// and the document needs writable space for the images the user will add.
/// So the folder holding it — markdown and its images together — is copied
/// to `<documents>/MDWisp` the first time it is asked for, and left alone on
/// every later request.
pub fn resolve_stock(stock_root: &Path, documents: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let source = stock_root.join(name);
    if !source.exists() {
        return Ok(None);
    }
    let dest_root = documents.join("MDWisp");
    let target = dest_root.join(name);
    if target.exists() {
        return Ok(Some(target));
    }
    let rel_dir = relative(stock_root, &source).parent().map(Path::to_path_buf).unwrap_or_default();
    fs::create_dir_all(&dest_root).map_err(io_err)?;
    if rel_dir.as_os_str().is_empty() {
        fs::create_dir_all(target.parent().unwrap_or(&dest_root)).map_err(io_err)?;
        fs::copy(&source, &target).map_err(io_err)?;
    } else {
        copy_dir(&stock_root.join(&rel_dir), &dest_root.join(&rel_dir)).map_err(io_err)?;
    }
    Ok(target.exists().then_some(target))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_stale_simultaneous_save_without_overwriting_another_window() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("doc.md");
        fs::write(&path, "# Original").unwrap();
        let docs = Documents::default();
        let results: Vec<_> = std::thread::scope(|s| {
            let a = s.spawn(|| docs.write(&path, "# First edit", Some("# Original")));
            let b = s.spawn(|| docs.write(&path, "# Second edit", Some("# Original")));
            [a.join().unwrap(), b.join().unwrap()]
        })
        .into();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        let winner = if results[0].is_ok() { "# First edit" } else { "# Second edit" };
        assert_eq!(fs::read_to_string(&path).unwrap(), winner);
        docs.write(&path, "# Next edit", Some(winner)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Next edit");
    }

    #[test]
    fn creates_renames_and_carries_the_asset_folder_and_links() {
        let root = tempfile::tempdir().unwrap();
        let meta = create(root.path(), " a:b ", true).unwrap();
        assert_eq!(meta.stem, "ab");
        assert!(meta.asset_dir.is_dir());
        assert!(create(root.path(), "ab", false).unwrap_err().starts_with("already exists"));
        fs::write(&meta.path, "![x](./ab_img/x.png)").unwrap();
        let next = rename(&meta.path, "cd").unwrap();
        assert_eq!(fs::read_to_string(&next.path).unwrap(), "![x](./cd_img/x.png)");
        assert!(next.asset_dir.is_dir() && !meta.path.exists());
        assert_eq!(asset_link_prefix(&next.path).unwrap(), "./cd_img");
    }

    #[test]
    fn copies_a_bundled_document_folder_once() {
        let stock = tempfile::tempdir().unwrap();
        let docs = tempfile::tempdir().unwrap();
        fs::create_dir_all(stock.path().join("W").join("W_img")).unwrap();
        fs::write(stock.path().join("W").join("W.md"), "w").unwrap();
        fs::write(stock.path().join("W").join("W_img").join("a.png"), "a").unwrap();
        let target = resolve_stock(stock.path(), docs.path(), "W/W.md").unwrap().unwrap();
        assert_eq!(target, docs.path().join("MDWisp").join("W/W.md"));
        assert!(docs.path().join("MDWisp").join("W").join("W_img").join("a.png").exists());
        fs::write(&target, "edited").unwrap();
        resolve_stock(stock.path(), docs.path(), "W/W.md").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "edited");
        assert_eq!(resolve_stock(stock.path(), docs.path(), "nope.md").unwrap(), None);
    }

    #[test]
    fn finds_the_front_matter_block() {
        assert_eq!(frontmatter_end("---\ntitle: a\n---\n# H"), 17);
        assert_eq!(frontmatter_end("---\r\n---  \r\nx"), 12);
        assert_eq!(frontmatter_end("---\na\n---"), 9);
        assert_eq!(frontmatter_end("# H\n---\n"), 0);
        assert_eq!(frontmatter_end("---\nopen"), 0);
    }
}
