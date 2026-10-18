//! Strip everything that would break the document on someone else's machine:
//! image refs, absolute paths, local-only HTML media, and our own markers.
//!
//! Deliberately line-by-line rather than a markdown AST round-trip — a rewrite
//! through a renderer would reflow the author's formatting and lose intent.

use std::path::Path;
use std::sync::OnceLock;

use mdwisp_core::i18n::{t, tf};
use mdwisp_core::paths::is_portable_link;
use regex::Regex;

use crate::{ExportMode, ExportPreview, Removal, sibling, stem};

/// Which placeholder to leave behind for a local image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImagePolicy {
    Drop,
    #[default]
    AltPlaceholder,
    EmptyRef,
}

struct Patterns {
    private_key: Regex,
    fence: Regex,
    marker: Regex,
    media: Regex,
    src: Regex,
    image: Regex,
    img_tag: Regex,
    link: Regex,
    bare_path: Regex,
    file_url: Regex,
    drive_dirs: Regex,
    spaces: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let re = |s: &str| Regex::new(s).unwrap();
        Patterns {
            private_key: re(r"(?i)^mdview:"),
            fence: re(r"^\s*(```|~~~)"),
            marker: re(r"(?i)<!--\s*mdview:[\s\S]*?-->"),
            media: re(r"(?i)<(video|audio|source|iframe)\b"),
            src: re(r#"(?i)src=["']([^"']+)["']"#),
            image: re(r#"!\[([^\]]*)\]\(([^)\s]+)(\s+"[^"]*")?\)"#),
            img_tag: re(r"(?i)<img\b"),
            link: re(r#"\[([^\]]*)\]\(([^)\s]+)(\s+"[^"]*")?\)"#),
            bare_path: re(r"(?i)[A-Za-z]:\\\\?[^ \n]*\\|file:///"),
            file_url: re(r"(?i)file:///\S+"),
            drive_dirs: re(r"[A-Za-z]:\\(?:[^\\\s]+\\)+"),
            spaces: re(r" {2,}"),
        }
    })
}

/// Remote and inline images travel with the document; local ones do not.
fn is_portable_image(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("//") || lower.starts_with("http://") || lower.starts_with("https://") || target.starts_with("data:")
}

fn local_src(p: &Patterns, line: &str) -> bool {
    p.src.captures(line).is_none_or(|c| !is_portable_link(&c[1]))
}

pub fn preview_plain_md(doc_path: &Path, text: &str, policy: ImagePolicy) -> ExportPreview {
    let p = patterns();
    let mut removals = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut in_frontmatter = false;
    // The opening fence token while inside a code block.
    let mut fence: Option<String> = None;
    let mut remove = |line: usize, text: &str, reason: &str| removals.push(Removal { line, text: text.into(), reason: reason.into() });

    for (i, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let no = i + 1;
        let trimmed = line.trim();

        // Frontmatter block at the very top: drop mdview-private keys, keep the rest.
        if i == 0 && trimmed == "---" {
            in_frontmatter = true;
            kept.push(line.into());
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            } else if p.private_key.is_match(trimmed) {
                remove(no, line, t("本地私有状态"));
                continue;
            }
            kept.push(line.into());
            continue;
        }

        // Fenced content is sample text, not document structure. An image link
        // inside a fence is there to be *read*, so rewriting or removing it would
        // corrupt the example — leave everything between fences untouched.
        if let Some(m) = p.fence.captures(line) {
            match &fence {
                None => fence = Some(m[1].into()),
                Some(open) if *open == m[1] => fence = None,
                Some(_) => {}
            }
            kept.push(line.into());
            continue;
        }
        if fence.is_some() {
            kept.push(line.into());
            continue;
        }

        // Our own inline markers.
        if p.marker.is_match(line) {
            let stripped = p.marker.replace_all(line, "");
            remove(no, line, t("编辑器标记"));
            if !stripped.trim_end().is_empty() {
                kept.push(stripped.trim_end().into());
            }
            continue;
        }

        // Local HTML media — video/audio/iframe pointing at a file on this disk.
        if p.media.is_match(line) && local_src(p, line) {
            remove(no, line, t("本地媒体引用"));
            continue;
        }

        // Markdown images. A line can hold several, and only the ones pointing at
        // this disk are touched — a remote or inline image travels perfectly well.
        let images: Vec<_> = p.image.captures_iter(line).filter(|m| !is_portable_image(&m[2])).collect();
        if !images.is_empty() {
            // Dropping the line only makes sense when the images are all it holds.
            if policy == ImagePolicy::Drop && p.image.replace_all(line, "").trim().is_empty() {
                remove(no, line, t("图片引用"));
                continue;
            }
            let mut next = line.to_string();
            for m in &images {
                let replacement = match policy {
                    ImagePolicy::Drop => String::new(),
                    ImagePolicy::AltPlaceholder if m[1].is_empty() => t("*[图片]*").into(),
                    ImagePolicy::AltPlaceholder => tf("*[图：{0}]*", &[&&m[1]]),
                    ImagePolicy::EmptyRef => "![]()".into(),
                };
                next = next.replace(&m[0], &replacement);
            }
            remove(no, line, t("本地图片引用"));
            kept.push(p.spaces.replace_all(&next, " ").trim_end().into());
            continue;
        }

        // HTML <img> tags: drop entirely when they carry a local path.
        if p.img_tag.is_match(line) && local_src(p, line) {
            remove(no, line, t("本地图片标签"));
            continue;
        }

        // Link targets that point at this machine. `./other.md` is fine — portable.
        let mut mutated = line.to_string();
        let mut touched = false;
        for m in p.link.captures_iter(line) {
            let target = &m[2];
            if target.starts_with('#') || is_portable_link(target) {
                continue;
            }
            mutated = mutated.replace(&m[0], &m[1]);
            touched = true;
        }
        if touched {
            remove(no, line, t("本地绝对路径"));
            kept.push(mutated);
            continue;
        }

        // Bare absolute paths left in prose.
        if p.bare_path.is_match(line) {
            remove(no, line, t("本地路径"));
            let without_urls = p.file_url.replace_all(line, "");
            kept.push(p.drive_dirs.replace_all(&without_urls, "").into());
            continue;
        }

        kept.push(line.into());
    }

    ExportPreview {
        mode: ExportMode::PlainMd,
        removals,
        output: kept.join("\n"),
        target_path: sibling(doc_path, &format!("{}.plain.md", stem(doc_path))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r"C:\notes\我的笔记\我的笔记.md";

    fn run_with(text: &str, policy: ImagePolicy) -> String {
        preview_plain_md(Path::new(DOC), text, policy).output
    }

    fn run(text: &str) -> String {
        run_with(text, ImagePolicy::default())
    }

    fn reasons(text: &str) -> Vec<String> {
        preview_plain_md(Path::new(DOC), text, ImagePolicy::default()).removals.into_iter().map(|r| r.reason).collect()
    }

    // ---- removes what would break on another machine

    #[test]
    fn strips_a_relative_image_link() {
        let out = run("before\n![图](./我的笔记_img/a.png)\nafter");
        assert!(!out.contains("我的笔记_img/a.png"));
        assert!(out.contains("before") && out.contains("after"));
    }

    #[test]
    fn strips_an_absolute_windows_image_path() {
        assert!(!run(r"![本地](C:\Users\me\Pictures\a.png)").contains(r"C:\Users"));
    }

    #[test]
    fn keeps_the_link_text_but_drops_a_local_file_link() {
        let out = run("见 [设计文档](D:/work/spec.md) 那篇");
        assert!(out.contains("设计文档"));
        assert!(!out.contains("D:/work/spec.md"));
    }

    #[test]
    fn drops_an_img_tag_pointing_at_file_url() {
        assert!(!run(r#"<img src="file:///C:/a.png" width="600">"#).contains("file://"));
    }

    #[test]
    fn removes_our_own_inline_markers() {
        let out = run("正文 <!-- mdview:cursor=42 --> 后续");
        assert!(!out.contains("mdview:"));
        assert!(out.contains("正文") && out.contains("后续"));
    }

    #[test]
    fn drops_the_private_frontmatter_keys_and_keeps_the_rest() {
        let out = run("---\ntitle: 笔记\ntags: [a]\nmdview: { cursor: 10 }\n---\n\n正文");
        assert!(out.contains("title: 笔记") && out.contains("tags: [a]"));
        assert!(!out.contains("mdview:"));
    }

    // ---- keeps what is portable

    #[test]
    fn leaves_a_remote_image_alone() {
        let src = "![图](https://example.com/a.png)";
        assert_eq!(run(src), src);
    }

    #[test]
    fn leaves_a_data_uri_alone() {
        let src = "![图](data:image/png;base64,AAAA)";
        assert_eq!(run(src), src);
    }

    #[test]
    fn leaves_a_relative_link_to_another_document() {
        let src = "见 [另一篇](./另一篇.md)";
        assert_eq!(run(src), src);
    }

    #[test]
    fn leaves_an_anchor_link_alone() {
        let src = "见 [本节](#section)";
        assert_eq!(run(src), src);
    }

    // ---- fenced code is sample text, not structure

    #[test]
    fn does_not_rewrite_an_image_link_inside_a_fence() {
        let src = "```markdown\n![示例](./assets/example.png)\n```";
        assert_eq!(run(src), src);
        assert!(reasons(src).is_empty());
    }

    #[test]
    fn does_not_rewrite_a_windows_path_inside_a_fence() {
        let src = "```\nC:\\Users\\me\\file.txt\n```";
        assert_eq!(run(src), src);
    }

    #[test]
    fn resumes_scrubbing_after_the_fence_closes() {
        let out = run("```\n![inside](./a.png)\n```\n![outside](./b.png)");
        assert!(out.contains("![inside](./a.png)"));
        assert!(!out.contains("b.png"));
    }

    #[test]
    fn handles_tilde_fences_too() {
        let src = "~~~\n![示例](./a.png)\n~~~";
        assert_eq!(run(src), src);
    }

    // ---- image policy

    #[test]
    fn replaces_the_link_with_an_italic_alt_placeholder_by_default() {
        assert_eq!(run_with("![架构图](./x_img/a.png)", ImagePolicy::AltPlaceholder), "*[图：架构图]*");
    }

    #[test]
    fn falls_back_to_a_bare_label_when_there_is_no_alt() {
        assert_eq!(run_with("![](./x_img/a.png)", ImagePolicy::AltPlaceholder), "*[图片]*");
    }

    #[test]
    fn removes_the_whole_line_when_the_policy_says_drop() {
        let out = run_with("before\n![图](./x_img/a.png)\nafter", ImagePolicy::Drop);
        assert_eq!(out.split('\n').collect::<Vec<_>>(), ["before", "after"]);
    }

    #[test]
    fn leaves_an_empty_reference_when_the_policy_says_so() {
        assert_eq!(run_with("![图](./x_img/a.png)", ImagePolicy::EmptyRef), "![]()");
    }

    #[test]
    fn replaces_every_occurrence_on_the_line_not_just_the_first() {
        let out = run_with("![a](./x_img/a.png) 和 ![b](./x_img/b.png)", ImagePolicy::AltPlaceholder);
        assert_eq!(out, "*[图：a]* 和 *[图：b]*");
    }

    // ---- reports what it removed

    #[test]
    fn records_the_line_number_and_a_reason_for_each_removal() {
        let preview = preview_plain_md(Path::new(DOC), "line one\n![图](./x_img/a.png)\nline three", ImagePolicy::default());
        assert_eq!(preview.removals.len(), 1);
        assert_eq!(preview.removals[0].line, 2);
        assert!(preview.removals[0].reason.contains("图片"));
    }

    #[test]
    fn suggests_a_sibling_plain_md_target() {
        let target = preview_plain_md(Path::new(DOC), "x", ImagePolicy::default()).target_path;
        assert!(target.to_string_lossy().contains("我的笔记.plain.md"));
    }

    #[test]
    fn reports_nothing_for_a_document_that_is_already_portable() {
        assert!(reasons("# 标题\n\n正文，含 [外链](https://example.com) 一个。").is_empty());
    }
}
