//! Source lines → shaped visual lines. Each line keeps its own DirectWrite
//! layout and a map between source bytes and displayed UTF-16 positions, so
//! hidden markup costs nothing on screen and the caret skips over it.
//! Layouts are cached by content, so a keystroke reshapes one line.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use windows::Win32::Graphics::DirectWrite::*;
use windows::core::Interface;

use super::deco::{Code, Deco, LineDeco, Style as Mark, Widget};
use crate::gfx::{Gfx, Stack, Style, metrics, spacer};
use crate::theme::{Color, Theme};

/// Settings that change how lines are shaped.
#[derive(Clone, PartialEq, Hash, Default)]
pub struct Look {
    pub read: u32,
    pub line_height: u32,
    pub code: u32,
    pub tab: usize,
    pub font_ui: Option<String>,
    pub font_read: Option<String>,
    pub font_code: Option<String>,
    pub font_display: Option<String>,
    pub source: bool,
    /// Theme colours are baked into layouts as drawing effects.
    pub theme: u64,
}

impl Look {
    pub fn size(&self) -> f32 {
        f32::from_bits(self.read)
    }
    pub fn lh(&self) -> f32 {
        f32::from_bits(self.line_height)
    }
    pub fn code_size(&self) -> f32 {
        f32::from_bits(self.code)
    }
    fn family(&self, stack: Stack) -> Option<&str> {
        match stack {
            Stack::Ui => self.font_ui.as_deref(),
            Stack::Read => self.font_read.as_deref(),
            Stack::Code => self.font_code.as_deref(),
            Stack::Display => self.font_display.as_deref(),
        }
    }
}

/// A contiguous piece of a line: visible text, or one widget's source.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub src: usize,
    pub src_len: usize,
    pub disp: u32,
    pub disp_len: u32,
    pub text: bool,
}

#[derive(Clone, Debug)]
pub enum Obj {
    Checkbox { at: usize, checked: bool },
    Image { src: String, alt: String, range: Range<usize> },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Bg {
    Code,
    Mark,
}

/// What a visual line is made of; geometry is filled in by the view.
#[derive(Clone)]
pub struct Shaped {
    pub layout: Option<IDWriteTextLayout>,
    pub segs: Vec<Seg>,
    /// The hanging list marker, drawn left of the text box.
    pub marker: Option<(IDWriteTextLayout, usize)>,
    pub objects: Vec<(u32, Obj)>,
    pub backgrounds: Vec<(Range<u32>, Bg)>,
    /// Text box offset and padding inside the line box.
    pub indent: f32,
    pub pad_top: f32,
    pub pad_bottom: f32,
    /// Height of the text box (or a fixed box for header/end lines).
    pub text_h: f32,
    pub width: f32,
}

impl Shaped {
    pub fn empty() -> Shaped {
        Shaped { layout: None, segs: vec![], marker: None, objects: vec![], backgrounds: vec![], indent: 0.0, pad_top: 0.0, pad_bottom: 0.0, text_h: 0.0, width: 0.0 }
    }

    /// The same line moved from source offset `from` to `to`: the cache is keyed
    /// by content, and an edit above a line shifts it without changing it.
    fn moved(mut self, from: usize, to: usize) -> Shaped {
        if from != to {
            let m = |p: &mut usize| *p = *p - from + to;
            for s in &mut self.segs {
                m(&mut s.src);
            }
            for (_, o) in &mut self.objects {
                match o {
                    Obj::Checkbox { at, .. } => m(at),
                    Obj::Image { range, .. } => {
                        m(&mut range.start);
                        m(&mut range.end);
                    }
                }
            }
            if let Some((_, end)) = &mut self.marker {
                m(end);
            }
        }
        self
    }
}

pub struct Line {
    pub src: Range<usize>,
    pub deco: LineDeco,
    pub table: Option<usize>,
    pub shaped: Shaped,
    pub y: f32,
    pub h: f32,
}

pub struct Cache {
    /// Shaped lines by content, with the source offset they were shaped at.
    map: HashMap<u64, (usize, Shaped)>,
}

impl Cache {
    pub fn new() -> Cache {
        Cache { map: HashMap::new() }
    }
}

/// Typography of a line, from `markdown.css` / `editor.css`.
pub struct LineStyle {
    pub style: Style,
    pub lh: f32,
    pub color: Color,
    pub pad_top: f32,
    pub pad_bottom: f32,
    pub indent: f32,
    pub right: f32,
    pub tracking: f32,
    pub upper: bool,
    pub center: bool,
}

pub fn line_style(d: &LineDeco, look: &Look, t: &Theme) -> LineStyle {
    let size = look.size();
    let lh = look.lh();
    let mut s = LineStyle {
        style: Style::new(Stack::Read, size),
        lh: lh * size,
        color: t.text_primary,
        pad_top: 0.0,
        pad_bottom: 0.0,
        indent: 0.0,
        right: 0.0,
        tracking: 0.0,
        upper: false,
        center: false,
    };
    if look.source {
        s.style = Style::new(Stack::Code, look.code_size());
        s.lh = lh * look.code_size();
        return s;
    }
    if d.frontmatter {
        s.style = Style::new(Stack::Code, crate::theme::TEXT_SM);
        s.lh = lh * crate::theme::TEXT_SM;
        s.color = t.text_tertiary;
        return s;
    }
    if d.table > 0 {
        let size = size * 0.92;
        s.style = Style::new(Stack::Ui, size).weight(if d.table == 1 { 620 } else { 400 });
        s.lh = size * 1.55;
        return s;
    }
    if d.code != Code::None {
        s.style = Style::new(Stack::Code, look.code_size());
        s.lh = lh * look.code_size();
        s.color = t.text_secondary;
        s.indent = 16.0;
        s.right = 16.0;
        match d.code {
            Code::Header => s.lh = 32.0,
            Code::End => s.lh = 12.0,
            _ => {}
        }
        return s;
    }
    if d.para_start {
        s.pad_top = 8.0;
    }
    if d.para_end {
        s.pad_bottom = 8.0;
    }
    match d.heading {
        1 => {
            s.style = Style::new(Stack::Display, size * 2.15).weight(500);
            s.lh = size * 2.15 * 1.16;
            (s.pad_top, s.pad_bottom) = (24.0, 24.0);
            s.tracking = -0.018 * size * 2.15;
        }
        2 => {
            s.style = Style::new(Stack::Read, size * 1.55).weight(600);
            s.lh = size * 1.55 * 1.24;
            (s.pad_top, s.pad_bottom) = (36.0, 16.0);
            s.tracking = -0.008 * size * 1.55;
        }
        3 => {
            s.style = Style::new(Stack::Ui, size * 1.2).weight(640);
            s.lh = size * 1.2 * 1.32;
            (s.pad_top, s.pad_bottom) = (28.0, 12.0);
            s.tracking = -0.004 * size * 1.2;
        }
        4 => {
            s.style = Style::new(Stack::Ui, size * 1.02).weight(640);
            s.lh = lh * size * 1.02;
            (s.pad_top, s.pad_bottom) = (24.0, 8.0);
            s.tracking = 0.012 * size * 1.02;
        }
        5 => {
            s.style = Style::new(Stack::Ui, size * 0.84).weight(600);
            s.lh = lh * size * 0.84;
            (s.pad_top, s.pad_bottom) = (24.0, 8.0);
            s.color = t.text_secondary;
            s.upper = true;
            s.tracking = 0.09 * size * 0.84;
            s.indent = 12.0 + 2.0;
        }
        6 => {
            s.style = Style::new(Stack::Code, size * 0.78).weight(500);
            s.lh = lh * size * 0.78;
            (s.pad_top, s.pad_bottom) = (24.0, 8.0);
            s.color = t.text_tertiary;
            s.upper = true;
            s.tracking = 0.09 * size * 0.78;
        }
        _ => {}
    }
    if d.heading == 1 {
        // The plate pulls left by its own padding; text keeps the column's margin.
        s.indent = 0.0;
    }
    if d.list > 0 {
        s.indent += d.list as f32 * 1.5 * size + 1.35 * size;
        s.pad_top = s.pad_top.max(4.0);
        s.pad_bottom = s.pad_bottom.max(4.0);
    }
    if d.quote {
        s.indent += 2.0 + 16.0;
        s.color = t.text_secondary;
    }
    if d.blank {
        s.lh = 12.0;
    }
    if d.image {
        s.center = true;
        (s.pad_top, s.pad_bottom) = (24.0, 24.0);
    }
    if d.rule {
        (s.pad_top, s.pad_bottom) = (24.0, 24.0);
        s.lh = 1.0;
    }
    s
}

/// Image size hint: (natural width, natural height) once decoded.
pub type ImageSize<'a> = &'a dyn Fn(&str) -> Option<(f32, f32)>;

/// Shape one source line (or the whole range of a block) into a layout.
pub fn shape(g: &Gfx, cache: &mut Cache, src: &str, deco: &Deco, line: Range<usize>, ld: &LineDeco, width: f32, look: &Look, t: &Theme, images: ImageSize) -> Shaped {
    shape_line(g, cache, src, deco, line, ld, width, look, t, images, false).unwrap()
}

/// Rough height of a line that has not been shaped yet: its characters
/// spread over the text width. Refined once the line really is shaped.
pub fn estimate(src: &str, line: &Range<usize>, ld: &LineDeco, width: f32, look: &Look, t: &Theme) -> f32 {
    let ls = line_style(ld, look, t);
    let text = &src[line.clone()];
    let advance: f32 = text.chars().map(|c| if (c as u32) < 0x2e80 { 0.55 } else { 1.0 }).sum::<f32>() * ls.style.size;
    let rows = (advance / (width - ls.indent - ls.right).max(1.0)).ceil().max(1.0);
    let image = if ld.image { 200.0 } else { 0.0 };
    ls.pad_top + ls.pad_bottom + rows * ls.lh + image
}

/// `lazy`: return None instead of shaping a line the cache does not hold.
#[allow(clippy::too_many_arguments)]
pub fn shape_line(g: &Gfx, cache: &mut Cache, src: &str, deco: &Deco, line: Range<usize>, ld: &LineDeco, width: f32, look: &Look, t: &Theme, images: ImageSize, lazy: bool) -> Option<Shaped> {
    let ls = line_style(ld, look, t);
    let mut key = std::hash::DefaultHasher::new();
    (&src[line.clone()], ld, width.to_bits(), look).hash(&mut key);
    let hidden: Vec<Range<usize>> = deco.hidden_in(&line).iter().map(|h| h.start.max(line.start)..h.end.min(line.end)).collect();
    let widgets: Vec<&(Range<usize>, Widget)> = deco.widgets_in(&line).iter().collect();
    let marks: Vec<&(Range<usize>, Mark)> = deco.marks_in(&line).collect();
    for h in &hidden {
        (h.start - line.start, h.end - line.start).hash(&mut key);
    }
    for (r, w) in &widgets {
        (r.start - line.start, r.end - line.start).hash(&mut key);
        match w {
            // Draws nothing inside the line, and its ranges are absolute offsets.
            Widget::CodeLang { .. } => {}
            Widget::Image { src, .. } => (w, images(src).map(|(a, b)| (a.to_bits(), b.to_bits()))).hash(&mut key),
            _ => w.hash(&mut key),
        }
    }
    for (r, m) in &marks {
        (r.start.saturating_sub(line.start), r.end - line.start, m).hash(&mut key);
    }
    let key = key.finish();
    if let Some((at, s)) = cache.map.get(&key) {
        return Some(s.clone().moved(*at, line.start));
    }
    if lazy {
        return None;
    }

    let size = ls.style.size;
    let text_w = (width - ls.indent - ls.right).max(1.0);
    let mut segs: Vec<Seg> = vec![];
    let mut disp: Vec<u16> = vec![];
    let mut objects: Vec<(u32, Obj)> = vec![];
    let mut marker: Option<(String, usize)> = None;
    let mut spacers: Vec<(u32, f32, f32, f32)> = vec![];
    let push_text = |segs: &mut Vec<Seg>, disp: &mut Vec<u16>, s: usize, e: usize| {
        if e <= s {
            return;
        }
        let piece = &src[s..e];
        let piece = if ls.upper { piece.to_ascii_uppercase() } else { piece.to_string() };
        let w: Vec<u16> = piece.encode_utf16().collect();
        segs.push(Seg { src: s, src_len: e - s, disp: disp.len() as u32, disp_len: w.len() as u32, text: true });
        disp.extend(w);
    };

    // Header and end lines of fenced code show no text of their own.
    let skip_text = ld.code == Code::Header || ld.code == Code::End || ld.rule;
    let mut p = line.start;
    let mut wi = 0;
    while p < line.end && !skip_text {
        if let Some(h) = hidden.iter().find(|h| h.start <= p && p < h.end) {
            p = h.end;
            continue;
        }
        if let Some((r, w)) = widgets.get(wi).filter(|(r, _)| r.start <= p) {
            wi += 1;
            if r.start < p {
                continue;
            }
            match w {
                Widget::Bullet => marker = Some(("•".into(), r.end)),
                Widget::Checkbox { checked } => {
                    let at = disp.len() as u32;
                    segs.push(Seg { src: r.start, src_len: r.end - r.start, disp: at, disp_len: 1, text: false });
                    disp.push(0xfffc);
                    let b = 15.0;
                    spacers.push((at, b + 8.0, b, b * 0.8));
                    objects.push((at, Obj::Checkbox { at: r.start, checked: *checked }));
                }
                Widget::Image { src: s, alt } => {
                    let at = disp.len() as u32;
                    segs.push(Seg { src: r.start, src_len: r.end - r.start, disp: at, disp_len: 1, text: false });
                    disp.push(0xfffc);
                    let (w, h) = match images(s) {
                        Some((w, h)) => {
                            let k = (text_w / w).min(1.0);
                            (w * k, h * k)
                        }
                        None => (text_w.min(160.0), 96.0),
                    };
                    spacers.push((at, w, h, h - size * 0.25));
                    objects.push((at, Obj::Image { src: s.clone(), alt: alt.clone(), range: r.clone() }));
                }
                Widget::Rule | Widget::CodeLang { .. } => {}
            }
            p = r.end.max(p);
            continue;
        }
        let next_hidden = hidden.iter().filter(|h| h.start > p).map(|h| h.start).min().unwrap_or(line.end);
        let next_widget = widgets.get(wi).map_or(line.end, |(r, _)| r.start.max(p));
        let e = next_hidden.min(next_widget).min(line.end);
        push_text(&mut segs, &mut disp, p, e);
        p = e;
    }
    // Ordered markers are text with a list-marker mark; hang them like bullets.
    if ld.list_start && marker.is_none() {
        if let Some((r, _)) = marks.iter().find(|(r, m)| *m == Mark::ListMarker && r.start >= line.start) {
            if let Some(first) = segs.first().copied().filter(|s| s.text && s.src == r.start) {
                let cut = (r.end - first.src).min(first.src_len);
                let text = src[first.src..first.src + cut].trim_end().to_string();
                let cut16 = src[first.src..first.src + cut].encode_utf16().count() as u32;
                disp.drain(..cut16 as usize);
                for s in segs.iter_mut() {
                    s.disp = s.disp.saturating_sub(cut16);
                }
                segs[0].src += cut;
                segs[0].src_len -= cut;
                segs[0].disp_len -= cut16;
                if segs[0].src_len == 0 {
                    segs.remove(0);
                }
                for o in objects.iter_mut() {
                    o.0 -= cut16;
                }
                for s in spacers.iter_mut() {
                    s.0 -= cut16;
                }
                marker = Some((text, r.end));
            }
        }
    }

    let family = look.family(ls.style.stack);
    let layout = (!skip_text).then(|| {
        let text = String::from_utf16_lossy(&disp);
        let l = g.new_layout(&text, ls.style, text_w, family);
        unsafe {
            let fam = family.unwrap_or(Gfx::primary_family(ls.style.stack));
            let (a, d) = g.ascent(fam);
            let baseline = (ls.lh - (a + d) * size) / 2.0 + a * size;
            // Uniform spacing would let a tall image overflow upwards; such lines grow to fit.
            if !objects.iter().any(|(_, o)| matches!(o, Obj::Image { .. })) {
                l.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, ls.lh, baseline).unwrap();
            }
            if ls.center {
                l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER).unwrap();
            }
            l.SetDrawingEffect(&g.solid(ls.color), whole(&disp)).unwrap();
            if ls.tracking != 0.0 {
                if let Ok(l1) = l.cast::<IDWriteTextLayout1>() {
                    l1.SetCharacterSpacing(0.0, ls.tracking, 0.0, whole(&disp)).unwrap();
                }
            }
            for (at, w, h, b) in &spacers {
                l.SetInlineObject(&spacer(*w, *h, *b), DWRITE_TEXT_RANGE { startPosition: *at, length: 1 }).unwrap();
            }
        }
        l
    });

    let mut backgrounds = vec![];
    if let Some(l) = &layout {
        let code_family = look.font_code.as_deref().unwrap_or("Consolas");
        for (r, m) in &marks {
            let Some(dr) = to_disp_range(&segs, src, r.clone()) else { continue };
            let range = DWRITE_TEXT_RANGE { startPosition: dr.start, length: dr.end - dr.start };
            unsafe {
                match m {
                    Mark::Strong => l.SetFontWeight(DWRITE_FONT_WEIGHT(650), range).unwrap(),
                    Mark::Em => l.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, range).unwrap(),
                    Mark::Strike => {
                        l.SetStrikethrough(true, range).unwrap();
                        l.SetDrawingEffect(&g.solid(t.text_tertiary), range).unwrap();
                    }
                    Mark::Underline => l.SetUnderline(true, range).unwrap(),
                    Mark::Code => {
                        g.set_family(l, code_family, range);
                        l.SetFontSize(size * 0.88, range).unwrap();
                        l.SetDrawingEffect(&g.solid(t.accent), range).unwrap();
                        backgrounds.push((dr.clone(), Bg::Code));
                    }
                    Mark::Math => {
                        g.set_family(l, code_family, range);
                        l.SetFontSize(size * 0.88, range).unwrap();
                        l.SetDrawingEffect(&g.solid(t.text_secondary), range).unwrap();
                    }
                    Mark::Mark => backgrounds.push((dr.clone(), Bg::Mark)),
                    Mark::Sup | Mark::Sub => l.SetFontSize(size * 0.75, range).unwrap(),
                    Mark::Link => {
                        l.SetUnderline(true, range).unwrap();
                        l.SetDrawingEffect(&g.solid(t.link), range).unwrap();
                    }
                    Mark::ListMarker => l.SetDrawingEffect(&g.solid(t.accent), range).unwrap(),
                    Mark::Hl(tok) => {
                        use mdwisp_text::highlight::Token as K;
                        let c = match tok {
                            K::Comment | K::Quote => t.hl_comment,
                            K::Keyword | K::Literal | K::Section | K::Name | K::Meta => t.hl_keyword,
                            K::String | K::Attribute => t.hl_string,
                            K::Number | K::Symbol | K::Bullet | K::Link => t.hl_number,
                            K::Title => t.hl_func,
                            K::Type | K::BuiltIn => t.hl_type,
                            K::Attr | K::Variable => t.hl_attr,
                            K::Emphasis | K::Strong => ls.color,
                        };
                        l.SetDrawingEffect(&g.solid(c), range).unwrap();
                        match tok {
                            K::Comment | K::Quote | K::Emphasis => l.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, range).unwrap(),
                            K::Strong => l.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, range).unwrap(),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    let marker = marker.map(|(text, end)| {
        let l = g.new_layout(&text, Style::new(ls.style.stack, size), 1.35 * size, family);
        unsafe {
            let (a, d) = g.ascent(family.unwrap_or(Gfx::primary_family(ls.style.stack)));
            l.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, ls.lh, (ls.lh - (a + d) * size) / 2.0 + a * size).unwrap();
            l.SetDrawingEffect(&g.solid(t.accent), DWRITE_TEXT_RANGE { startPosition: 0, length: text.encode_utf16().count() as u32 }).unwrap();
        }
        (l, end)
    });

    let text_h = match &layout {
        Some(l) => metrics(l).h.max(ls.lh),
        None => ls.lh,
    };
    let shaped = Shaped { layout, segs, marker, objects, backgrounds, indent: ls.indent, pad_top: ls.pad_top, pad_bottom: ls.pad_bottom, text_h, width: text_w };
    if cache.map.len() > 20000 {
        cache.map.clear();
    }
    cache.map.insert(key, (line.start, shaped.clone()));
    Some(shaped)
}

fn whole(w: &[u16]) -> DWRITE_TEXT_RANGE {
    DWRITE_TEXT_RANGE { startPosition: 0, length: w.len().max(1) as u32 }
}

/// Source byte range → displayed range on this line, clipped to what is visible.
pub fn to_disp_range(segs: &[Seg], src: &str, r: Range<usize>) -> Option<Range<u32>> {
    let start = src_to_disp_pos(segs, r.start, src);
    let end = src_to_disp_pos(segs, r.end, src);
    (end > start).then_some(start..end)
}

/// Displayed position → source offset. Positions next to hidden markup land
/// after it: typing at the start of `**bold**` goes inside the bold, at its
/// end outside.
pub fn disp_to_src(segs: &[Seg], line: &Range<usize>, d: u32, src: &str) -> usize {
    for s in segs {
        if d >= s.disp + s.disp_len {
            continue;
        }
        if d <= s.disp || !s.text {
            return s.src;
        }
        let mut n = 0u32;
        for (i, c) in src[s.src..s.src + s.src_len].char_indices() {
            if n >= d - s.disp {
                return s.src + i;
            }
            n += c.len_utf16() as u32;
        }
        return s.src + s.src_len;
    }
    line.end
}

/// Source offset → displayed caret position on this line.
pub fn src_to_disp_pos(segs: &[Seg], pos: usize, src: &str) -> u32 {
    let mut last = 0;
    for s in segs {
        if pos < s.src {
            return s.disp;
        }
        if pos <= s.src + s.src_len {
            if !s.text {
                return if pos == s.src { s.disp } else { s.disp + s.disp_len };
            }
            // A position from another line's coordinates may land mid-character; round down.
            let mut end = pos.min(src.len());
            while !src.is_char_boundary(end) {
                end -= 1;
            }
            return s.disp + src[s.src..end.max(s.src)].encode_utf16().count() as u32;
        }
        last = s.disp + s.disp_len;
    }
    last
}


