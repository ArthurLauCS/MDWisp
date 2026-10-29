//! Editor timings and random-editing crash checks on real layouts: `cargo test --release -p mdwisp bench -- --ignored --nocapture`.

use std::time::Instant;

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::editor::layout::Look;
use crate::editor::view::View;
use crate::gfx::{Gfx, Rect};
use crate::theme;

fn big_doc(copies: usize) -> String {
    let one = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../stock/欢迎使用/欢迎使用.md")).unwrap();
    // Distinct copies, so the layout cache cannot share lines between them.
    (0..copies).map(|i| one.replace("的", &format!("的{i}"))).collect::<Vec<_>>().join("\n")
}

fn look() -> Look {
    Look { read: 16f32.to_bits(), line_height: 1.7f32.to_bits(), code: 14f32.to_bits(), tab: 4, ..Default::default() }
}

fn frame(g: &Gfx, v: &mut View, t: &theme::Theme) -> f64 {
    let a = Instant::now();
    unsafe {
        g.dc.BeginDraw();
        g.dc.Clear(Some(&t.bg_base));
    }
    v.paint(g, t, Rect::new(260.0, 40.0, 900.0, 800.0));
    unsafe {
        let _ = g.dc.EndDraw(None, None);
    }
    a.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore]
fn bench() {
    let fonts = crate::resource_dir("fonts");
    let g = Gfx::new(fonts.as_deref(), false).unwrap();
    let t = theme::DARK;
    unsafe {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            ..Default::default()
        };
        let bmp = g.dc.CreateBitmap(D2D_SIZE_U { width: 1280, height: 840 }, None, 0, &props).unwrap();
        g.dc.SetTarget(&bmp);
    }
    for copies in [1, 20, 60] {
        let text = big_doc(copies);
        let mut v = View::new(text.clone(), None);
        v.set_look(look());
        let first = frame(&g, &mut v, &t);
        let mut fill = 0.0;
        let mut rounds = 0;
        while v.pending && rounds < 1000 {
            let a = Instant::now();
            v.fill_budget = 300;
            v.relayout(&g, &t);
            fill += a.elapsed().as_secs_f64() * 1000.0;
            rounds += 1;
        }
        let idle = frame(&g, &mut v, &t);
        // Type into the middle of the document, the way WM_CHAR does.
        let mid = text.len() / 2;
        let mid = (mid..text.len()).find(|&i| text.is_char_boundary(i) && text[..i].ends_with('\n')).unwrap();
        v.move_to(mid, false);
        frame(&g, &mut v, &t);
        let a = Instant::now();
        for c in "测试输入abc".chars() {
            v.type_text(&c.to_string(), true);
            v.relayout(&g, &t);
        }
        let typing = a.elapsed().as_secs_f64() * 1000.0 / 7.0;
        let a = Instant::now();
        for _ in 0..10 {
            v.move_h(1, false, false);
            v.relayout(&g, &t);
        }
        let caret = a.elapsed().as_secs_f64() * 1000.0 / 10.0;
        let typed_frame = frame(&g, &mut v, &t);
        let a = Instant::now();
        let focus = crate::editor::deco::Focus { from: mid, to: mid, head: mid, searching: false };
        for _ in 0..5 {
            std::hint::black_box(crate::editor::deco::decorate(&v.buf.text, &focus));
        }
        let deco = a.elapsed().as_secs_f64() * 1000.0 / 5.0;
        let a = Instant::now();
        for _ in 0..5 {
            std::hint::black_box(mdwisp_text::parser(&v.buf.text).count());
        }
        let parse = a.elapsed().as_secs_f64() * 1000.0 / 5.0;
        println!("     decorate {deco:6.2} ms (parse alone {parse:6.2})");
        println!("{copies:>3}x {:>7} bytes {:>6} lines | first frame {first:7.1} ms | fill {fill:7.1} ms / {rounds} rounds | idle frame {idle:5.1} | key {typing:6.2} | caret {caret:6.2} | frame after typing {typed_frame:5.1}", text.len(), v.lines.len());
    }
}

#[test]
#[ignore]
fn stale_segs() {
    let fonts = crate::resource_dir("fonts");
    let g = Gfx::new(fonts.as_deref(), false).unwrap();
    let t = theme::DARK;
    let mut v = View::new("第一行\n\n第二行 **粗体** 文字\n\n第三行".into(), None);
    v.set_look(look());
    v.content_w = 600.0;
    v.rect = Rect::new(0.0, 0.0, 800.0, 600.0);
    v.relayout(&g, &t);
    v.move_to(0, false);
    v.type_text("中", false);
    v.move_to(0, false);
    v.relayout(&g, &t);
    // Cached layouts of the lines below the edit must carry their new offsets.
    for l in &v.lines {
        if let Some(s) = l.shaped.segs.first() {
            assert!(s.src >= l.src.start && s.src <= l.src.end, "line {:?} first seg src {}", l.src, s.src);
        }
    }
}

/// Random editing through the same entry points as keys and clicks; any panic is a crash in the app.
#[test]
#[ignore]
fn fuzz() {
    let fonts = crate::resource_dir("fonts");
    let g = Gfx::new(fonts.as_deref(), false).unwrap();
    let t = theme::DARK;
    unsafe {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            ..Default::default()
        };
        let bmp = g.dc.CreateBitmap(D2D_SIZE_U { width: 1280, height: 840 }, None, 0, &props).unwrap();
        g.dc.SetTarget(&bmp);
    }
    let pieces = ["中", "文", "a", "b", " ", "*", "**", "_", "`", "```", "~", "#", "# ", "## ", "- ", "1. ", "> ", "[", "]", "(", ")", "![", "|", "| a | b |\n|---|---|\n| 1 | 2 |", "$", "==", "<u>", "</u>", "http://x.cn ", "😀", "\t", "- [ ] ", "---", "\n", "\n\n", "é", "粘贴\r\n两行\r\n"];
    let seeds: u64 = std::env::var("FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    for seed in 0..seeds {
        let mut r = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut next = |n: usize| {
            r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((r >> 33) as usize) % n.max(1)
        };
        let base = crate::doc::normalise(&big_doc(1));
        let start = next(base.len());
        let start = (start..base.len()).find(|&i| base.is_char_boundary(i)).unwrap_or(0);
        let mut v = View::new(base[start..].chars().take(1500).collect(), None);
        v.set_look(look());
        let mut log = vec![];
        for _ in 0..400 {
            let op = next(24);
            log.push(op);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Keys and clicks end an IME composition first, as Window::end_ime does.
                if let Some(r) = v.composition.clone().filter(|_| op < 22) {
                    let s = v.buf.text[r].to_string();
                    v.commit(&s, false);
                }
                match op {
                    0..=5 => v.type_text(pieces[next(pieces.len())], next(2) == 0),
                    6 => v.enter(next(2) == 0, next(4) == 0),
                    7 => v.backspace(next(2) == 0, next(4) == 0),
                    8 => v.delete_forward(next(4) == 0),
                    9 => v.tab(next(2) == 0, 4),
                    10 => v.move_h(if next(2) == 0 { -1 } else { 1 }, next(3) == 0, next(3) == 0),
                    11 => v.move_v(if next(2) == 0 { -1 } else { 1 }, next(3) == 0, next(5) == 0),
                    12 => v.move_line_edge(next(2) == 0, next(3) == 0),
                    13 => {
                        let p = next(v.buf.len() + 1);
                        let p = (0..=p).rev().find(|&i| v.buf.text.is_char_boundary(i)).unwrap_or(0);
                        v.move_to(p, next(3) == 0)
                    }
                    14 => {
                        let (x, y) = (260.0 + next(900) as f32, 40.0 + next(800) as f32);
                        let p = v.pos_at(x, y);
                        v.move_to(p, next(3) == 0)
                    }
                    15 => {
                        let p = v.buf.sel.head;
                        v.select_word_at(p)
                    }
                    16 => {
                        v.buf.undo();
                    }
                    17 => {
                        v.buf.redo();
                    }
                    18 => v.select_all(),
                    19 => v.insert(&pieces[next(pieces.len())].repeat(1 + next(3))),
                    20 => v.scroll_to(next(3000) as f32, false),
                    21 if next(4) == 0 => {
                        let mut s = crate::editor::search::Search::default();
                        s.find = crate::shell::widgets::Field::new(pieces[next(pieces.len())]);
                        s.replace = crate::shell::widgets::Field::new(pieces[next(pieces.len())]);
                        s.regex = next(2) == 0;
                        v.search = Some(s);
                        v.relayout(&g, &t);
                        match next(3) {
                            0 => drop(v.search_step(1)),
                            1 => v.search_replace(),
                            _ => drop(v.search_replace_all()),
                        }
                    }
                    21 if next(2) == 0 => v.search = None,
                    22 => {
                        let p = pieces[next(pieces.len())];
                        v.compose(p, next(p.encode_utf16().count() + 1))
                    }
                    23 => v.commit(if next(2) == 0 { "" } else { pieces[next(pieces.len())] }, next(2) == 0),
                    _ => {
                        let _ = v.selected_text().len();
                    }
                }
                v.relayout(&g, &t);
                let _ = v.caret_at(v.buf.sel.head);
                frame(&g, &mut v, &t);
                // Background fill, as the layout timer does.
                if v.pending {
                    v.fill_budget = 300;
                    v.relayout(&g, &t);
                }
            }));
            if result.is_err() {
                panic!("seed {seed} after ops {log:?}\ntext: {:?}\nsel {:?}", v.buf.text, v.buf.sel);
            }
        }
    }
}

/// Which face DirectWrite really picked for each run of a layout, read from the glyph runs.
mod face_probe {
    use std::ffi::c_void;
    use super::*;
    use windows_core::AsImpl;

    #[windows_core::implement(windows::Win32::Graphics::DirectWrite::IDWriteTextRenderer)]
    struct RunFaces(std::cell::RefCell<Vec<(u32, u32, String)>>);

    use windows::Win32::Foundation::E_NOTIMPL;
    use windows_core::BOOL;
    use windows::Win32::Graphics::DirectWrite::*;
    use windows_core::{Interface, Ref, Result};

    impl IDWritePixelSnapping_Impl for RunFaces_Impl {
        fn IsPixelSnappingDisabled(&self, _: *const c_void) -> Result<BOOL> {
            Ok(BOOL(0))
        }
        fn GetCurrentTransform(&self, _: *const c_void, m: *mut DWRITE_MATRIX) -> Result<()> {
            unsafe { *m = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 } };
            Ok(())
        }
        fn GetPixelsPerDip(&self, _: *const c_void) -> Result<f32> {
            Ok(1.0)
        }
    }

    impl IDWriteTextRenderer_Impl for RunFaces_Impl {
        fn DrawGlyphRun(&self, _: *const c_void, _: f32, _: f32, _: DWRITE_MEASURING_MODE, run: *const DWRITE_GLYPH_RUN, desc: *const DWRITE_GLYPH_RUN_DESCRIPTION, _: Ref<windows_core::IUnknown>) -> Result<()> {
            unsafe {
                let face = (*run).fontFace.as_ref().ok_or(E_NOTIMPL)?;
                let face3: IDWriteFontFace3 = face.cast()?;
                let names = face3.GetFamilyNames()?;
                let len = names.GetStringLength(0)?;
                let mut buf = vec![0u16; len as usize + 1];
                names.GetString(0, &mut buf)?;
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                let wn = face3.GetFaceNames()?;
                let l2 = wn.GetStringLength(0)?;
                let mut b2 = vec![0u16; l2 as usize + 1];
                wn.GetString(0, &mut b2)?;
                let style = String::from_utf16_lossy(&b2[..l2 as usize]);
                self.0.borrow_mut().push(((*desc).textPosition, (*desc).stringLength, format!("{name} / {style}")));
            }
            Ok(())
        }
        fn DrawUnderline(&self, _: *const c_void, _: f32, _: f32, _: *const DWRITE_UNDERLINE, _: Ref<windows_core::IUnknown>) -> Result<()> {
            Ok(())
        }
        fn DrawStrikethrough(&self, _: *const c_void, _: f32, _: f32, _: *const DWRITE_STRIKETHROUGH, _: Ref<windows_core::IUnknown>) -> Result<()> {
            Ok(())
        }
        fn DrawInlineObject(&self, _: *const c_void, _: f32, _: f32, _: Ref<IDWriteInlineObject>, _: BOOL, _: BOOL, _: Ref<windows_core::IUnknown>) -> Result<()> {
            Ok(())
        }
    }

/// Mixed Chinese / English text resolves to Noto Serif SC and Latin Modern Roman, quotes and
/// emphasis to LXGW WenKai. Skipped when the font files have not been fetched.
#[test]
fn bundled_faces_resolve() {
    use crate::gfx::{Stack, Style};
    use windows::Win32::Graphics::DirectWrite::*;
    let Some(dir) = crate::resource_dir("fonts") else { return };
    if !dir.join("NotoSerifSC-VF.ttf").exists() {
        eprintln!("fonts not fetched: skipped");
        return;
    }
    let g = Gfx::new(Some(&dir), false).unwrap();
    let probe = |text: &str, style: Style, kai: bool| -> Vec<String> {
        let l = g.new_layout(text, style, 800.0, None);
        let w: Vec<u16> = text.encode_utf16().collect();
        if kai {
            g.kai_runs(&l, &w, DWRITE_TEXT_RANGE { startPosition: 0, length: w.len() as u32 });
        }
        let r: IDWriteTextRenderer = RunFaces(Default::default()).into();
        unsafe { l.Draw(None, &r, 0.0, 0.0).unwrap() };
        let inner: &RunFaces = unsafe { r.as_impl() };
        let mut v: Vec<String> = inner.0.borrow().iter().map(|(p, n, f)| format!("{}", {
            let s: Vec<u16> = w[*p as usize..(*p + *n) as usize].to_vec();
            format!("{:?} -> {f}", String::from_utf16_lossy(&s))
        })).collect();
        v.sort();
        v
    };
    let body = Style::new(Stack::Read, 17.0);
    let mixed = probe("在 Rust 里写 hello world，“引号”和——破折号。", body, false);
    eprintln!("{mixed:#?}");
    let joined = mixed.join("\n");
    assert!(joined.contains("\"hello world\"") || joined.contains("hello world"), "{joined}");
    for line in &mixed {
        let cjk = line.chars().next() == Some('"') && line.chars().nth(1).is_some_and(|c| c as u32 >= 0x2e80);
        if cjk {
            assert!(line.contains("Noto Serif SC"), "{line}");
        } else if line.contains("Rust") || line.contains("hello") {
            assert!(line.contains("Latin Modern Roman") || line.contains("LM Roman"), "{line}");
        }
    }
    let heading = probe("标题 Heading 一", Style::new(Stack::Display, 26.0).weight(700), false);
    eprintln!("{heading:#?}");
    let quote = probe("引用 quote 文字", body, true);
    eprintln!("{quote:#?}");
    assert!(quote.iter().any(|l| l.contains("WenKai")), "{quote:?}");
    let italic = probe("斜体 italic 文字", Style { italic: true, ..body }, false);
    eprintln!("{italic:#?}");
    assert!(italic.iter().any(|l| l.contains("WenKai")) && italic.iter().any(|l| l.contains("Italic")), "{italic:?}");

    // Without the files the roles fall back to the Windows faces, silently.
    let g = Gfx::new(None, false).unwrap();
    let l = g.new_layout("在 Rust 里", body, 800.0, None);
    let r: IDWriteTextRenderer = RunFaces(Default::default()).into();
    unsafe { l.Draw(None, &r, 0.0, 0.0).unwrap() };
    let inner: &RunFaces = unsafe { r.as_impl() };
    let fell: Vec<String> = inner.0.borrow().iter().map(|(_, _, f)| f.clone()).collect();
    eprintln!("fallback: {fell:?}");
    assert!(fell.iter().any(|f| f.starts_with("Sitka")) && fell.iter().any(|f| f.starts_with("SimSun")), "{fell:?}");
}
}
