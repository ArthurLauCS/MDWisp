use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::paths::{has_md_ext, normalize};
use crate::{io_err, now_ms};

const IGNORED: [&str; 5] = ["node_modules", ".git", ".mdview", ".obsidian", ".trash"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Dir,
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
    /// Present for documents, None for directories and assets.
    pub doc_id: Option<String>,
    /// `_img`-style asset directory (or a file in one), shown separately in the sidebar.
    pub is_asset_dir: bool,
    pub children: Option<Vec<TreeNode>>,
    pub mtime_ms: f64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub root_path: PathBuf,
    pub name: String,
    pub tree: TreeNode,
    pub opened_at: u64,
}

/// Stable document id from an absolute path — case-insensitive on Windows.
/// History folders are named by it, so the scheme must never change.
pub fn doc_id_for(abs_path: &Path) -> String {
    let digest = Sha1::digest(abs_path.to_string_lossy().to_lowercase().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// `Notes_img` for `Notes` — the one place this naming rule lives.
pub fn asset_dir_name(stem: &str) -> String {
    format!("{stem}_img")
}

pub fn is_asset_dir_name(name: &str) -> bool {
    name.ends_with("_img") || name == "img" || name == "assets"
}

#[derive(Default)]
pub struct Workspace {
    current: Option<WorkspaceInfo>,
}

impl Workspace {
    pub fn current(&self) -> Option<&WorkspaceInfo> {
        self.current.as_ref()
    }

    pub fn root_path(&self) -> Result<&Path, String> {
        self.current.as_ref().map(|w| w.root_path.as_path()).ok_or_else(|| "no workspace is open".to_string())
    }

    /// Open a folder, or the folder holding a file.
    pub fn open(&mut self, path: &Path) -> Result<&WorkspaceInfo, String> {
        let abs = normalize(&std::path::absolute(path).map_err(io_err)?);
        let root = if fs::metadata(&abs).map_err(io_err)?.is_dir() { abs } else { abs.parent().unwrap_or(&abs).to_path_buf() };
        let tree = scan(&root, 0)?;
        let name = file_name(&root);
        Ok(self.current.insert(WorkspaceInfo { root_path: root, name, tree, opened_at: now_ms() }))
    }

    pub fn rescan(&mut self) -> Result<&WorkspaceInfo, String> {
        let root = self.root_path()?.to_path_buf();
        self.open(&root)
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn mtime_ms(meta: &fs::Metadata) -> f64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// Recursive scan. Prunes ignored dirs and asset dirs before descending.
pub fn scan(dir: &Path, depth: usize) -> Result<TreeNode, String> {
    let mut children = vec![];
    if depth <= 12 {
        for entry in fs::read_dir(dir).map_err(io_err)? {
            let entry = entry.map_err(io_err)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = entry.file_type().map_err(io_err)?;
            if IGNORED.contains(&name.as_str()) || (name.starts_with('.') && kind.is_dir()) {
                continue;
            }
            let abs = dir.join(&name);
            if kind.is_dir() {
                // Asset dirs are listed but not walked — they only ever hold binaries.
                children.push(if is_asset_dir_name(&name) { asset_dir_node(&abs, name)? } else { scan(&abs, depth + 1)? });
            } else if kind.is_file() && has_md_ext(&name) {
                let st = fs::metadata(&abs).map_err(io_err)?;
                children.push(TreeNode {
                    name,
                    doc_id: Some(doc_id_for(&abs)),
                    path: abs,
                    kind: NodeKind::File,
                    is_asset_dir: false,
                    children: None,
                    mtime_ms: mtime_ms(&st),
                    size_bytes: st.len(),
                });
            }
        }
    }
    children.sort_by(|a, b| match (a.kind, b.kind) {
        (NodeKind::Dir, NodeKind::File) => Ordering::Less,
        (NodeKind::File, NodeKind::Dir) => Ordering::Greater,
        _ => natural_cmp(&a.name, &b.name),
    });
    let st = fs::metadata(dir).map_err(io_err)?;
    Ok(TreeNode {
        name: file_name(dir),
        path: dir.to_path_buf(),
        kind: NodeKind::Dir,
        doc_id: None,
        is_asset_dir: false,
        children: Some(children),
        mtime_ms: mtime_ms(&st),
        size_bytes: 0,
    })
}

fn asset_dir_node(dir: &Path, name: String) -> Result<TreeNode, String> {
    let st = fs::metadata(dir).map_err(io_err)?;
    let mtime = mtime_ms(&st);
    let mut children = vec![];
    for entry in fs::read_dir(dir).map_err(io_err)? {
        let entry = entry.map_err(io_err)?;
        if entry.file_type().map_err(io_err)?.is_file() {
            children.push(TreeNode {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                kind: NodeKind::File,
                doc_id: None,
                is_asset_dir: true,
                children: None,
                mtime_ms: mtime,
                size_bytes: 0,
            });
        }
    }
    Ok(TreeNode { name, path: dir.to_path_buf(), kind: NodeKind::Dir, doc_id: None, is_asset_dir: true, children: Some(children), mtime_ms: mtime, size_bytes: 0 })
}

/// Case-insensitive, with digit runs compared by value: `第2章` before `第10章`.
/// Stands in for ICU's `zh-Hans-CN` numeric collation, minus pinyin order.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        s.push(c);
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let o = m.len().cmp(&n.len()).then_with(|| m.cmp(&n));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_id_is_sha1_of_the_lowercased_path() {
        assert_eq!(doc_id_for(Path::new(r"C:\A.md")), doc_id_for(Path::new(r"c:\a.md")));
        assert_eq!(doc_id_for(Path::new("abc")), "a9993e364706816a");
    }

    #[test]
    fn scans_documents_dirs_first_and_lists_asset_dirs_without_walking() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        fs::create_dir_all(r.join("第10章")).unwrap();
        fs::create_dir_all(r.join("第2章")).unwrap();
        fs::create_dir_all(r.join("a_img").join("deep")).unwrap();
        fs::create_dir_all(r.join(".hidden")).unwrap();
        fs::create_dir_all(r.join("node_modules")).unwrap();
        fs::write(r.join("a_img").join("x.png"), "x").unwrap();
        fs::write(r.join("B.md"), "b").unwrap();
        fs::write(r.join("a.md"), "a").unwrap();
        fs::write(r.join("note.txt"), "t").unwrap();
        let mut ws = Workspace::default();
        let info = ws.open(&r.join("a.md")).unwrap();
        let names: Vec<_> = info.tree.children.as_ref().unwrap().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["a_img", "第2章", "第10章", "a.md", "B.md"]);
        let assets = &info.tree.children.as_ref().unwrap()[0];
        assert!(assets.is_asset_dir);
        assert_eq!(assets.children.as_ref().unwrap().iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["x.png"]);
        assert_eq!(info.tree.children.as_ref().unwrap()[3].size_bytes, 1);
        assert!(ws.rescan().is_ok());
    }
}
