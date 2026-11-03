//! Mermaid diagrams drawn as SVG in pure Rust, for the editor and the HTML export.

use mermaid_rs_renderer::{LayoutConfig, RenderOptions, Theme, render_with_options};

/// Diagram colours as CSS colour strings, taken from the page's theme tokens.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Palette {
    /// A dark page: pie and git series start from Mermaid's dark set.
    pub dark: bool,
    /// Behind edge labels and groups.
    pub page: String,
    /// Nodes and sequence actors.
    pub fill: String,
    pub border: String,
    /// Edges and lifelines.
    pub line: String,
    pub text: String,
    pub note: String,
    pub note_border: String,
}

/// The diagram as standalone SVG on a transparent background, labels in the
/// UI face (Microsoft YaHei UI covers Latin and CJK); None when the source
/// does not parse.
pub fn svg(code: &str, p: &Palette) -> Option<String> {
    let mut t = if p.dark { Theme::dark() } else { Theme::modern() };
    t.font_family = "Microsoft YaHei UI, Microsoft YaHei, sans-serif".into();
    t.font_size = 14.0;
    t.background = "none".into();
    (t.edge_label_background, t.cluster_background) = (p.page.clone(), p.page.clone());
    (t.primary_color, t.sequence_actor_fill) = (p.fill.clone(), p.fill.clone());
    for c in [&mut t.primary_border_color, &mut t.cluster_border, &mut t.sequence_actor_border, &mut t.sequence_activation_border, &mut t.pie_outer_stroke_color] {
        *c = p.border.clone();
    }
    for c in [&mut t.line_color, &mut t.sequence_actor_line, &mut t.pie_stroke_color] {
        *c = p.line.clone();
    }
    for c in [&mut t.primary_text_color, &mut t.text_color, &mut t.pie_title_text_color, &mut t.pie_legend_text_color] {
        *c = p.text.clone();
    }
    (t.sequence_note_fill, t.sequence_note_border) = (p.note.clone(), p.note_border.clone());
    let opts = RenderOptions { theme: t, layout: LayoutConfig::default() };
    // A renderer bug on odd input must not take the editor or an export down with it.
    std::panic::catch_unwind(|| render_with_options(code, opts).ok()).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let c = |s: &str| s.to_string();
        Palette { dark: false, page: c("#fdfdfb"), fill: c("#f5f5f0"), border: c("#b0b0aa"), line: c("#45453f"), text: c("#111111"), note: c("#eeeaf5"), note_border: c("#6a4c9c") }
    }

    #[test]
    fn draws_in_the_palette_and_rejects_what_does_not_parse() {
        let out = svg("graph TD\n  A[开始] --> B[结束]\n", &palette()).unwrap();
        assert!(out.starts_with("<svg") && out.contains("开始") && out.contains("#f5f5f0") && out.contains("Microsoft YaHei UI"));
        // The parser is lenient inside a diagram; only a missing diagram type fails.
        assert!(svg("hello world", &palette()).is_none());
    }
}
