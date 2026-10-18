//! Document images: decoded off the UI thread with WIC, uploaded as D2D
//! bitmaps, keyed by absolute path. Remote URLs are not fetched — the editor
//! never makes network requests on its own.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use windows::Win32::Foundation::{GENERIC_READ, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::PCWSTR;
use windows_numerics::Matrix3x2;

use crate::gfx::{Gfx, Rect, Stack, Style, metrics};
use crate::theme::{Theme, with_alpha};

/// Posted when a decode finished; the window drains `Images::poll`.
pub const WM_APP_IMAGE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;

enum Entry {
    Loading,
    Ready { bmp: ID2D1Bitmap, w: f32, h: f32, at: std::time::Instant },
    Missing,
}

/// Decoded pixels; `natural` is the CSS size (an SVG is rasterised larger).
struct Pixels {
    w: u32,
    h: u32,
    px: Vec<u8>,
    natural: (f32, f32),
}

type Decoded = (PathBuf, Option<Pixels>);

pub struct Images {
    map: RefCell<HashMap<PathBuf, Entry>>,
    tx: Sender<Decoded>,
    rx: Receiver<Decoded>,
    pub hwnd: HWND,
    /// Display scale: SVGs are rasterised for the pixels they will cover, no more.
    pub scale: f32,
    /// Bumped when an image arrives, so line layouts reserve its real size.
    pub generation: u64,
}

impl Default for Images {
    fn default() -> Self {
        let (tx, rx) = channel();
        Images { map: RefCell::default(), tx, rx, hwnd: HWND::default(), scale: 1.0, generation: 0 }
    }
}

/// Where a markdown image target points on disk; None for remote or data URLs.
pub fn resolve(dir: Option<&Path>, src: &str) -> Option<PathBuf> {
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("data:") || src.starts_with("//") {
        return None;
    }
    let path = mdwisp_core::paths::decode_uri(src.split(['?', '#']).next().unwrap_or(""));
    let path = path.strip_prefix("file:///").map(str::to_string).unwrap_or(path);
    let p = Path::new(&path);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    Some(mdwisp_core::paths::resolve(dir?, p))
}

impl Images {
    /// Natural size once decoded; starts the decode on first sight.
    pub fn size(&self, dir: Option<&Path>, src: &str) -> Option<(f32, f32)> {
        let path = resolve(dir, src)?;
        let mut map = self.map.borrow_mut();
        match map.get(&path) {
            Some(Entry::Ready { w, h, .. }) => Some((*w, *h)),
            Some(_) => None,
            None => {
                map.insert(path.clone(), Entry::Loading);
                let tx = self.tx.clone();
                let hwnd = self.hwnd.0 as isize;
                let scale = self.scale;
                std::thread::spawn(move || {
                    let out = decode(&path, scale);
                    let _ = tx.send((path, out));
                    unsafe {
                        let _ = PostMessageW(Some(HWND(hwnd as _)), WM_APP_IMAGE, WPARAM(0), LPARAM(0));
                    }
                });
                None
            }
        }
    }

    /// Upload finished decodes; true when anything arrived.
    pub fn poll(&mut self, g: &Gfx) -> bool {
        let mut any = false;
        while let Ok((path, data)) = self.rx.try_recv() {
            let entry = match data.and_then(|p| upload(g, p.w, p.h, &p.px).map(|b| (b, p.natural))) {
                Some((bmp, (w, h))) => Entry::Ready { bmp, w, h, at: std::time::Instant::now() },
                None => Entry::Missing,
            };
            self.map.borrow_mut().insert(path, entry);
            any = true;
        }
        if any {
            self.generation += 1;
        }
        any
    }

    /// Forget everything so edited files on disk are re-read.
    pub fn clear(&mut self) {
        self.map.borrow_mut().clear();
        self.generation += 1;
    }

/// `object-fit: cover` into `r` (sidebar thumbnails); nothing until decoded.
    pub fn draw_cover(&self, g: &Gfx, path: &Path, r: Rect) {
        let map = self.map.borrow();
        let Some(Entry::Ready { bmp, .. }) = map.get(path) else { return };
        unsafe {
            let size = bmp.GetSize();
            let k = (r.w / size.width).max(r.h / size.height);
            let (dw, dh) = (size.width * k, size.height * k);
            let props = D2D1_BITMAP_BRUSH_PROPERTIES { extendModeX: D2D1_EXTEND_MODE_CLAMP, extendModeY: D2D1_EXTEND_MODE_CLAMP, interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR };
            let rt: &ID2D1RenderTarget = &g.dc;
            if let Ok(brush) = rt.CreateBitmapBrush(bmp, Some(&props), None) {
                brush.SetTransform(&(Matrix3x2::scale(k, k) * Matrix3x2::translation(r.x + (r.w - dw) / 2.0, r.y + (r.h - dh) / 2.0)));
                g.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: 5.0, radiusY: 5.0 }, &brush);
            }
        }
    }

    pub fn draw(&self, g: &Gfx, t: &Theme, dir: Option<&Path>, src: &str, alt: &str, r: Rect) {
        let path = resolve(dir, src);
        let map = self.map.borrow();
        match path.as_ref().and_then(|p| map.get(p)) {
            Some(Entry::Ready { bmp, at, .. }) => unsafe {
                let motion = crate::app().ui_motion();
                let k = (at.elapsed().as_secs_f32() * 1000.0 / crate::theme::Dur::Slow.ms(motion)).min(1.0);
                let e = crate::theme::Ease::Out.at(k);
                if k < 1.0 {
                    crate::win::request_frame(self.hwnd);
                }
                let grow = 0.014 * (1.0 - e);
                let r = Rect::new(r.x - r.w * grow / 2.0, r.y - r.h * grow / 2.0, r.w * (1.0 + grow), r.h * (1.0 + grow));
                if e < 0.999 {
                    let params = D2D1_LAYER_PARAMETERS1 {
                        contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: Matrix3x2::identity(),
                        opacity: e,
                        ..Default::default()
                    };
                    g.dc.PushLayer(&params, None);
                }
                let size = bmp.GetSize();
                // Rounded corners: fill the box with the bitmap as a brush.
                let props = D2D1_BITMAP_BRUSH_PROPERTIES { extendModeX: D2D1_EXTEND_MODE_CLAMP, extendModeY: D2D1_EXTEND_MODE_CLAMP, interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR };
                let rt: &ID2D1RenderTarget = &g.dc;
                if let Ok(brush) = rt.CreateBitmapBrush(bmp, Some(&props), None) {
                    brush.SetTransform(&(Matrix3x2::scale(r.w / size.width, r.h / size.height) * Matrix3x2::translation(r.x, r.y)));
                    g.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: 8.0, radiusY: 8.0 }, &brush);
                }
                if e < 0.999 {
                    g.dc.PopLayer();
                }
            },
            Some(Entry::Loading) => g.fill_round(r, 8.0, t.bg_raised),
            _ => {
                // A missing asset is a designed state, not a broken-image glyph.
                g.fill_round(r, 8.0, t.bg_raised);
                g.clip(r);
                let mut x = r.x - r.h;
                while x < r.right() {
                    g.line(x, r.bottom(), x + r.h, r.y, with_alpha(t.bg_inset, 0.9), 8.0);
                    x += 22.6;
                }
                g.unclip();
                g.stroke_round(r, 8.0, t.border_strong, 1.0);
                if !alt.is_empty() {
                    let l = g.layout(alt, Style::new(Stack::Ui, crate::theme::TEXT_SM), r.w - 16.0);
                    let m = metrics(&l);
                    g.text(&l, r.x + (r.w - m.w) / 2.0, r.y + (r.h - m.h) / 2.0, t.text_tertiary);
                }
            }
        }
    }
}

fn decode(path: &Path, scale: f32) -> Option<Pixels> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        return svg(path, scale);
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let wpath: Vec<u16> = path.as_os_str().encode_wide_z();
        let dec = factory.CreateDecoderFromFilename(PCWSTR(wpath.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).ok()?;
        // Huge photos are scaled down: nothing is shown wider than a screen.
        let source: IWICBitmapSource = if w > 4096 {
            let scaler = factory.CreateBitmapScaler().ok()?;
            let nh = (h as u64 * 4096 / w as u64).max(1) as u32;
            scaler.Initialize(&frame, 4096, nh, WICBitmapInterpolationModeFant).ok()?;
            (w, h) = (4096, nh);
            scaler.into()
        } else {
            frame.into()
        };
        let conv = factory.CreateFormatConverter().ok()?;
        conv.Initialize(&source, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        let mut px = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut px).ok()?;
        Some(Pixels { w, h, px, natural: (w as f32, h as f32) })
    }
}

/// SVG through resvg: Direct2D's own SVG support draws no `<text>`, and
/// diagrams are mostly text. Rasterised at the display scale so it stays sharp
/// on HiDPI without wasting memory at 100%; the returned size is the natural one.
fn svg(path: &Path, display: f32) -> Option<Pixels> {
    use resvg::{tiny_skia, usvg};
    static FONTS: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        std::sync::Arc::new(db)
    });
    let data = std::fs::read(path).ok()?;
    let opt = usvg::Options { resources_dir: path.parent().map(Into::into), fontdb: fonts.clone(), ..Default::default() };
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;
    let size = tree.size();
    let scale = display.max(1.0).min(4096.0 / size.width().max(size.height()));
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pix = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    let mut px = pix.take();
    // tiny-skia is premultiplied RGBA; Direct2D wants premultiplied BGRA.
    for p in px.chunks_mut(4) {
        p.swap(0, 2);
    }
    Some(Pixels { w, h, px, natural: (size.width(), size.height()) })
}

fn upload(g: &Gfx, w: u32, h: u32, px: &[u8]) -> Option<ID2D1Bitmap> {
    unsafe {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let rt: &ID2D1RenderTarget = &g.dc;
        rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(px.as_ptr() as _), w * 4, &props).ok()
    }
}

trait WideZ {
    fn encode_wide_z(&self) -> Vec<u16>;
}

impl WideZ for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}

fn wic() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

fn encode_png(factory: &IWICImagingFactory, source: &IWICBitmapSource) -> Option<Vec<u8>> {
    unsafe {
        let mem = windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal(Default::default(), true).ok()?;
        let enc = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
        enc.Initialize(&mem, WICBitmapEncoderNoCache).ok()?;
        let mut frame = None;
        enc.CreateNewFrame(&mut frame, std::ptr::null_mut()).ok()?;
        let frame = frame?;
        frame.Initialize(None).ok()?;
        frame.WriteSource(source, std::ptr::null()).ok()?;
        frame.Commit().ok()?;
        enc.Commit().ok()?;
        let mut stat = windows::Win32::System::Com::STATSTG::default();
        mem.Stat(&mut stat, windows::Win32::System::Com::STATFLAG_NONAME).ok()?;
        let len = stat.cbSize as usize;
        let _ = mem.Seek(0, windows::Win32::System::Com::STREAM_SEEK_SET, None);
        let mut out = vec![0u8; len];
        let mut read = 0;
        let _ = mem.Read(out.as_mut_ptr() as _, len as u32, Some(&mut read));
        out.truncate(read as usize);
        Some(out)
    }
}

/// Scale an encoded image down to `max_width` as PNG; None when it already fits.
pub fn downscale(data: &[u8], max_width: u32) -> Option<Vec<u8>> {
    unsafe {
        let f = wic()?;
        let stream = f.CreateStream().ok()?;
        stream.InitializeFromMemory(data).ok()?;
        let dec = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).ok()?;
        if max_width == 0 || w <= max_width {
            return None;
        }
        let scaler = f.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, max_width, (h as u64 * max_width as u64 / w as u64).max(1) as u32, WICBitmapInterpolationModeFant).ok()?;
        encode_png(&f, &scaler.into())
    }
}

/// A clipboard DIB (header + pixels, as CF_DIB delivers it) as PNG bytes.
pub fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    // Prefix a BITMAPFILEHEADER so WIC's BMP decoder can read it.
    let header_size = u32::from_le_bytes(dib.get(0..4)?.try_into().ok()?) as usize;
    let bit_count = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = u32::from_le_bytes(dib.get(16..20)?.try_into().ok()?);
    let colors = u32::from_le_bytes(dib.get(32..36)?.try_into().ok()?) as usize;
    let masks = if compression == 3 && header_size == 40 { 12 } else { 0 };
    let palette = if bit_count <= 8 { (if colors == 0 { 1usize << bit_count } else { colors }) * 4 } else { colors * 4 };
    let offset = 14 + header_size + masks + palette;
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&(offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    unsafe {
        let f = wic()?;
        let stream = f.CreateStream().ok()?;
        stream.InitializeFromMemory(&bmp).ok()?;
        let dec = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        encode_png(&f, &frame.into())
    }
}
