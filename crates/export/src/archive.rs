//! Package a document folder so it survives the trip to another machine.
//!
//! The whole point of this mode is that the markdown is not rewritten: links
//! stay relative, and because they are relative they keep working after
//! extraction. That only holds if entry names are relative to the document
//! folder and the folder's internal shape is reproduced exactly.

use std::io;
use std::path::Path;

use mdwisp_core::i18n::t;

use crate::zip::{ZipEntry, build_zip};
use crate::{Omitted, stem};

/// Directories that are never part of the document, whatever they contain.
const SKIP_DIRS: [&str; 3] = [".mdview", ".git", "node_modules"];

pub struct ZipPlan {
    pub entries: Vec<ZipEntry>,
    /// Paths that were deliberately left out, with a reason.
    pub skipped: Vec<Omitted>,
    pub bytes: usize,
}

pub fn archive_name_for(doc_path: &Path) -> String {
    format!("{}.zip", stem(doc_path))
}

/// Walk the document folder and collect what travels.
///
/// Everything that is not hidden comes along: images, other markdown in a
/// multi-document folder, and anything else the author put beside the file.
/// Filtering to known extensions would silently drop a PDF or a data file a
/// document links to, which is the same failure this mode exists to prevent.
pub fn plan_zip(doc_path: &Path) -> io::Result<ZipPlan> {
    let mut plan = ZipPlan { entries: Vec::new(), skipped: Vec::new(), bytes: 0 };
    walk(doc_path.parent().unwrap_or(Path::new("")), "", &mut plan)?;
    plan.entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plan)
}

fn walk(dir: &Path, prefix: &str, plan: &mut ZipPlan) -> io::Result<()> {
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        let file_name = item.file_name().to_string_lossy().into_owned();
        let abs = item.path();
        let name = format!("{prefix}{file_name}");
        let kind = item.file_type()?;

        if kind.is_dir() {
            if file_name.starts_with('.') || SKIP_DIRS.contains(&file_name.as_str()) {
                plan.skipped.push(Omitted { path: abs, reason: t("隐藏或工具目录，不属于文档").into() });
                continue;
            }
            plan.entries.push(ZipEntry { name: format!("{name}/"), data: Vec::new(), directory: true });
            walk(&abs, &format!("{name}/"), plan)?;
            continue;
        }

        if !kind.is_file() {
            plan.skipped.push(Omitted { path: abs, reason: t("不是普通文件").into() });
            continue;
        }

        let data = std::fs::read(&abs)?;
        plan.bytes += data.len();
        plan.entries.push(ZipEntry { name, data, directory: false });
    }
    Ok(())
}

pub fn run_zip(doc_path: &Path) -> io::Result<Vec<u8>> {
    Ok(build_zip(&plan_zip(doc_path)?.entries, chrono::Local::now().naive_local()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::tests::{now, read_zip};
    use std::fs;

    /// A real folder on disk, not a mock: the mode's entire job is to reproduce
    /// a directory's shape, and a mocked fs would let the walk and the entry
    /// naming disagree without anything noticing.
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let doc_dir = root.path().join("我的笔记");
        fs::create_dir_all(doc_dir.join("我的笔记_img")).unwrap();
        fs::write(doc_dir.join("我的笔记.md"), "# 笔记\n\n![图](./我的笔记_img/a.png)\n").unwrap();
        fs::write(doc_dir.join("我的笔记_img").join("a.png"), "PNG-BYTES").unwrap();
        let doc = doc_dir.join("我的笔记.md");
        (root, doc)
    }

    fn names(doc: &Path) -> Vec<String> {
        plan_zip(doc).unwrap().entries.into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn includes_the_markdown_and_its_image_folder_with_relative_names() {
        let (_root, doc) = fixture();
        let names = names(&doc);
        assert!(names.contains(&"我的笔记.md".into()));
        assert!(names.contains(&"我的笔记_img/a.png".into()));
        // Relative, never absolute: that is what keeps the links working.
        assert!(names.iter().all(|n| !n.contains(':') && !n.starts_with('/')));
    }

    #[test]
    fn writes_directory_entries_so_empty_folders_survive() {
        let (_root, doc) = fixture();
        let plan = plan_zip(&doc).unwrap();
        assert!(plan.entries.iter().find(|e| e.name == "我的笔记_img/").unwrap().directory);
    }

    #[test]
    fn includes_other_markdown_in_the_same_folder() {
        let (_root, doc) = fixture();
        fs::write(doc.parent().unwrap().join("另一篇.md"), "# 另一篇\n").unwrap();
        assert!(names(&doc).contains(&"另一篇.md".into()));
    }

    #[test]
    fn skips_hidden_and_tool_directories_and_says_so() {
        let (_root, doc) = fixture();
        for d in [".mdview", ".git", "node_modules"] {
            let dir = doc.parent().unwrap().join(d);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("junk.txt"), "junk").unwrap();
        }
        let plan = plan_zip(&doc).unwrap();
        for d in [".mdview", ".git", "node_modules"] {
            assert!(!plan.entries.iter().any(|e| e.name.contains(d)));
        }
        assert_eq!(plan.skipped.len(), 3);
        assert!(plan.skipped[0].reason.contains("不属于文档"));
    }

    #[test]
    fn produces_an_archive_that_reads_back_with_the_same_bytes() {
        let (_root, doc) = fixture();
        let read = read_zip(&run_zip(&doc).unwrap());
        let mut got: Vec<_> = read.iter().map(|e| e.name.clone()).collect();
        got.sort();
        assert_eq!(got, names(&doc));
        let get = |n: &str| String::from_utf8(read.iter().find(|e| e.name == n).unwrap().data.clone()).unwrap();
        assert!(get("我的笔记.md").contains("# 笔记"));
        assert_eq!(get("我的笔记_img/a.png"), "PNG-BYTES");
    }

    #[test]
    fn names_the_archive_after_the_document_stem() {
        let (_root, doc) = fixture();
        assert_eq!(archive_name_for(&doc), "我的笔记.zip");
    }

    #[test]
    fn builds_an_archive_from_an_empty_plan_without_throwing() {
        assert_eq!(build_zip(&[], now()).len(), 22);
    }
}
