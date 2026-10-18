use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::{has_md_ext, resolve};

/// The document (and heading) a new window opens on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowLaunch {
    pub path: Option<PathBuf>,
    pub fragment: String,
}

/// The document a launch was asked to open.
///
/// Explorer passes a double-clicked file as an argument, but its position is
/// not fixed: a development launch puts other arguments first. So the
/// extension is the test, not the index.
pub fn document_arg(argv: &[String], cwd: &Path) -> Option<PathBuf> {
    argv.iter().skip(1).rev().find(|a| has_md_ext(a)).map(|a| resolve(cwd, a))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn finds_the_document_among_switches_wherever_it_sits() {
        assert_eq!(document_arg(&args(&["MDWisp.exe", "--allow-file-access", "D:/笔记/a.md"]), Path::new("C:/")), Some(PathBuf::from(r"D:\笔记\a.md")));
        assert_eq!(document_arg(&args(&["electron.exe", ".", "docs/b.markdown", "--flag"]), Path::new("C:/repo")), Some(PathBuf::from(r"C:\repo\docs\b.markdown")));
    }

    #[test]
    fn ignores_a_launch_without_a_document_and_the_executable_itself() {
        assert_eq!(document_arg(&args(&["MDWisp.exe"]), Path::new("C:/")), None);
        assert_eq!(document_arg(&args(&["C:/tools/viewer.md", "--flag"]), Path::new("C:/")), None);
    }
}
