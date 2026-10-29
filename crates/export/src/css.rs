//! The stylesheet for a standalone document.
//!
//! `assets/` holds export's copy of the app sheets (`base.css` already cut down
//! to the document rules; KaTeX rules swapped for MathML ones) plus the
//! standalone shell. The fonts are 51 MB, so they are not compiled into the
//! binary: they are read from `fonts/` beside the executable, or from this
//! crate's `assets/fonts` in development and tests. The two CJK faces are cut
//! down to the characters the document uses (a few hundred KB instead of 25 MB
//! each); a font file that is missing leaves its stack on the Windows faces.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use base64::Engine;
use regex::Regex;

const SHEETS: [&str; 5] = [
    include_str!("../assets/tokens.css"),
    include_str!("../assets/fonts.css"),
    include_str!("../assets/base.css"),
    include_str!("../assets/markdown.css"),
    include_str!("../assets/export.css"),
];

/// The faces too big to embed whole.
const SUBSET: [&str; 2] = ["NotoSerifSC-VF.ttf", "LXGWWenKai-Regular.ttf"];

// The installer puts the fonts beside the exe; development builds read the crate's copy.
fn fonts_dir() -> PathBuf {
    let installed = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join("fonts")));
    installed
        .filter(|d| d.join(SUBSET[0]).is_file())
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts")))
}

/// Inline the bundled fonts so the file renders the same on a machine without them.
fn embed_fonts(css: &str, chars: &HashSet<char>) -> String {
    let dir = fonts_dir();
    let re = Regex::new(r#"url\(['"]?\./fonts/([^)'"\s]+)['"]?\)\s*format\([^)]*\)"#).unwrap();
    re.replace_all(css, |c: &regex::Captures| {
        let name = &c[1];
        let mime = if name.ends_with(".otf") { "font/otf" } else { "font/ttf" };
        let bytes = std::fs::read(dir.join(name)).ok().and_then(|bytes| {
            if SUBSET.contains(&name) { fontcull::subset_font_data(&bytes, chars, &[]).ok() } else { Some(bytes) }
        });
        match bytes {
            Some(bytes) => format!("url('data:{mime};base64,{}')", base64::engine::general_purpose::STANDARD.encode(bytes)),
            // A valid source that matches nothing, so the next one in the list is used.
            None => "local('__missing')".to_string(),
        }
    })
    .into_owned()
}

/// `tokens.css` defaults `:root` to dark and overrides it under
/// `[data-theme='light']`, which is exactly the switch a standalone file
/// needs: write the attribute on `<html>` for light, omit it for dark.
/// `text` is the document's markdown: its characters decide what the CJK faces keep.
pub fn document_styles(text: &str) -> io::Result<String> {
    let mut chars: HashSet<char> = text.chars().filter(|c| !c.is_control()).collect();
    // Punctuation a heading anchor, list marker or table may add around the text.
    chars.extend(" .,:;!?()[]“”‘’、。，：；！？（）—…·".chars());
    let mut sheets = SHEETS.map(String::from);
    sheets[1] = embed_fonts(&sheets[1], &chars);
    Ok(sheets.join("\n"))
}

#[cfg(test)]
pub fn fonts_fetched() -> bool {
    SUBSET.iter().all(|f| fonts_dir().join(f).is_file())
}
