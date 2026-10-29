//! Drawing device and text system.
//!
//! One WARP (CPU-rasterised) D3D11 device for the whole process, one flip-model
//! swap chain per window hooked up through DirectComposition — the same setup
//! as Nookery. Measured on this machine (2026-10-18): creating a hardware
//! device costs ~200 ms before the first frame (driver load) and ~+47 MB
//! private memory; WARP gets a painted window in ~40 ms at ~25 MB.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::*;
use windows_numerics::Matrix3x2;

use crate::theme::Color;

pub struct Gfx {
    d3d: ID3D11Device,
    dxgi: IDXGIFactory2,
    /// Outlives device switches: windows keep their composition targets.
    pub dcomp: IDCompositionDevice,
    pub factory: ID2D1Factory1,
    pub dc: ID2D1DeviceContext,
    pub brush: ID2D1SolidColorBrush,
    pub dw: IDWriteFactory5,
    /// The bundled faces only. Merging them into the system font set measured
    /// 117 ms at startup; a small separate collection is nearly free.
    bundled: IDWriteFontCollection1,
    system: IDWriteFontCollection,
    faces: Faces,
    formats: RefCell<HashMap<(Stack, u32, i32, bool), IDWriteTextFormat>>,
    layouts: RefCell<HashMap<LayoutKey, IDWriteTextLayout>>,
    brushes: RefCell<HashMap<[u32; 4], ID2D1SolidColorBrush>>,
    ascents: RefCell<HashMap<String, (f32, f32)>>,
}

/// A fixed-size box inside a text layout (checkboxes, inline images). Drawing is
/// done by the editor at the box's position; the object only reserves space.
#[implement(IDWriteInlineObject)]
pub struct Spacer {
    pub w: f32,
    pub h: f32,
    pub baseline: f32,
}

impl IDWriteInlineObject_Impl for Spacer_Impl {
    fn Draw(&self, _: *const core::ffi::c_void, _: windows_core::Ref<IDWriteTextRenderer>, _: f32, _: f32, _: BOOL, _: BOOL, _: windows_core::Ref<windows_core::IUnknown>) -> Result<()> {
        Ok(())
    }
    fn GetMetrics(&self) -> Result<DWRITE_INLINE_OBJECT_METRICS> {
        Ok(DWRITE_INLINE_OBJECT_METRICS { width: self.w, height: self.h, baseline: self.baseline, supportsSideways: BOOL(0) })
    }
    fn GetOverhangMetrics(&self) -> Result<DWRITE_OVERHANG_METRICS> {
        Ok(DWRITE_OVERHANG_METRICS::default())
    }
    fn GetBreakConditions(&self, before: *mut DWRITE_BREAK_CONDITION, after: *mut DWRITE_BREAK_CONDITION) -> Result<()> {
        unsafe {
            *before = DWRITE_BREAK_CONDITION_NEUTRAL;
            *after = DWRITE_BREAK_CONDITION_NEUTRAL;
        }
        Ok(())
    }
}

pub fn spacer(w: f32, h: f32, baseline: f32) -> IDWriteInlineObject {
    Spacer { w, h, baseline }.into()
}

#[derive(Clone)]
pub struct Surface {
    swap: IDXGISwapChain1,
    target: IDCompositionTarget,
    visual: IDCompositionVisual,
}

/// The four font stacks (style `paper`, typeset by role like a ctex document).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stack {
    /// Chrome (menus, panels, buttons): Microsoft YaHei UI covers Latin and CJK.
    Ui,
    /// Body text: Latin Modern Roman, CJK in Noto Serif SC.
    Read,
    /// Headings: the same two faces, bold.
    Display,
    Code,
}

const UI: &str = "Microsoft YaHei UI";
const CODE: &str = "Consolas";
/// Bundled faces, each with the family names DirectWrite may report for it (a
/// font-set collection groups by typographic or by RIBBI family name).
const LATIN_MODERN: &[&str] = &["Latin Modern Roman 10", "Latin Modern Roman", "LM Roman 10"];
const NOTO_SERIF: &[&str] = &["Noto Serif SC"];
const WENKAI: &[&str] = &["LXGW WenKai", "霞鹜文楷"];
/// Windows faces used when the font file is missing.
const FALLBACK_BODY: &str = "Sitka Text";
const FALLBACK_HEADING: &str = "Sitka Heading";
const FALLBACK_CJK: &str = "SimSun";

/// The families the stacks resolved to at startup: bundled when the file loaded,
/// the Windows face otherwise.
struct Faces {
    body: String,
    heading: String,
    /// CJK runs of body text and headings.
    cjk: String,
    /// CJK runs of blockquotes and emphasis (楷体 beside 宋体).
    kai: String,
}

/// YaHei UI has 400 and 700 only, and the style uses two weights.
fn snap_weight(w: i32) -> i32 {
    if w >= 500 { 700 } else { 400 }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub stack: Stack,
    pub size: f32,
    pub weight: i32,
    pub italic: bool,
}

impl Style {
    pub const fn new(stack: Stack, size: f32) -> Style {
        Style { stack, size, weight: 400, italic: false }
    }
    pub const fn weight(mut self, weight: i32) -> Style {
        self.weight = weight;
        self
    }
}

impl Eq for Style {}

#[derive(PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    style: (Stack, u32, i32, bool),
    width: u32,
    family: Option<String>,
}

impl std::hash::Hash for Style {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (self.stack, self.size.to_bits(), self.weight, self.italic).hash(h)
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

impl Gfx {
    /// `gpu`: the hardware device, which draws blurred overlays and high-DPI
    /// frames far faster but takes ~170 ms to create; otherwise WARP (CPU, ~40 ms).
    pub fn new(font_dir: Option<&Path>, gpu: bool) -> Result<Gfx> {
        unsafe {
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let mut d3d = None;
            let driver = if gpu { D3D_DRIVER_TYPE_HARDWARE } else { D3D_DRIVER_TYPE_WARP };
            D3D11CreateDevice(None, driver, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None)?;
            let d3d = d3d.unwrap();
            let dxgi_dev: IDXGIDevice = d3d.cast()?;
            let dc = factory.CreateDevice(&dxgi_dev)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let brush = dc.CreateSolidColorBrush(&Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let dw: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let bundled = font_collection(&dw, font_dir)?;
            let mut system = None;
            dw.GetSystemFontCollection(false, &mut system, false)?;
            let system: IDWriteFontCollection = system.unwrap().cast()?;
            let find = |names: &[&str]| names.iter().find(|n| family_in(&bundled, n)).map(|n| n.to_string());
            let cjk = find(NOTO_SERIF).unwrap_or_else(|| FALLBACK_CJK.into());
            let latin = find(LATIN_MODERN);
            let faces = Faces {
                body: latin.clone().unwrap_or_else(|| FALLBACK_BODY.into()),
                heading: latin.unwrap_or_else(|| FALLBACK_HEADING.into()),
                kai: find(WENKAI).unwrap_or_else(|| cjk.clone()),
                cjk,
            };
            Ok(Gfx {
                dxgi: CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?,
                dcomp: DCompositionCreateDevice(&dxgi_dev)?,
                d3d,
                factory,
                dc,
                brush,
                dw,
                bundled,
                system,
                faces,
                formats: RefCell::default(),
                layouts: RefCell::default(),
                brushes: RefCell::default(),
                ascents: RefCell::default(),
            })
        }
    }

    pub fn surface(&self, hwnd: HWND, w: u32, h: u32) -> Result<Surface> {
        unsafe {
            let target = self.dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = self.dcomp.CreateVisual()?;
            target.SetRoot(&visual)?;
            let s = Surface { swap: self.swap_chain(w, h)?, target, visual };
            self.show(&s)?;
            Ok(s)
        }
    }

    /// The same window surface with a swap chain on this device. The old one stays
    /// on screen until `show`, after the first frame: switching devices never blanks.
    pub fn move_surface(&self, s: &Surface, w: u32, h: u32) -> Result<Surface> {
        Ok(Surface { swap: self.swap_chain(w, h)?, target: s.target.clone(), visual: s.visual.clone() })
    }

    pub fn show(&self, s: &Surface) -> Result<()> {
        unsafe {
            s.visual.SetContent(&s.swap)?;
            self.dcomp.Commit()
        }
    }

    fn swap_chain(&self, w: u32, h: u32) -> Result<IDXGISwapChain1> {
        unsafe {
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w.max(1),
                Height: h.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                // Shader input lets the frame be copied for the blurred backdrop behind overlays.
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT | DXGI_USAGE_SHADER_INPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            self.dxgi.CreateSwapChainForComposition(&self.d3d, &desc, None)
        }
    }

    /// Point the context at the surface's back buffer; then BeginDraw as usual.
    pub fn bind(&self, s: &Surface, dpi: f32) -> Result<()> {
        unsafe {
            let buf: IDXGISurface = s.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: dpi,
                dpiY: dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                ..Default::default()
            };
            let bmp = self.dc.CreateBitmapFromDxgiSurface(&buf, Some(&props))?;
            self.dc.SetTarget(&bmp);
            self.dc.SetDpi(dpi, dpi);
            Ok(())
        }
    }

    /// After EndDraw: unbind and present. `vsync` throttles animation frames to the display.
    pub fn present(&self, s: &Surface, vsync: bool) -> Result<()> {
        unsafe {
            self.dc.SetTarget(None);
            s.swap.Present(vsync as u32, DXGI_PRESENT(0)).ok()
        }
    }

    /// Save the frame just drawn (call after EndDraw, before present) as a PNG.
    pub fn capture(&self, s: &Surface, path: &Path) -> Result<()> {
        unsafe {
            let buf: ID3D11Texture2D = s.swap.GetBuffer(0)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            buf.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.d3d.CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.unwrap();
            let ctx = self.d3d.GetImmediateContext()?;
            ctx.CopyResource(&staging, &buf);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let (w, h) = (desc.Width, desc.Height);
            let mut px = vec![0u8; (w * h * 4) as usize];
            for y in 0..h as usize {
                let src = std::slice::from_raw_parts((map.pData as *const u8).add(y * map.RowPitch as usize), w as usize * 4);
                px[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(src);
            }
            ctx.Unmap(&staging, 0);
            use windows::Win32::Graphics::Imaging::*;
            use windows::Win32::System::Com::*;
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = wic.CreateStream()?;
            stream.InitializeFromFilename(PCWSTR(wide_z(&path.to_string_lossy()).as_ptr()), GENERIC_WRITE.0)?;
            let enc = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            enc.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.unwrap();
            frame.Initialize(None)?;
            frame.SetSize(w, h)?;
            let mut fmt = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut fmt)?;
            // The swap chain ignores alpha; make it opaque for viewers.
            for p in px.chunks_mut(4) {
                p[3] = 255;
            }
            frame.WritePixels(h, w * 4, &px)?;
            frame.Commit()?;
            enc.Commit()
        }
    }

    pub fn resize(&self, s: &Surface, w: u32, h: u32) {
        unsafe {
            self.dc.SetTarget(None);
            let _ = s.swap.ResizeBuffers(0, w.max(1), h.max(1), DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0));
        }
    }

    pub fn format(&self, style: Style) -> IDWriteTextFormat {
        let style = Style { weight: snap_weight(style.weight), ..style };
        let key = (style.stack, style.size.to_bits(), style.weight, style.italic);
        if let Some(f) = self.formats.borrow().get(&key) {
            return f.clone();
        }
        let family = self.primary_family(style.stack);
        let collection: IDWriteFontCollection = if family_in(&self.bundled, family) { self.bundled.cast().unwrap() } else { self.system.clone() };
        let f = unsafe {
            self.dw
                .CreateTextFormat(
                    PCWSTR(wide_z(family).as_ptr()),
                    &collection,
                    DWRITE_FONT_WEIGHT(style.weight),
                    if style.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL },
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    w!("zh-cn"),
                )
                .unwrap()
        };
        self.formats.borrow_mut().insert(key, f.clone());
        f
    }

    /// A shaped layout for `text`, cached: the same labels are drawn every frame.
    pub fn layout(&self, text: &str, style: Style, max_w: f32) -> IDWriteTextLayout {
        self.layout_with(text, style, max_w, None)
    }

    /// `family` overrides the stack's primary face (user-chosen fonts).
    pub fn layout_with(&self, text: &str, style: Style, max_w: f32, family: Option<&str>) -> IDWriteTextLayout {
        let key = LayoutKey { text: text.into(), style: (style.stack, style.size.to_bits(), style.weight, style.italic), width: max_w.to_bits(), family: family.map(Into::into) };
        if let Some(l) = self.layouts.borrow().get(&key) {
            return l.clone();
        }
        let l = self.new_layout(text, style, max_w, family);
        let mut cache = self.layouts.borrow_mut();
        // Wholesale clear instead of LRU: a frame re-creates what it needs once.
        if cache.len() > 4000 {
            cache.clear();
        }
        cache.insert(key, l.clone());
        l
    }

    /// `text` on one line, cut to `max_w` with an ellipsis: at the end, or at
    /// the start when `keep_end` (paths, where the file name matters most).
    pub fn elide(&self, text: &str, style: Style, max_w: f32, keep_end: bool) -> IDWriteTextLayout {
        let l = self.layout(text, style, 1.0e4);
        let w = metrics(&l).w;
        if w <= max_w {
            return l;
        }
        let room = max_w - metrics(&self.layout("…", style, 1.0e4)).w;
        let (mut trailing, mut inside, mut m) = (BOOL(0), BOOL(0), DWRITE_HIT_TEST_METRICS::default());
        // The character under the cut would be clipped mid-glyph: it goes too.
        unsafe { l.HitTestPoint(if keep_end { w - room } else { room }, 1.0, &mut trailing, &mut inside, &mut m).unwrap() };
        let u: Vec<u16> = text.encode_utf16().collect();
        let cut = if keep_end { format!("…{}", String::from_utf16_lossy(&u[(m.textPosition + m.length) as usize..])) } else { format!("{}…", String::from_utf16_lossy(&u[..m.textPosition as usize])) };
        self.layout(&cut, style, 1.0e4)
    }

    /// An uncached layout: for editor lines, which own their layouts.
    pub fn new_layout(&self, text: &str, style: Style, max_w: f32, family: Option<&str>) -> IDWriteTextLayout {
        let w = wide(text);
        unsafe {
            let l = self.dw.CreateTextLayout(&w, &self.format(style), max_w.max(0.0), 1.0e6).unwrap();
            if let Some(f) = family {
                self.set_family(&l, f, whole(&w));
            }
            // Serif stacks hand their CJK runs to the CJK serif (italic text to the kai face,
            // which has no italic to fake), unless the user chose a face.
            if family.is_none() && matches!(style.stack, Stack::Read | Stack::Display) {
                self.cjk_runs(&l, &w, whole(&w), if style.italic { &self.faces.kai } else { &self.faces.cjk });
            }
            // The Sitka fallback's default figures are old-style; text and tables want lining figures.
            if matches!(style.stack, Stack::Read | Stack::Display) {
                if let Ok(ty) = self.dw.CreateTypography() {
                    let _ = ty.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_LINING_FIGURES, parameter: 1 });
                    let _ = l.SetTypography(&ty, whole(&w));
                }
            }
            l
        }
    }

    /// Set a family on a range, from whichever collection holds it.
    pub fn set_family(&self, l: &IDWriteTextLayout, family: &str, range: DWRITE_TEXT_RANGE) {
        let name = wide_z(family);
        unsafe {
            let mut index = 0;
            let mut exists = BOOL(0);
            let bundled = self.bundled.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool();
            let collection: IDWriteFontCollection = if bundled { self.bundled.cast().unwrap() } else { self.system.clone() };
            let _ = l.SetFontCollection(&collection, range);
            let _ = l.SetFontFamilyName(PCWSTR(name.as_ptr()), range);
        }
    }

    /// The Latin face has no CJK glyphs: everything from U+2E80 up, and the general
    /// punctuation Chinese text borrows (quotes, dashes, ellipsis) when it touches
    /// CJK, goes to `family`. Only `within` is touched; a kai run is upright.
    fn cjk_runs(&self, l: &IDWriteTextLayout, w: &[u16], within: DWRITE_TEXT_RANGE, family: &str) {
        // Mark ranges can reach past text that was hidden or cut.
        let end = ((within.startPosition + within.length) as usize).min(w.len());
        let cjk = |c: u16| c >= 0x2e80;
        let borrowed = |c: u16| matches!(c, 0x2014 | 0x2015 | 0x2018..=0x201f | 0x2026);
        let is = |i: usize| {
            let c = w[i];
            cjk(c) || (borrowed(c) && ((i > 0 && cjk(w[i - 1])) || (i + 1 < w.len() && cjk(w[i + 1]))))
        };
        let mut i = within.startPosition as usize;
        while i < end {
            if is(i) {
                let start = i;
                while i < end && is(i) {
                    i += 1;
                }
                let range = DWRITE_TEXT_RANGE { startPosition: start as u32, length: (i - start) as u32 };
                self.set_family(l, family, range);
                unsafe {
                    let _ = l.SetFontStyle(DWRITE_FONT_STYLE_NORMAL, range);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Re-face the CJK runs inside `range` with the kai face (blockquotes, emphasis).
    pub fn kai_runs(&self, l: &IDWriteTextLayout, w: &[u16], range: DWRITE_TEXT_RANGE) {
        self.cjk_runs(l, w, range, &self.faces.kai);
    }

    pub fn drop_layout_cache(&self) {
        self.layouts.borrow_mut().clear();
    }

    pub fn has_family(&self, name: &str) -> bool {
        unsafe {
            let mut index = 0;
            let mut exists = BOOL(0);
            let n = wide_z(name);
            (self.bundled.FindFamilyName(PCWSTR(n.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool())
                || (self.system.FindFamilyName(PCWSTR(n.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool())
        }
    }

    /// Every installed family, for the settings font pickers.
    pub fn families(&self) -> Vec<String> {
        unsafe {
            let n = self.system.GetFontFamilyCount();
            let mut out: Vec<String> = (0..n)
                .filter_map(|i| {
                    let names = self.system.GetFontFamily(i).ok()?.GetFamilyNames().ok()?;
                    let mut idx = 0;
                    let mut ok = BOOL(0);
                    let _ = names.FindLocaleName(w!("en-us"), &mut idx, &mut ok);
                    let len = names.GetStringLength(if ok.as_bool() { idx } else { 0 }).ok()?;
                    let mut buf = vec![0u16; len as usize + 1];
                    names.GetString(if ok.as_bool() { idx } else { 0 }, &mut buf).ok()?;
                    Some(String::from_utf16_lossy(&buf[..len as usize]))
                })
                .collect();
            out.sort();
            out.dedup();
            out
        }
    }

    /// A brush that stays this colour, for text-layout drawing effects.
    pub fn solid(&self, c: Color) -> ID2D1SolidColorBrush {
        let key = [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()];
        self.brushes.borrow_mut().entry(key).or_insert_with(|| unsafe { self.dc.CreateSolidColorBrush(&c, None).unwrap() }).clone()
    }

    /// (ascent, descent) per em of a family's regular face, for CSS-style half-leading.
    pub fn ascent(&self, family: &str) -> (f32, f32) {
        if let Some(v) = self.ascents.borrow().get(family) {
            return *v;
        }
        let v = unsafe {
            let mut index = 0;
            let mut exists = BOOL(0);
            let name = wide_z(family);
            let bundled: IDWriteFontCollection = self.bundled.cast().unwrap();
            let coll = [bundled, self.system.clone()].into_iter().find(|c| {
                c.FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool()
            });
            coll
                .and_then(|c| c.GetFontFamily(index).ok())
                .and_then(|f| f.GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL).ok())
                .map(|font| {
                    let mut m = DWRITE_FONT_METRICS::default();
                    font.GetMetrics(&mut m);
                    let em = m.designUnitsPerEm as f32;
                    (m.ascent as f32 / em, m.descent as f32 / em)
                })
                .unwrap_or((0.9, 0.25))
        };
        self.ascents.borrow_mut().insert(family.into(), v);
        v
    }

    pub fn primary_family(&self, stack: Stack) -> &str {
        match stack {
            Stack::Ui => UI,
            Stack::Read => &self.faces.body,
            Stack::Display => &self.faces.heading,
            Stack::Code => CODE,
        }
    }

    /// Horizontal gradient brush across `x0..x1`.
    pub fn gradient(&self, x0: f32, x1: f32, y: f32, stops: &[(f32, Color)]) -> ID2D1LinearGradientBrush {
        unsafe {
            let stops: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|(p, c)| D2D1_GRADIENT_STOP { position: *p, color: *c }).collect();
            let rt: &ID2D1RenderTarget = &self.dc;
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).unwrap();
            self.dc
                .CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2 { X: x0, Y: y }.into(), endPoint: Vector2 { X: x1, Y: y }.into() },
                    None,
                    &coll,
                )
                .unwrap()
        }
    }

    pub fn fill_with(&self, r: Rect, radius: f32, brush: &ID2D1Brush) {
        unsafe {
            if radius > 0.0 {
                self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, brush)
            } else {
                self.dc.FillRectangle(&r.d2d(), brush)
            }
        }
    }

    /// `backdrop-filter: blur()` behind an overlay: blur what has been drawn so far.
    pub fn blur_backdrop(&self, radius: f32) {
        if radius < 0.5 {
            return;
        }
        unsafe {
            let Ok(target) = self.dc.GetTarget() else { return };
            let Ok(target) = target.cast::<ID2D1Bitmap1>() else { return };
            let size = target.GetPixelSize();
            let (mut dx, mut dy) = (0.0, 0.0);
            self.dc.GetDpi(&mut dx, &mut dy);
            // Same pixel format as the swap chain buffer, or the copy is refused.
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: dx,
                dpiY: dy,
                ..Default::default()
            };
            let copy = match self.dc.CreateBitmap(size, None, 0, &props) {
                Ok(c) => c,
                Err(e) => return crate::startup_mark(&format!("blur bitmap: {e}")),
            };
            let _ = self.dc.Flush(None, None);
            if let Err(e) = copy.CopyFromBitmap(None, &target, None) {
                return crate::startup_mark(&format!("blur copy: {e}"));
            }
            let Ok(effect) = self.dc.CreateEffect(&CLSID_D2D1GaussianBlur) else { return };
            effect.SetInput(0, &copy, true);
            let _ = effect.SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &radius.to_le_bytes());
            let _ = effect.SetValue(D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_BORDER_MODE_HARD.0 as u32).to_le_bytes());
            let mut old = Matrix3x2::default();
            self.dc.GetTransform(&mut old);
            self.dc.SetTransform(&Matrix3x2::identity());
            if let Ok(out) = effect.GetOutput() {
                self.dc.DrawImage(&out, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            }
            self.dc.SetTransform(&old);
        }
    }

    /// Stroke an SVG path (`M L H V A Z`, absolute and relative) from the
    /// original icons, so they look the same as in the web version.
    pub fn stroke_svg(&self, d: &str, x: f32, y: f32, scale: f32, c: Color, width: f32) {
        let Some(geo) = svg_geometry(&self.factory, d) else { return };
        unsafe {
            let old = {
                let mut m = Matrix3x2::default();
                self.dc.GetTransform(&mut m);
                m
            };
            self.dc.SetTransform(&(Matrix3x2::scale(scale, scale) * Matrix3x2::translation(x, y) * old));
            let props = D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
            let style = self.factory.CreateStrokeStyle(&props, None).ok().and_then(|s| s.cast::<ID2D1StrokeStyle>().ok());
            self.dc.DrawGeometry(&geo, self.color(c), width / scale, style.as_ref());
            self.dc.SetTransform(&old);
        }
    }

    // ---- drawing primitives ------------------------------------------------

    pub fn color(&self, c: Color) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&c) };
        &self.brush
    }

    pub fn fill(&self, r: Rect, c: Color) {
        unsafe { self.dc.FillRectangle(&r.d2d(), self.color(c)) }
    }

    pub fn fill_round(&self, r: Rect, radius: f32, c: Color) {
        unsafe { self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, self.color(c)) }
    }

    pub fn stroke_round(&self, r: Rect, radius: f32, c: Color, width: f32) {
        let r = r.inset(width / 2.0);
        unsafe { self.dc.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, self.color(c), width, None) }
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color, width: f32) {
        unsafe { self.dc.DrawLine(Vector2 { X: x0, Y: y0 }.into(), Vector2 { X: x1, Y: y1 }.into(), self.color(c), width, None) }
    }

    pub fn text(&self, l: &IDWriteTextLayout, x: f32, y: f32, c: Color) {
        unsafe { self.dc.DrawTextLayout(Vector2 { X: x, Y: y }.into(), l, self.color(c), D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) }
    }

    /// Soft drop shadow: stacked translucent rounded rects, cheap on WARP
    /// where a Gaussian blur effect per frame is not.
    pub fn shadow(&self, r: Rect, radius: f32, spread: f32, offset_y: f32, alpha: f32) {
        let steps = 6;
        for i in 0..steps {
            let k = (i + 1) as f32 / steps as f32;
            let grow = spread * (1.0 - k);
            let a = alpha * k * k / steps as f32 * 2.0;
            self.fill_round(Rect { x: r.x - grow, y: r.y - grow + offset_y, w: r.w + grow * 2.0, h: r.h + grow * 2.0 }, radius + grow, Color { r: 0.0, g: 0.0, b: 0.0, a });
        }
    }

    pub fn clip(&self, r: Rect) {
        unsafe { self.dc.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) }
    }

    pub fn unclip(&self) {
        unsafe { self.dc.PopAxisAlignedClip() }
    }

    pub fn translate(&self, x: f32, y: f32) {
        unsafe { self.dc.SetTransform(&Matrix3x2::translation(x, y)) }
    }

    pub fn reset_transform(&self) {
        unsafe { self.dc.SetTransform(&Matrix3x2::identity()) }
    }

    /// Run `f` inside an opacity layer (fades, transitions).
    pub fn with_opacity(&self, opacity: f32, f: impl FnOnce()) {
        if opacity >= 0.999 {
            return f();
        }
        unsafe {
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity,
                ..Default::default()
            };
            self.dc.PushLayer(&params, None);
            f();
            self.dc.PopLayer();
        }
    }
}

fn svg_geometry(factory: &ID2D1Factory1, d: &str) -> Option<ID2D1PathGeometry1> {
    enum Tok {
        Cmd(char),
        Num(f32),
    }
    let mut toks = vec![];
    let mut num = String::new();
    let flush = |num: &mut String, toks: &mut Vec<Tok>| {
        if let Ok(v) = num.parse() {
            toks.push(Tok::Num(v));
        }
        num.clear();
    };
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            flush(&mut num, &mut toks);
            toks.push(Tok::Cmd(c));
        } else if c == ',' || c.is_whitespace() {
            flush(&mut num, &mut toks);
        } else if c == '-' && !num.is_empty() {
            flush(&mut num, &mut toks);
            num.push(c);
        } else if c == '.' && num.contains('.') {
            flush(&mut num, &mut toks);
            num.push(c);
        } else {
            num.push(c);
        }
    }
    flush(&mut num, &mut toks);
    unsafe {
        let geo = factory.CreatePathGeometry().ok()?;
        let sink = geo.Open().ok()?;
        let pt = |x: f32, y: f32| Vector2 { X: x, Y: y }.into();
        let (mut cx, mut cy, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut open = false;
        let mut cmd = 'M';
        let mut i = 0;
        let n = |i: &mut usize| -> f32 {
            let v = match toks.get(*i) {
                Some(Tok::Num(v)) => *v,
                _ => 0.0,
            };
            *i += 1;
            v
        };
        while i < toks.len() {
            if let Tok::Cmd(c) = toks[i] {
                cmd = c;
                i += 1;
                if matches!(c, 'Z' | 'z') {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                    (cx, cy) = (sx, sy);
                    continue;
                }
            }
            let rel = cmd.is_ascii_lowercase();
            let (bx, by) = if rel { (cx, cy) } else { (0.0, 0.0) };
            match cmd.to_ascii_uppercase() {
                'M' => {
                    let (x, y) = (n(&mut i) + bx, n(&mut i) + by);
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(pt(x, y), D2D1_FIGURE_BEGIN_HOLLOW);
                    open = true;
                    (cx, cy, sx, sy) = (x, y, x, y);
                    // Further pairs after a moveto are linetos.
                    cmd = if rel { 'l' } else { 'L' };
                }
                'L' => {
                    let (x, y) = (n(&mut i) + bx, n(&mut i) + by);
                    sink.AddLine(pt(x, y));
                    (cx, cy) = (x, y);
                }
                'H' => {
                    cx = n(&mut i) + bx;
                    sink.AddLine(pt(cx, cy));
                }
                'V' => {
                    cy = n(&mut i) + by;
                    sink.AddLine(pt(cx, cy));
                }
                'A' => {
                    let (rx, ry, rot, large, sweep) = (n(&mut i), n(&mut i), n(&mut i), n(&mut i), n(&mut i));
                    let (x, y) = (n(&mut i) + bx, n(&mut i) + by);
                    sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: pt(x, y),
                        size: D2D_SIZE_F { width: rx, height: ry },
                        rotationAngle: rot,
                        sweepDirection: if sweep != 0.0 { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                        arcSize: if large != 0.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                    });
                    (cx, cy) = (x, y);
                }
                _ => i += 1,
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close().ok()?;
        Some(geo)
    }
}

fn whole(w: &[u16]) -> DWRITE_TEXT_RANGE {
    DWRITE_TEXT_RANGE { startPosition: 0, length: w.len() as u32 }
}

/// Font files loaded from the `fonts` directory at startup (scripts/fonts.ps1
/// fetches them). A missing file leaves its role on the Windows fallback.
const BUNDLED_FONTS: &[&str] = &[
    "NotoSerifSC-VF.ttf",
    "LXGWWenKai-Regular.ttf",
    "lmroman10-regular.otf",
    "lmroman10-bold.otf",
    "lmroman10-italic.otf",
    "lmroman10-bolditalic.otf",
];

fn family_in(c: &IDWriteFontCollection1, name: &str) -> bool {
    let (mut index, mut exists) = (0, BOOL(0));
    unsafe { c.FindFamilyName(PCWSTR(wide_z(name).as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() }
}

/// The bundled faces as their own small collection.
fn font_collection(dw: &IDWriteFactory5, dir: Option<&Path>) -> Result<IDWriteFontCollection1> {
    unsafe {
        let builder = dw.CreateFontSetBuilder()?;
        if let Some(dir) = dir {
            for name in BUNDLED_FONTS {
                let path = dir.join(name);
                if path.exists() {
                    let file = dw.CreateFontFileReference(PCWSTR(wide_z(&path.to_string_lossy()).as_ptr()), None)?;
                    builder.AddFontFile(&file)?;
                }
            }
        }
        dw.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
    pub fn right(self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn inset(self, d: f32) -> Rect {
        Rect { x: self.x + d, y: self.y + d, w: (self.w - 2.0 * d).max(0.0), h: (self.h - 2.0 * d).max(0.0) }
    }
    pub fn pad(self, x: f32, y: f32) -> Rect {
        Rect { x: self.x + x, y: self.y + y, w: (self.w - 2.0 * x).max(0.0), h: (self.h - 2.0 * y).max(0.0) }
    }
    pub fn cut_left(&mut self, w: f32) -> Rect {
        let r = Rect { w, ..*self };
        self.x += w;
        self.w = (self.w - w).max(0.0);
        r
    }
    pub fn cut_right(&mut self, w: f32) -> Rect {
        self.w = (self.w - w).max(0.0);
        Rect { x: self.x + self.w, w, ..*self }
    }
    pub fn cut_top(&mut self, h: f32) -> Rect {
        let r = Rect { h, ..*self };
        self.y += h;
        self.h = (self.h - h).max(0.0);
        r
    }
    pub fn cut_bottom(&mut self, h: f32) -> Rect {
        self.h = (self.h - h).max(0.0);
        Rect { y: self.y + self.h, h, ..*self }
    }
}

pub struct Metrics {
    pub w: f32,
    pub h: f32,
}

pub fn metrics(l: &IDWriteTextLayout) -> Metrics {
    let mut m = DWRITE_TEXT_METRICS::default();
    unsafe { l.GetMetrics(&mut m).unwrap() };
    Metrics { w: m.widthIncludingTrailingWhitespace, h: m.height }
}

pub use windows_numerics::Vector2;
