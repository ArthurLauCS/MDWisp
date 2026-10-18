//! Render the standalone HTML to PDF by printing it with headless Microsoft
//! Edge, which ships with every Windows 10/11 — the app itself carries no
//! browser engine any more.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mdwisp_core::i18n::t;

use crate::html::{HtmlBuild, build_html};
use crate::Theme;

const TIMEOUT: Duration = Duration::from_secs(90);

/// A file URL must not let document HTML fetch other files from this machine.
/// `default-src 'none'` also keeps scripts off; `--blink-settings=scriptEnabled=false`
/// would too, but with it headless Edge exits without printing.
const POLICY: &str = "default-src 'none'; img-src data: https: http:; style-src 'unsafe-inline'; font-src data:";

/// A4 with the margins (inches) the Electron build passed to `printToPDF`;
/// backgrounds are forced on, as `printBackground: true` did.
const PAGE: &str = "@page { size: A4; margin: 0.6in 0.5in; } html { print-color-adjust: exact; -webkit-print-color-adjust: exact; }";

fn find_edge() -> Option<PathBuf> {
    let app_path = windows_registry::LOCAL_MACHINE
        .open(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe")
        .and_then(|k| k.get_string(""))
        .ok()
        .map(PathBuf::from);
    let installed = ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(|v| std::env::var_os(v))
        .map(|dir| Path::new(&dir).join(r"Microsoft\Edge\Application\msedge.exe"));
    app_path.into_iter().chain(installed).find(|p| p.is_file())
}

pub fn render_pdf(doc_path: &Path, source: &str, theme: Theme) -> io::Result<(Vec<u8>, HtmlBuild)> {
    let build = build_html(doc_path, source, theme)?;
    let edge = find_edge().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t("找不到 Microsoft Edge，无法导出 PDF")))?;

    // Embedded fonts and images exceed the browser's URL size limit, so the page goes through a file.
    // Dropping the dir ignores errors: Edge can hold its profile a moment after exiting.
    let temp = tempfile::Builder::new().prefix("mdwisp-pdf-").tempdir()?;
    Ok((print(&edge, temp.path(), &build.html)?, build))
}

fn print(edge: &Path, temp: &Path, html: &str) -> io::Result<Vec<u8>> {
    let page = temp.join("document.html");
    let out = temp.join("document.pdf");
    let head = format!(r#"<head><meta http-equiv="Content-Security-Policy" content="{POLICY}"><style>{PAGE}</style>"#);
    std::fs::write(&page, html.replacen("<head>", &head, 1))?;

    let mut child = Command::new(edge)
        .args(["--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--disable-extensions"])
        .arg("--no-pdf-header-footer")
        .arg(format!("--user-data-dir={}", temp.join("profile").display()))
        .arg(format!("--print-to-pdf={}", out.display()))
        .arg(&page)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let start = Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            return Err(io::Error::new(io::ErrorKind::TimedOut, t("PDF 导出超时")));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::read(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "launches Microsoft Edge"]
    fn prints_a_document_with_edge() {
        let root = tempfile::tempdir().unwrap();
        let doc = root.path().join("样例.md");
        let source = "# 标题\n\n## 小节[^1]\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n$$\n\\frac{a}{b} = \\sum_{i=0}^n x_i\n$$\n\n[^1]: 注释\n\n正文 ==重点== $a^2+b^2=c^2$\n\n```rust\nfn main() {}\n```\n\n- [x] 完成\n";
        let (pdf, build) = render_pdf(&doc, source, Theme::Light).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(build.missing.is_empty());
        std::fs::write(root.path().join("样例.pdf"), &pdf).unwrap();
    }
}
