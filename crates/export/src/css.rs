//! The stylesheet for a standalone document.
//!
//! `assets/` holds export's copy of the app sheets (`base.css` already cut down
//! to the document rules; KaTeX rules swapped for MathML ones) plus the
//! standalone shell. The CJK fonts are 22 MB, so they are not compiled into the
//! binary: they are read from `fonts/` beside the executable, or from this
//! crate's `assets/fonts` in development and tests.

use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;

const SHEETS: [&str; 5] = [
    include_str!("../assets/tokens.css"),
    include_str!("../assets/fonts.css"),
    include_str!("../assets/base.css"),
    include_str!("../assets/markdown.css"),
    include_str!("../assets/export.css"),
];

// The installer puts the fonts beside the exe; development builds read the crate's copy.
fn fonts_dir() -> PathBuf {
    let installed = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("fonts")));
    installed
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts")))
}

/// Inline every bundled font so the file renders the same on a machine without them.
fn embed_fonts(css: &str) -> io::Result<String> {
    let dir = fonts_dir();
    let re = Regex::new(r#"url\(['"]?\./fonts/([^)'"\s]+)['"]?\)"#).unwrap();
    let mut err = None;
    let out = re.replace_all(css, |c: &regex::Captures| {
        let name = &c[1];
        let mime = if name.ends_with(".otf") { "font/otf" } else { "font/ttf" };
        match std::fs::read(dir.join(name)) {
            Ok(bytes) => format!("url('data:{mime};base64,{}')", base64::engine::general_purpose::STANDARD.encode(bytes)),
            Err(e) => {
                err = Some(e);
                String::new()
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(out.into_owned()),
    }
}

/// Built once: base64 of the fonts is ~30 MB and identical for every export.
///
/// `tokens.css` defaults `:root` to dark and overrides it under
/// `[data-theme='light']`, which is exactly the switch a standalone file
/// needs: write the attribute on `<html>` for light, omit it for dark.
pub fn document_styles() -> io::Result<&'static str> {
    static CSS: OnceLock<String> = OnceLock::new();
    if let Some(css) = CSS.get() {
        return Ok(css);
    }
    let mut sheets = SHEETS.map(String::from);
    sheets[1] = embed_fonts(&sheets[1])?;
    Ok(CSS.get_or_init(|| sheets.join("\n")))
}
