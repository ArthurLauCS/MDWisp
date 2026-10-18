//! Previews for the three packaged export modes.
//!
//! `plain-md` reports what it strips from one file; these modes package files
//! that are not modified at all, so the useful thing to report is what did and
//! did not make it in. A preview must stay pure: no save dialog, no window, no
//! write. `pdf` in particular cannot report a page count without printing the
//! document, which would make previewing a document a side-effecting act.

use std::io;
use std::path::Path;

use mdwisp_core::i18n::{t, tf};

use crate::archive::{archive_name_for, plan_zip};
use crate::html::build_html;
use crate::{ExportMode, ExportPreview, Omitted, Removal, Theme, sibling, stem};

fn human_bytes(n: usize) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / 1024.0 / 1024.0),
    }
}

fn removals(omitted: &[Omitted]) -> Vec<Removal> {
    omitted
        .iter()
        .enumerate()
        .map(|(i, o)| Removal {
            line: i + 1,
            text: o.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            reason: o.reason.clone(),
        })
        .collect()
}

/// `tf` for the optional clauses: present only when `n > 0`.
fn when(n: usize, key: &str) -> String {
    if n > 0 { tf(key, &[&n]) } else { String::new() }
}

pub fn preview_zip(doc_path: &Path) -> io::Result<ExportPreview> {
    let plan = plan_zip(doc_path)?;
    let dirs = plan.entries.iter().filter(|e| e.directory).count();
    let files = plan.entries.len() - dirs;
    let removals = removals(&plan.skipped);

    let output = tf("将打包 {0} 个文件", &[&files])
        + &when(dirs, "、{0} 个文件夹")
        + &tf("，共 {0}。", &[&human_bytes(plan.bytes)])
        + t("文档内的相对链接不会改动，解压后即可直接打开。")
        + &when(removals.len(), "另有 {0} 项未包含。");

    Ok(ExportPreview { mode: ExportMode::Zip, removals, output, target_path: sibling(doc_path, &archive_name_for(doc_path)) })
}

pub fn preview_html(doc_path: &Path, text: &str, theme: Theme) -> io::Result<ExportPreview> {
    let build = build_html(doc_path, text, theme)?;
    let removals = removals(&build.missing);

    let output = tf("将生成一个自包含的 HTML 文件，约 {0}。", &[&human_bytes(build.bytes)])
        + &tf("已内嵌 {0} 张本地图片", &[&build.inlined.len()])
        + &when(build.external.len(), "，保留 {0} 个网络图片地址")
        + t("。打开时不产生任何网络请求。")
        + &when(removals.len(), "有 {0} 张本地图片读不到，将显示占位符。");

    Ok(ExportPreview { mode: ExportMode::Html, removals, output, target_path: sibling(doc_path, &format!("{}.html", stem(doc_path))) })
}

/// The PDF preview reuses the HTML build so the reported cost is the real one.
pub fn preview_pdf(doc_path: &Path, text: &str, theme: Theme) -> io::Result<ExportPreview> {
    let build = build_html(doc_path, text, theme)?;
    let removals = removals(&build.missing);

    let output = tf("将按 A4 排版输出 PDF，约 {0} 的 HTML 内容。", &[&human_bytes(build.bytes)])
        + &tf("已内嵌 {0} 张本地图片。", &[&build.inlined.len()])
        + t("分页由排版决定，导出后才能确定页数。")
        + &when(removals.len(), "有 {0} 张本地图片读不到，将显示占位符。");

    Ok(ExportPreview { mode: ExportMode::Pdf, removals, output, target_path: sibling(doc_path, &format!("{}.pdf", stem(doc_path))) })
}
