use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::documents::meta_for;
use crate::io_err;
use crate::organize::{ext, map_image_links};
use crate::paths::{decode_uri, has_md_ext, resolve, to_link};
use crate::settings::{AppSettings, ImageNaming};
use crate::workspace::is_asset_dir_name;

const IMAGE_EXT: [&str; 8] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".svg", ".bmp"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    /// Exactly as written in the markdown, e.g. `./Notes_img/a.png`.
    pub rel_path: String,
    /// Resolved against the document folder.
    pub abs_path: PathBuf,
    pub exists: bool,
    /// False when the link climbs out of the document folder.
    pub inside_doc_folder: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertedAsset {
    pub rel_path: String,
    pub abs_path: PathBuf,
    /// True when an identical file (sha256 match) already existed.
    pub reused: bool,
    pub bytes: usize,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Decodes `bytes` and, when wider than `max_width`, returns it scaled to that
/// width as PNG; None keeps the original. Image decoding lives in the app (WIC).
pub type Downscale<'a> = &'a dyn Fn(&[u8], u32) -> Option<Vec<u8>>;

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn is_image(name: &str) -> bool {
    IMAGE_EXT.contains(&ext(name).to_lowercase().as_str())
}

/// The stored filename for a new asset, per the user's naming preference.
fn name_for(naming: ImageNaming, stem: &str, hash: &str, ext: &str) -> String {
    match naming {
        ImageNaming::Original => format!("{stem}{ext}"),
        ImageNaming::Hash => format!("{}{ext}", &hash[..12]),
        ImageNaming::DateHashName => format!("{}-{}-{stem}{ext}", chrono::Local::now().format("%Y-%m-%d"), &hash[..4]),
    }
}

/// Existing images in the asset dir, keyed by content hash, for dedupe.
fn hash_index(asset_dir: &Path) -> Result<HashMap<String, String>, String> {
    let mut index = HashMap::new();
    let Ok(entries) = fs::read_dir(asset_dir) else { return Ok(index) };
    for entry in entries {
        let name = entry.map_err(io_err)?.file_name().to_string_lossy().into_owned();
        if is_image(&name) {
            index.insert(sha256(&fs::read(asset_dir.join(&name)).map_err(io_err)?), name);
        }
    }
    Ok(index)
}

/// Never overwrite. With naming set to `original` two photos called
/// `image.png` would otherwise silently become one, and the second insert
/// would point at the first one's bytes.
fn unique_name(name: String, index: &HashMap<String, String>) -> String {
    let taken = |n: &str| index.values().any(|v| v == n);
    if !taken(&name) {
        return name;
    }
    let e = ext(&name);
    let stem = &name[..name.len() - e.len()];
    (2..1000).map(|n| format!("{stem}-{n}{e}")).find(|c| !taken(c)).unwrap_or(name)
}

/// Write image bytes beside the document and return the link to insert.
/// Identical content is reused rather than duplicated.
pub fn insert(
    doc_path: &Path,
    mut data: Vec<u8>,
    original_name: Option<&str>,
    ext: &str,
    settings: &AppSettings,
    downscale: Downscale,
) -> Result<InsertedAsset, String> {
    let meta = meta_for(doc_path);
    fs::create_dir_all(&meta.asset_dir).map_err(io_err)?;

    // Downscale before hashing: the stored file is what the document points
    // at, so dedupe must key on the bytes that are actually written. SVG is
    // resolution independent and GIF would lose its animation.
    let mut ext = ext.to_string();
    let max = settings.image_max_width;
    if max > 0.0 && ext != ".svg" && ext != ".gif" {
        if let Some(png) = downscale(&data, max as u32) {
            data = png;
            ext = ".png".into();
        }
    }

    let hash = sha256(&data);
    let index = hash_index(&meta.asset_dir)?;
    let existing = if settings.image_dedupe { index.get(&hash) } else { None };
    let (abs_path, reused) = match existing {
        Some(name) => (meta.asset_dir.join(name), true),
        None => {
            let stem = match original_name.filter(|n| !n.is_empty()) {
                Some(n) => {
                    let base = n.rsplit(['/', '\\']).next().unwrap_or(n);
                    let base = base.strip_suffix(crate::organize::ext(base)).filter(|s| !s.is_empty()).unwrap_or(base);
                    base.chars().map(|c| if "\\/:*?\"<>|#%".contains(c) { '-' } else { c }).collect()
                }
                None => "paste".to_string(),
            };
            let abs = meta.asset_dir.join(unique_name(name_for(settings.image_naming, &stem, &hash, &ext), &index));
            fs::write(&abs, &data).map_err(io_err)?;
            (abs, false)
        }
    };
    Ok(InsertedAsset { rel_path: to_link(&meta.path, &abs_path)?, abs_path, reused, bytes: data.len(), width: None, height: None })
}

/// Copy an existing file (drag-drop from Explorer) into the asset folder.
pub fn insert_from_path(doc_path: &Path, src: &Path, settings: &AppSettings, downscale: Downscale) -> Result<InsertedAsset, String> {
    let data = fs::read(src).map_err(io_err)?;
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    insert(doc_path, data, Some(&name), &ext(&name).to_lowercase(), settings, downscale)
}

/// Dropped files, skipping markdown: dropping a document is opening it, not embedding it.
pub fn insert_files(doc_path: &Path, paths: &[PathBuf], settings: &AppSettings, downscale: Downscale) -> Result<Vec<InsertedAsset>, String> {
    paths
        .iter()
        .filter(|p| !p.to_string_lossy().to_lowercase().ends_with(".md"))
        .map(|p| insert_from_path(doc_path, p, settings, downscale))
        .collect()
}

fn is_web(target: &str) -> bool {
    target.starts_with("//") || target.starts_with("http://") || target.starts_with("https://")
}

/// Every image link in a document, resolved and checked for existence.
pub fn refs(doc_path: &Path, text: &str) -> Vec<AssetRef> {
    let meta = meta_for(doc_path);
    let mut targets: Vec<String> = vec![];
    map_image_links(text, |t| {
        if !targets.iter().any(|x| x == t) {
            targets.push(t.to_string());
        }
        None
    });
    targets
        .into_iter()
        .filter(|raw| !is_web(raw) && !raw.starts_with("data:"))
        .map(|raw| {
            let decoded = decode_uri(&raw);
            let abs_path = resolve(&meta.parent_dir, &decoded);
            AssetRef {
                inside_doc_folder: !decoded.replace('\\', "/").split('/').any(|s| s == ".."),
                exists: abs_path.exists(),
                abs_path,
                rel_path: raw,
            }
        })
        .collect()
}

/// Images in asset dirs under `root` that no document references; empty when the walk fails.
pub fn orphans(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, referenced: &mut Vec<PathBuf>, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let abs = dir.join(&name);
            if entry.file_type()?.is_dir() {
                if is_asset_dir_name(&name) {
                    for f in fs::read_dir(&abs)? {
                        let f = f?.file_name().to_string_lossy().into_owned();
                        if is_image(&f) {
                            files.push(abs.join(f));
                        }
                    }
                    continue;
                }
                walk(&abs, referenced, files)?;
            } else if has_md_ext(&name) && !name.to_lowercase().ends_with(".mdx") {
                let text = fs::read_to_string(&abs)?;
                map_image_links(&text, |t| {
                    if !is_web(&t.to_lowercase()) {
                        referenced.push(resolve(dir, decode_uri(t)));
                    }
                    None
                });
            }
        }
        Ok(())
    }
    let (mut referenced, mut files) = (vec![], vec![]);
    if walk(root, &mut referenced, &mut files).is_err() {
        return vec![];
    }
    files.retain(|f| !referenced.contains(f));
    files
}

/// Resolve a link written in markdown into an on-disk path.
pub fn resolve_link(doc_path: &Path, link: &str) -> PathBuf {
    resolve(doc_path.parent().unwrap_or(Path::new("")), decode_uri(link))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_fenced_and_inline_examples_while_retaining_actual_markdown_and_html_images() {
        let source = "![real](./real.png)\n\n```md\n![example](./example.png)\n```\n\n`![inline](./inline.png)`\n\n<img src=\"./html.png\">";
        let refs = refs(Path::new("notes/notes.md"), source);
        assert_eq!(refs.iter().map(|r| r.rel_path.as_str()).collect::<Vec<_>>(), ["./real.png", "./html.png"]);
    }

    #[test]
    fn dedupes_identical_bytes_and_never_overwrites_a_different_image() {
        let root = tempfile::tempdir().unwrap();
        let doc = root.path().join("N").join("N.md");
        fs::create_dir(root.path().join("N")).unwrap();
        let settings = AppSettings { image_naming: ImageNaming::Original, ..Default::default() };
        let keep: Downscale = &|_, _| None;
        let a = insert(&doc, b"one".to_vec(), Some("shot.png"), ".png", &settings, keep).unwrap();
        assert_eq!((a.rel_path.as_str(), a.reused), ("./N_img/shot.png", false));
        assert!(insert(&doc, b"one".to_vec(), Some("other.png"), ".png", &settings, keep).unwrap().reused);
        let b = insert(&doc, b"two".to_vec(), Some("C:\\x\\shot.png"), ".png", &settings, keep).unwrap();
        assert_eq!(b.rel_path, "./N_img/shot-2.png");
        let wide = AppSettings { image_max_width: 10.0, image_naming: ImageNaming::Hash, ..Default::default() };
        let c = insert(&doc, b"big".to_vec(), None, ".jpg", &wide, &|_, w| Some(format!("png{w}").into_bytes())).unwrap();
        assert_eq!(c.abs_path, doc.parent().unwrap().join("N_img").join(format!("{}.png", &sha256(b"png10")[..12])));
        assert_eq!(orphans(root.path()).len(), 3);
        fs::write(&doc, "![a](./N_img/shot.png)").unwrap();
        assert_eq!(orphans(root.path()).len(), 2);
    }
}
