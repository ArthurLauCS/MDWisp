//! Window layout: title bar on top, then sidebar · document · history dock,
//! and the overlays above them. Also routes pointer and keyboard input that
//! belongs to the chrome rather than to the editor.

mod history;
mod menu;
mod modal;
mod pane;
mod panels;
mod sidebar;
mod titlebar;
pub mod widgets;

use std::path::PathBuf;

use mdwisp_core::i18n::t;
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

use crate::gfx::{Gfx, Rect};
use crate::keys::Mods;
use crate::theme::{S2, SIDE_PANEL_MAX_W, SIDE_PANEL_MIN_W, TITLEBAR_H};
use crate::ui::Hit;
use crate::window::Window;
use crate::{app, dialog, win};

pub use modal::Modal;

#[derive(Default)]
pub struct Shell {
    pub sidebar: sidebar::State,
    pub history: history::State,
    drag: Option<Drag>,
    pub table_bar: Option<Rect>,
    pub table_scroll: f32,
    sidebar_shown: Option<bool>,
}

#[derive(Clone, Copy)]
enum Drag {
    Sidebar { x0: f32, w0: f32 },
    History { x0: f32, w0: f32 },
    Split { y0: f32, r0: f32, h: f32 },
    MarginLeft { x0: f32, v0: f32, w: f32 },
    MarginRight { x0: f32, v0: f32, w: f32 },
    Thumb { y0: f32, s0: f32 },
}

/// Below this width the sidebar costs more than it gives.
const NARROW: f32 = 900.0;

pub fn sidebar_width(w: &Window) -> f32 {
    (app().s().sidebar_width as f32).clamp(SIDE_PANEL_MIN_W, SIDE_PANEL_MAX_W.min(w.width * 0.35).max(SIDE_PANEL_MIN_W))
}

pub fn paint(w: &mut Window, g: &Gfx) {
    let mut body = w.bounds();
    let bar = body.cut_top(TITLEBAR_H);
    titlebar::paint(w, g, bar);
    let s = app().s();
    let narrow = w.width <= NARROW;
    let rail = history::paint(w, g, &mut body);
    let _ = rail;
    let shown = s.sidebar_visible;
    if w.shell.sidebar_shown.is_some_and(|was| !was) && shown {
        w.ui.set((Hit::SidebarResize, 6), 0.0);
        w.ui.animate((Hit::SidebarResize, 6), 1.0, crate::theme::Dur::Base, crate::theme::Ease::Out);
    }
    w.shell.sidebar_shown = Some(shown);
    let enter = w.ui.value((Hit::SidebarResize, 6), 1.0);
    if shown && !narrow {
        let sw = sidebar_width(w);
        let side = body.cut_left(sw);
        g.with_opacity(enter, || {
            g.translate(-12.0 * (1.0 - enter), 0.0);
            sidebar::paint(w, g, side);
            g.reset_transform();
        });
    }
    pane::paint(w, g, body);
    if s.sidebar_visible && narrow {
        // An overlay over the editor on narrow windows; a click off it dismisses it.
        let th = app().theme;
        g.fill(body, crate::theme::with_alpha(th.bg_base, 0.5));
        w.ui.region(body, Hit::Backdrop);
        let side = Rect::new(body.x, body.y, sidebar_width(w), body.h);
        g.shadow(side, 0.0, 24.0, 8.0, 0.46 * th.shadow);
        sidebar::paint(w, g, side);
    }
    modal::paint(w, g);
    pane::toast(w, g);
}

/// The draggable page margins inside the editor surface.
pub fn margins(w: &mut Window, g: &Gfx, r: Rect) {
    let Some(d) = &w.doc else { return };
    let th = app().theme;
    let left = d.view.content_x;
    let right = d.view.content_x + d.view.content_w;
    for (x, hit) in [(left - S2, Hit::MarginLeft), (right, Hit::MarginRight)] {
        let h = Rect::new(x, r.y, S2, r.h);
        let hv = w.ui.hover_t(hit).max(if matches!(w.shell.drag, Some(Drag::MarginLeft { .. }) if hit == Hit::MarginLeft) || matches!(w.shell.drag, Some(Drag::MarginRight { .. }) if hit == Hit::MarginRight) { 1.0 } else { 0.0 });
        if hv > 0.0 {
            g.fill(h, crate::theme::with_alpha(th.accent_soft, hv));
            g.stroke_round(h, 0.0, crate::theme::with_alpha(th.accent_line, hv), 1.0);
        }
        g.fill(Rect::new(x + 4.0, r.y + 12.0, 2.0, 24.0), th.border_strong);
        w.ui.region(h, hit);
    }
}

// ---- pointer ----------------------------------------------------------------

pub fn pointer_down(w: &mut Window, hit: Option<Hit>, x: f32, y: f32, button: u8) -> bool {
    if w.modal.is_some() {
        return modal::pointer_down(w, hit, x, y, button);
    }
    if button != 0 {
        return false;
    }
    let s = app().s();
    w.shell.drag = match hit {
        Some(Hit::SidebarResize) => Some(Drag::Sidebar { x0: x, w0: sidebar_width(w) }),
        Some(Hit::HistoryResize) => Some(Drag::History { x0: x, w0: s.history_width as f32 }),
        Some(Hit::SidebarSplit) => Some(Drag::Split { y0: y, r0: s.assets_ratio as f32, h: w.shell.sidebar.split_h }),
        Some(Hit::MarginLeft) => Some(Drag::MarginLeft { x0: x, v0: s.page_margin_left as f32, w: w.doc.as_ref().map_or(1.0, |d| d.view.rect.w) }),
        Some(Hit::MarginRight) => Some(Drag::MarginRight { x0: x, v0: s.page_margin_right as f32, w: w.doc.as_ref().map_or(1.0, |d| d.view.rect.w) }),
        Some(Hit::EditorScrollbar) => w.doc.as_mut().map(|d| {
            // A press on the track outside the thumb jumps there first.
            if let Some(th) = d.view.thumb().filter(|th| !(y >= th.y && y < th.bottom())) {
                d.view.thumb_drag(d.view.scroll, y - th.y - th.h / 2.0);
            }
            Drag::Thumb { y0: y, s0: d.view.scroll }
        }),
        _ => None,
    };
    if w.shell.drag.is_some() {
        return true;
    }
    // Editor widgets act on click, and must not move the caret on the way down.
    if matches!(hit, Some(Hit::Widget(_))) {
        return true;
    }
    sidebar::pointer_down(w, hit, x, y) || history::pointer_down(w, hit, x, y)
}

pub fn drag(w: &mut Window, x: f32, y: f32) -> bool {
    let Some(d) = w.shell.drag else { return modal::drag(w, x, y) };
    match d {
        Drag::Sidebar { x0, w0 } => {
            let v = (w0 + x - x0).clamp(SIDE_PANEL_MIN_W, SIDE_PANEL_MAX_W) as f64;
            app().patch_live(|s| s.sidebar_width = v.round());
        }
        Drag::History { x0, w0 } => {
            let v = (w0 - (x - x0)).clamp(SIDE_PANEL_MIN_W, SIDE_PANEL_MAX_W) as f64;
            app().patch_live(|s| s.history_width = v.round());
        }
        Drag::Split { y0, r0, h } => {
            let v = (r0 - (y - y0) / h.max(1.0)).clamp(0.0, 1.0) as f64;
            app().patch_live(|s| s.assets_ratio = (v * 1000.0).round() / 1000.0);
        }
        Drag::MarginLeft { x0, v0, w: pw } => {
            let v = (v0 + (x - x0) / pw * 100.0).clamp(0.0, 35.0) as f64;
            app().patch_live(|s| s.page_margin_left = (v * 10.0).round() / 10.0);
        }
        Drag::Thumb { y0, s0 } => {
            if let Some(d) = &mut w.doc {
                d.view.thumb_drag(s0, y - y0);
            }
            w.ui.set((Hit::EditorScrollbar, 5), 1.0);
            return true;
        }
        Drag::MarginRight { x0, v0, w: pw } => {
            let v = (v0 - (x - x0) / pw * 100.0).clamp(0.0, 35.0) as f64;
            app().patch_live(|s| s.page_margin_right = (v * 10.0).round() / 10.0);
        }
    }
    true
}

pub fn drag_end(w: &mut Window) {
    match w.shell.drag.take() {
        Some(Drag::Thumb { .. }) => w.ui.animate((Hit::EditorScrollbar, 5), 0.0, crate::theme::Dur::Base, crate::theme::Ease::Out),
        Some(_) => {
            let _ = app().settings.persist();
        }
        None => {}
    }
    modal::drag_end(w);
}

pub fn wheel(w: &mut Window, dy: f32) -> bool {
    if w.modal.is_some() {
        return modal::wheel(w, dy);
    }
    let (x, y) = (w.ui.mx, w.ui.my);
    if w.shell.table_bar.is_some_and(|b| b.contains(x, y)) {
        w.shell.table_scroll = (w.shell.table_scroll - dy * 60.0).max(0.0);
        return true;
    }
    sidebar::wheel(w, x, y, dy) || history::wheel(w, x, y, dy)
}

/// The code-language picker for the fenced block under the caret.
pub fn pick_code_language(w: &mut Window) {
    let Some(d) = &w.doc else { return };
    if d.view.read_only {
        return;
    }
    let head = d.view.buf.sel.head;
    let Some((_, info, detected)) = d.view.deco.widgets.iter().find_map(|(_, wd)| match wd {
        crate::editor::deco::Widget::CodeLang { block, info, detected, .. } if block.start <= head && head <= block.end => Some((block.clone(), info.clone(), *detected)),
        _ => None,
    }) else {
        return;
    };
    let current = d.view.buf.text[info.clone()].to_string();
    let mut entries = vec![
        menu::Entry::Item { id: String::new(), title: detected.map_or_else(|| t("自动检测").to_string(), |l| mdwisp_core::i18n::tf("自动检测 · {0}", &[&l])), key: String::new(), disabled: false },
        menu::Entry::Item { id: "text".into(), title: t("纯文本").into(), key: String::new(), disabled: false },
        menu::Entry::Sep,
    ];
    let mut langs: Vec<&str> = mdwisp_text::highlight::languages().collect();
    if !current.is_empty() && current != "text" && !langs.contains(&current.as_str()) {
        langs.push(&current);
    }
    langs.sort();
    let langs: Vec<String> = langs.into_iter().map(String::from).collect();
    for l in langs {
        let key = if l == current { "✓".to_string() } else { String::new() };
        entries.push(menu::Entry::Item { id: l.clone(), title: l, key, disabled: false });
    }
    let (x, y, _) = caret_rect(w).unwrap_or((w.ui.mx, w.ui.my, 0.0));
    let mut m = menu::Menu::new(x, y + 20.0, entries);
    m.pick = Some(|w, lang| {
        let Some(d) = &mut w.doc else { return };
        let head = d.view.buf.sel.head;
        let info = d.view.deco.widgets.iter().find_map(|(_, wd)| match wd {
            crate::editor::deco::Widget::CodeLang { block, info, .. } if block.start <= head && head <= block.end => Some(info.clone()),
            _ => None,
        });
        if let Some(info) = info {
            d.view.replace(info.start, info.end, lang, head.min(info.start + lang.len()).max(head));
            w.after_input();
        }
    });
    w.modal = Some(Modal::Menu(m));
}

/// Clicks in the editor that hit a widget rather than text.
pub fn editor_click(w: &mut Window, pos: usize, x: f32, y: f32) -> bool {
    let _ = (pos, x, y);
    let Some(d) = &mut w.doc else { return false };
    d.view.image_selected = false;
    // Clicking an image selects it (Z views it fullscreen) instead of placing the caret.
    let yy = y - d.view.rect.y + d.view.scroll;
    let li = d.view.lines.partition_point(|l| l.y + l.h <= yy).min(d.view.lines.len().saturating_sub(1));
    if let Some(l) = d.view.lines.get(li) {
        if let Some(layout) = &l.shaped.layout {
            for (at, obj) in &l.shaped.objects {
                if let crate::editor::layout::Obj::Image { range, .. } = obj {
                    let (mut px, mut py) = (0.0, 0.0);
                    let mut m = windows::Win32::Graphics::DirectWrite::DWRITE_HIT_TEST_METRICS::default();
                    unsafe {
                        let _ = layout.HitTestTextPosition(*at, false, &mut px, &mut py, &mut m);
                    }
                    let bx = d.view.content_x + l.shaped.indent + px;
                    let by = d.view.rect.y - d.view.scroll + l.y + l.shaped.pad_top + m.top;
                    if x >= bx && x < bx + m.width && y >= by && y < by + m.height {
                        let start = range.start;
                        d.view.move_to(start, false);
                        d.view.image_selected = true;
                        return true;
                    }
                }
            }
        }
        // The closing fence of a code block, or below a block that ends the document:
        // continue writing after it instead of inside it.
        let below_end = yy > l.y + l.h && li + 1 == d.view.lines.len();
        if l.deco.code == crate::editor::deco::Code::End && (below_end || (yy >= l.y && yy < l.y + l.h)) && !d.view.read_only {
            let end = l.src.end;
            d.view.leave_code_block(end);
            w.after_input();
            return true;
        }
    }
    let Some(d) = &mut w.doc else { return false };
    // Task checkboxes toggle in place.
    let line = d.view.line_of(pos);
    if let Some(l) = d.view.lines.get(line) {
        for (_, obj) in &l.shaped.objects {
            if let crate::editor::layout::Obj::Checkbox { at, checked } = obj {
                if pos == *at || pos == at + 3 {
                    let at = *at;
                    let mark = if *checked { " " } else { "x" };
                    if !d.view.read_only {
                        d.view.replace(at + 1, at + 2, mark, d.view.buf.sel.head);
                        w.after_input();
                    }
                    return true;
                }
            }
        }
    }
    false
}

pub fn click(w: &mut Window, hit: Hit) {
    if w.modal.is_some() {
        modal::click(w, hit);
        return;
    }
    match hit {
        Hit::Minimize => unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(w.hwnd, windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE);
        },
        Hit::Close => unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(w.hwnd), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, Default::default(), Default::default());
        },
        Hit::SidebarToggle => crate::actions::run(w, "view.toggleSidebar"),
        Hit::TitleAction(i) => crate::actions::run(w, titlebar::ACTION_IDS[i as usize]),
        Hit::Breadcrumb(1) => crate::actions::run(w, "document.organize"),
        Hit::Backdrop => app().patch(|s| s.sidebar_visible = false),
        Hit::Widget(id) if id >= pane::TABLE_BUTTON && id < pane::TABLE_BAR => pane::table_button(w, (id - pane::TABLE_BUTTON) as usize),
        Hit::Widget(id) if id < pane::TABLE_BUTTON => {
            // A code block's language chip: the picker for that block.
            if let Some(d) = &mut w.doc {
                if let Some((r, _, _)) = d.view.deco.code_blocks.get(id as usize) {
                    let at = r.start;
                    if !(d.view.buf.sel.head >= r.start && d.view.buf.sel.head <= r.end) {
                        d.view.move_to(at, false);
                    }
                }
            }
            pick_code_language(w);
        }
        h => {
            if pane::search_click(w, h) {
                return;
            }
            sidebar::click(w, h);
            history::click(w, h);
        }
    }
}

pub fn context_menu(w: &mut Window, hit: Option<Hit>, x: f32, y: f32) {
    if w.modal.is_some() {
        return;
    }
    if hit == Some(Hit::Editor) {
        if let Some(d) = &mut w.doc {
            // A right click outside the selection moves the caret there first.
            let pos = d.view.pos_at(x, y);
            let sel = d.view.buf.sel;
            if sel.empty() || pos < sel.from() || pos > sel.to() {
                d.view.move_to(pos, false);
            }
        }
    }
    menu::open_context(w, hit, x, y);
}

// ---- keyboard ---------------------------------------------------------------

pub fn modal_key(w: &mut Window, vk: VIRTUAL_KEY, m: Mods) -> bool {
    modal::key(w, vk, m)
}

pub fn modal_text(w: &mut Window, s: &str) {
    modal::text(w, s);
}

/// A text field outside the editor has the keyboard (the find bar).
pub fn text_focus(w: &Window) -> bool {
    w.doc.as_ref().and_then(|d| d.view.search.as_ref()).is_some_and(|s| s.focus.is_some())
}

pub fn focused_text(w: &mut Window, s: &str) {
    let Some(search) = w.doc.as_mut().and_then(|d| d.view.search.as_mut()) else { return };
    match search.focus {
        Some(0) => search.find.insert(s),
        Some(_) => search.replace.insert(s),
        None => {}
    }
}

/// Keys for the focused find bar: Enter finds (or replaces), Tab switches fields.
pub fn search_key(w: &mut Window, vk: VIRTUAL_KEY, m: Mods) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let hwnd = w.hwnd;
    let Some(d) = &mut w.doc else { return false };
    let Some(s) = &mut d.view.search else { return false };
    let Some(f) = s.focus else { return false };
    match vk {
        VK_RETURN if f == 0 => {
            d.view.search_step(if m.shift { -1 } else { 1 });
        }
        VK_RETURN => {
            d.view.search_replace();
            w.after_input();
        }
        VK_TAB => s.focus = Some(if f == 0 && !d.view.read_only { 1 } else { 0 }),
        VK_ESCAPE => w.search_close(),
        _ => {
            let field = if f == 0 { &mut s.find } else { &mut s.replace };
            return field.key(vk, m, hwnd);
        }
    }
    true
}

impl Window {
    pub fn search_open(&mut self, replace: bool) {
        let Some(d) = &mut self.doc else { return };
        let selected = d.view.selected_text().to_string();
        let s = d.view.search.get_or_insert_with(Default::default);
        // A single-line selection becomes the query, like the browser's find.
        if !selected.is_empty() && !selected.contains('\n') {
            s.find = widgets::Field::new(&selected);
        }
        s.find.anchor = 0;
        s.find.caret = s.find.text.len();
        s.focus = Some(if replace && !d.view.read_only { 1 } else { 0 });
        win::request_frame(self.hwnd);
    }

    pub fn search_is_open(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.view.search.is_some())
    }

    pub fn search_close(&mut self) {
        if let Some(d) = &mut self.doc {
            d.view.search = None;
            d.view.focused = true;
        }
        win::request_frame(self.hwnd);
    }

    /// F3 / Shift+F3 open the bar when needed, then move to the next match.
    pub fn search_step(&mut self, dir: i32) {
        if !self.search_is_open() {
            self.search_open(false);
        }
        if let Some(d) = &mut self.doc {
            d.view.search_step(dir);
        }
        win::request_frame(self.hwnd);
    }
}

/// Where the IME should put its window, in client DIPs.
pub fn caret_rect(w: &Window) -> Option<(f32, f32, f32)> {
    if let Some(m) = &w.modal {
        return m.caret();
    }
    let d = w.doc.as_ref()?;
    let (x, y, h) = d.view.caret_at(d.view.buf.sel.head)?;
    Some((x, y - d.view.scroll + d.view.rect.y, h))
}

// ---- documents, links, images ----------------------------------------------

pub fn follow_link(w: &mut Window, href: &str) {
    let doc_path = w.doc.as_ref().and_then(|d| d.path()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    match mdwisp_core::paths::resolve_document_link(&doc_path, href) {
        Ok(mdwisp_core::paths::LinkTarget::External { url }) => dialog::shell_open(&url),
        Ok(mdwisp_core::paths::LinkTarget::Document { path, fragment }) => {
            let same = w.doc.as_ref().and_then(|d| d.path()).is_some_and(|p| p == path) || path.as_os_str().is_empty();
            if same {
                w.jump_to_fragment(&fragment);
            } else {
                w.open_in_new_window(Some(path), fragment);
            }
        }
        Err(e) => w.toast(&e),
    }
}

/// Dropped files: markdown opens, images are copied beside the document.
pub fn dropped(w: &mut Window, paths: Vec<PathBuf>) {
    let (docs, images): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| mdwisp_core::paths::has_md_ext(&p.to_string_lossy()));
    for p in docs {
        w.open_in_new_window(Some(p), String::new());
    }
    if !images.is_empty() {
        insert_images(w, &images);
    }
}

pub fn insert_images(w: &mut Window, files: &[PathBuf]) {
    let Some(d) = &mut w.doc else { return };
    let Some(path) = d.path().map(|p| p.to_path_buf()) else {
        w.toast(t("请先保存文档，再插入图片；图片会放在文档旁边。"));
        return;
    };
    let s = app().s().clone();
    match mdwisp_core::assets::insert_files(&path, files, &s, &crate::editor::images::downscale) {
        Ok(inserted) if !inserted.is_empty() => {
            let lines: Vec<String> = inserted.iter().zip(files).map(|(a, f)| format!("![{}]({})", alt_for(f), a.rel_path)).collect();
            // One image goes inline at the cursor; several get their own lines.
            let text = if lines.len() == 1 { lines[0].clone() } else { format!("\n\n{}\n\n", lines.join("\n")) };
            let at = d.view.buf.sel.head;
            d.view.replace(at, at, &text, at + text.len());
            w.after_input();
        }
        Ok(_) => {}
        Err(e) => w.toast(&e),
    }
}

/// Alt text from the original file name — never from the stored, hashed one.
fn alt_for(p: &std::path::Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| t("图片").into())
}

pub fn paste(w: &mut Window) {
    if app().s().read_only {
        return;
    }
    if crate::clipboard::has_image() {
        let Some(d) = &w.doc else { return };
        let Some(path) = d.path().map(|p| p.to_path_buf()) else {
            w.toast(t("请先保存文档，再粘贴图片；图片会放在文档旁边。"));
            return;
        };
        let Some(png) = crate::clipboard::dib(w.hwnd).and_then(|dib| crate::editor::images::dib_to_png(&dib)) else { return };
        let s = app().s().clone();
        match mdwisp_core::assets::insert(&path, png, None, "png", &s, &crate::editor::images::downscale) {
            Ok(a) => {
                let d = w.doc.as_mut().unwrap();
                let sel = d.view.buf.sel;
                let alt = if sel.empty() { t("图片").to_string() } else { d.view.selected_text().to_string() };
                let text = format!("![{alt}]({})", a.rel_path);
                d.view.replace(sel.from(), sel.to(), &text, sel.from() + text.len());
            }
            Err(e) => w.toast(&e),
        }
        return;
    }
    let Some(text) = crate::clipboard::text(w.hwnd) else { return };
    let Some(d) = &mut w.doc else { return };
    let sel = d.view.buf.sel;
    let link = (!sel.empty()).then(|| mdwisp_text::ops::paste_as_link(d.view.selected_text(), &text)).flatten();
    match link {
        Some(l) => d.view.replace(sel.from(), sel.to(), &l, sel.from() + l.len()),
        None => d.view.insert(&text),
    }
    let _ = win::request_frame;
}
