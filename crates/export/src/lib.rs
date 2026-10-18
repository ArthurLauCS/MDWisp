//! Plain markdown, ZIP, single-file HTML and PDF export.

mod archive;
mod css;
mod html;
mod markdown;
mod pdf;
mod plain;
mod preview;
mod zip;

use std::path::{Path, PathBuf};

pub use archive::{ZipPlan, archive_name_for, plan_zip, run_zip};
pub use html::{HtmlBuild, build_html};
pub use pdf::render_pdf;
pub use plain::{ImagePolicy, preview_plain_md};
pub use preview::{preview_html, preview_pdf, preview_zip};
pub use zip::{ZipEntry, build_zip};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    /// Portable plain markdown: images and local paths stripped.
    PlainMd,
    /// Zip the whole document folder, paths untouched.
    Zip,
    /// Self-contained single HTML file with base64 assets.
    Html,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    /// 1-based line in the source for `plain-md`; a running index otherwise.
    pub line: usize,
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ExportPreview {
    pub mode: ExportMode,
    pub removals: Vec<Removal>,
    /// Final output text — only populated for `plain-md`.
    pub output: String,
    /// Absolute path the export would be written to.
    pub target_path: PathBuf,
}

/// Something deliberately or necessarily left out, with a reason for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omitted {
    pub path: PathBuf,
    pub reason: String,
}

/// File name without its markdown extension.
fn stem(doc_path: &Path) -> String {
    let name = doc_path.file_name().unwrap_or_default().to_string_lossy();
    match name.rfind('.') {
        Some(i) if mdwisp_core::paths::has_md_ext(&name) => name[..i].to_string(),
        _ => name.into_owned(),
    }
}

fn sibling(doc_path: &Path, name: &str) -> PathBuf {
    doc_path.parent().unwrap_or(Path::new("")).join(name)
}
