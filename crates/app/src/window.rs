//! One document window: its surface, pointer state and the views it shows.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mdwisp_core::session::{Session, SessionState};
use mdwisp_core::workspace::Workspace;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET};
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::HDROP;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::doc::Doc;
use crate::editor::layout::Look;
use crate::gfx::{Rect, Surface};
use crate::keys::Mods;
use crate::shell::Shell;
use crate::ui::{Cursor, Hit, Ui};
use crate::{app, shell, win};

const TIMER_BLINK: usize = 1;
const TIMER_SCRIPT: usize = 2;
const TIMER_TOAST: usize = 3;
/// Shapes lines that were only estimated for the first frame.
const TIMER_LAYOUT: usize = 4;

pub struct WinSession {
    pub store: Session,
    pub state: SessionState,
}

pub struct Window {
    pub hwnd: HWND,
    surface: Option<Surface>,
    /// Client size in DIPs.
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub ui: Ui,
    pub focused: bool,
    pub first_frame: bool,
    pub doc: Option<Doc>,
    pub session: WinSession,
    pub workspace: Workspace,
    pub shell: Shell,
    /// A persistent problem, shown above the document until resolved.
    pub error: Option<String>,
    pub toast_text: Option<(String, Instant)>,
    /// Overlay that owns keyboard input while open (palette, panels, menus).
    pub modal: Option<shell::Modal>,
    /// A panel set aside while a choice list it opened is up.
    pub parked: Option<shell::Modal>,
    /// Scroll the caret into view on the next paint (restored sessions).
    pub pending_reveal: bool,
    pending_fragment: String,
    /// A high surrogate waiting for its pair from WM_CHAR.
    surrogate: Option<u16>,
    last_click: (Instant, usize, u8),
    pub script: Option<crate::script::Script>,
    /// Save the next frame here (scripted runs).
    pub shot: Option<PathBuf>,
    /// Close without the save question (scripted runs).
    pub force_close: bool,
}

impl Window {
    pub fn new(restore: bool) -> Box<Window> {
        let mut store = Session::new(&app().user_data, restore);
        let state = store.boot().unwrap_or_default();
        Box::new(Window {
            hwnd: HWND::default(),
            surface: None,
            width: 0.0,
            height: 0.0,
            scale: 1.0,
            ui: Ui::new(),
            focused: false,
            first_frame: true,
            doc: None,
            session: WinSession { store, state },
            workspace: Workspace::default(),
            shell: Shell::default(),
            error: None,
            toast_text: None,
            modal: None,
            parked: None,
            pending_reveal: false,
            pending_fragment: String::new(),
            surrogate: None,
            last_click: (Instant::now(), usize::MAX, 0),
            script: crate::script::Script::from_env(),
            shot: None,
            force_close: false,
        })
    }

    /// Before the window exists: the document, plus the workspace scanned off-thread once it does.
    pub fn load_initial(&mut self, path: Option<&Path>, fragment: String) -> Result<(), String> {
        self.pending_fragment = fragment;
        match path {
            Some(p) => {
                let c = mdwisp_core::documents::read(p)?;
                self.doc = Some(Doc::new(Some(c.meta), c.text));
            }
            None => self.doc = Some(Doc::new(None, String::new())),
        }
        // The first paint comes from inside CreateWindow, before `created`.
        self.apply_settings();
        Ok(())
    }

    /// First message after creation: hook up everything that needs the HWND.
    pub fn created(&mut self) {
        if let Some(mut d) = self.doc.take() {
            d.view.images.hwnd = self.hwnd;
            d.view.images.scale = self.scale;
            self.set_doc(d);
        }
        let root = self.session.state.workspace_root.clone().filter(|_| self.session.state.workspace_root.is_some());
        if let Some(root) = root {
            self.open_workspace(&root);
        }
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_BLINK, 530, None);
            if self.script.is_some() {
                SetTimer(Some(self.hwnd), TIMER_SCRIPT, 30, None);
            }
            windows::Win32::UI::Shell::DragAcceptFiles(self.hwnd, true);
        }
    }

    /// Scan a folder off the UI thread; the tree appears when it is ready.
    pub fn open_workspace(&mut self, root: &Path) {
        let root = root.to_path_buf();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let mut ws = Workspace::default();
            let r = ws.open(&root).map(|_| ());
            win::post(hwnd, move |w| match r {
                Ok(()) => {
                    w.workspace = ws;
                    w.touch_session();
                }
                Err(e) => w.error = Some(e),
            });
        });
    }

    pub fn apply_settings(&mut self) {
        let s = app().s();
        self.ui.motion = match s.motion {
            mdwisp_core::settings::MotionLevel::Full => crate::theme::Motion::Full,
            mdwisp_core::settings::MotionLevel::Reduced => crate::theme::Motion::Reduced,
            mdwisp_core::settings::MotionLevel::Off => crate::theme::Motion::Off,
        };
        let t = app().theme;
        if let Some(d) = &mut self.doc {
            d.view.set_look(Look {
                read: (s.font_size as f32).to_bits(),
                line_height: (s.line_height as f32).to_bits(),
                code: (s.code_font_size as f32).to_bits(),
                tab: s.tab_size as usize,
                font_ui: s.font_ui.clone(),
                font_read: s.font_read.clone(),
                font_code: s.font_code.clone(),
                font_display: s.font_display.clone(),
                source: s.source_mode,
                theme: theme_key(&t),
            });
            d.view.read_only = s.read_only;
            d.view.highlight_line = s.highlight_current_line;
            d.view.margin_left = s.page_margin_left as f32;
            d.view.margin_right = s.page_margin_right as f32;
            d.view.cursor_style = s.cursor_style;
        }
        unsafe {
            let dark = windows::core::BOOL(t.dark as i32);
            let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(self.hwnd, windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
        }
        self.update_title();
    }

    pub fn update_title(&self) {
        let title = match &self.doc {
            Some(d) => format!("{}{} - MDWisp", if d.dirty() { "● " } else { "" }, d.name()),
            None => "MDWisp".into(),
        };
        let w: Vec<u16> = title.encode_utf16().chain([0]).collect();
        unsafe {
            let _ = SetWindowTextW(self.hwnd, windows::core::PCWSTR(w.as_ptr()));
        }
    }

    pub fn toast(&mut self, text: &str) {
        self.toast_text = Some((text.to_string(), Instant::now()));
        self.ui.set((Hit::Panel, 7), 0.0);
        self.ui.animate((Hit::Panel, 7), 1.0, crate::theme::Dur::Base, crate::theme::Ease::Spring);
        unsafe { SetTimer(Some(self.hwnd), TIMER_TOAST, 2200, None) };
        win::request_frame(self.hwnd);
    }

    pub fn history_changed(&mut self) {
        self.shell.history.stale = true;
        win::request_frame(self.hwnd);
    }

    pub fn resized(&mut self, w: u32, h: u32, scale: f32) {
        self.scale = scale;
        if let Some(d) = &mut self.doc {
            d.view.images.scale = scale;
        }
        self.width = w as f32 / scale;
        self.height = h as f32 / scale;
        let g = app().gfx();
        match &self.surface {
            Some(s) => g.resize(s, w, h),
            None => self.surface = g.surface(self.hwnd, w, h).ok(),
        }
        // Repaint inside the size loop so live resizing never shows stale pixels.
        self.paint();
    }

    pub fn paint(&mut self) {
        let Some(s) = self.surface.clone() else { return };
        let g = app().gfx();
        if g.bind(&s, self.scale * 96.0).is_err() {
            return;
        }
        if let Some(d) = &mut self.doc {
            d.view.images.poll(g);
        }
        self.ui.begin_frame();
        unsafe {
            g.dc.BeginDraw();
            g.dc.Clear(Some(&app().theme.bg_base));
        }
        let scrolling = self.doc.as_mut().is_some_and(|d| d.view.tick());
        shell::paint(self, g);
        let drawn = unsafe { g.dc.EndDraw(None, None) };
        if let Some(path) = self.shot.take() {
            if let Err(e) = g.capture(&s, &path) {
                crate::startup_mark(&format!("shot failed {e}"));
            }
        }
        self.ui.end_frame();
        if self.pending_reveal {
            self.pending_reveal = false;
            if let Some(d) = &mut self.doc {
                let head = d.view.buf.sel.head;
                d.view.scroll_into_view(head, true);
                win::request_frame(self.hwnd);
            }
            if !self.pending_fragment.is_empty() {
                let f = std::mem::take(&mut self.pending_fragment);
                self.jump_to_fragment(&f);
            }
        }
        let animating = self.ui.animating() || scrolling;
        let presented = g.present(&s, animating);
        if self.doc.as_ref().is_some_and(|d| d.view.pending) {
            unsafe { SetTimer(Some(self.hwnd), TIMER_LAYOUT, 1, None) };
        }
        if self.first_frame {
            self.first_frame = false;
            crate::startup_mark("first-frame");
        }
        if self.ui.update_hover() || animating {
            win::request_frame(self.hwnd);
        }
        self.place_ime();
        // Without a new device the window would stay frozen on its last frame.
        let lost = |r: &windows::core::Result<()>| r.as_ref().is_err_and(|e| [D2DERR_RECREATE_TARGET, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET].contains(&e.code()));
        if lost(&drawn) || lost(&presented) {
            let fonts = crate::resource_dir("fonts");
            if let Ok(g) = crate::gfx::Gfx::new(fonts.as_deref(), app().s().hardware_acceleration).or_else(|_| crate::gfx::Gfx::new(fonts.as_deref(), false)) {
                app().set_gfx(g);
            }
        }
    }

    /// Continue on the device `App::set_gfx` just installed.
    pub fn adopt_device(&mut self) {
        if let Some(d) = &mut self.doc {
            d.view.device_lost();
        }
        let Some(old) = self.surface.take() else { return };
        let (w, h) = ((self.width * self.scale).round() as u32, (self.height * self.scale).round() as u32);
        self.surface = app().gfx().move_surface(&old, w, h).ok();
        self.paint();
        if let Some(s) = &self.surface {
            let _ = app().gfx().show(s);
        }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    // ---- pointer ---------------------------------------------------------------

    pub fn pointer_move(&mut self, x: f32, y: f32, _nonclient: bool) {
        self.ui.mx = x;
        self.ui.my = y;
        self.ui.inside = true;
        if shell::drag(self, x, y) {
            win::request_frame(self.hwnd);
            return;
        }
        if let Some(d) = &mut self.doc {
            if d.view.dragging {
                let pos = d.view.pos_at(x, y);
                d.view.move_to(pos, true);
                d.view.scroll_into_view(pos, false);
                win::request_frame(self.hwnd);
                return;
            }
            // Which code block the pointer is over, for its language chip.
            let over_editor = d.view.rect.contains(x, y);
            let hover = over_editor.then(|| d.view.pos_at(x, y)).and_then(|p| d.view.deco.code_blocks.iter().position(|(r, _, _)| r.start <= p && p <= r.end));
            if hover != d.view.hover_block {
                d.view.hover_block = hover;
                win::request_frame(self.hwnd);
            }
        }
        if self.ui.update_hover() || self.ui.press.is_some() {
            win::request_frame(self.hwnd);
        }
    }

    pub fn pointer_leave(&mut self) {
        self.ui.inside = false;
        if self.ui.update_hover() {
            win::request_frame(self.hwnd);
        }
    }

    pub fn pointer_down(&mut self, x: f32, y: f32, button: u8, double: bool) {
        self.ui.mx = x;
        self.ui.my = y;
        let hit = self.ui.hit_at(x, y);
        // Double-clicking a page margin handle restores its default.
        if double && matches!(hit, Some(Hit::MarginLeft | Hit::MarginRight)) {
            let d = mdwisp_core::settings::AppSettings::default();
            if hit == Some(Hit::MarginLeft) {
                app().patch(|s| s.page_margin_left = d.page_margin_left);
            } else {
                app().patch(|s| s.page_margin_right = d.page_margin_right);
            }
            return;
        }
        if button == 0 {
            self.ui.press = hit;
        }
        if shell::pointer_down(self, hit, x, y, button) {
            win::request_frame(self.hwnd);
            return;
        }
        if hit == Some(Hit::Editor) && button == 0 {
            if let Some(s) = self.doc.as_mut().and_then(|d| d.view.search.as_mut()) {
                s.focus = None;
            }
            if let Some(d) = &mut self.doc {
                let pos = d.view.pos_at(x, y);
                let (at, last_pos, n) = self.last_click;
                let count = if at.elapsed().as_millis() < 500 && last_pos.abs_diff(pos) < 2 { n % 3 + 1 } else { 1 };
                self.last_click = (Instant::now(), pos, count);
                let m = win::mods();
                if m.ctrl {
                    if let Some(href) = d.view.link_at(pos).map(str::to_string) {
                        self.follow_link(&href);
                        return;
                    }
                }
                if shell::editor_click(self, pos, x, y) {
                    win::request_frame(self.hwnd);
                    return;
                }
                let Some(d) = &mut self.doc else { return };
                match count {
                    2 => d.view.select_word_at(pos),
                    3 => {
                        let a = d.view.buf.line_start(pos);
                        let b = d.view.buf.line_end(pos);
                        d.view.buf.sel = crate::editor::buffer::Sel { anchor: a, head: (b + 1).min(d.view.buf.len()) };
                    }
                    _ => {
                        d.view.move_to(pos, m.shift);
                        d.view.dragging = true;
                    }
                }
                self.touch_session();
            }
        }
        win::request_frame(self.hwnd);
    }

    pub fn pointer_up(&mut self, x: f32, y: f32, button: u8) {
        if let Some(d) = &mut self.doc {
            d.view.dragging = false;
        }
        shell::drag_end(self);
        let hit = self.ui.hit_at(x, y);
        let pressed = self.ui.press.take();
        if button == 0 && hit.is_some() && hit == pressed {
            shell::click(self, hit.unwrap());
        }
        if button == 1 {
            shell::context_menu(self, hit, x, y);
        }
        win::request_frame(self.hwnd);
    }

    pub fn toggle_maximize(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if win::is_maximized(self.hwnd) { SW_RESTORE } else { SW_MAXIMIZE });
        }
    }

    pub fn wheel(&mut self, dy: f32, _dx: f32) {
        if shell::wheel(self, dy) {
            win::request_frame(self.hwnd);
            return;
        }
        if let Some(d) = &mut self.doc {
            d.view.wheel(dy);
            win::request_frame(self.hwnd);
        }
    }

    pub fn cursor(&self) -> Cursor {
        let hit = self.ui.hit_at(self.ui.mx, self.ui.my);
        match hit {
            Some(Hit::Editor) => {
                let over_link = win::mods().ctrl && self.doc.as_ref().is_some_and(|d| d.view.link_at(d.view.pos_at(self.ui.mx, self.ui.my)).is_some());
                if over_link { Cursor::Hand } else { Cursor::IBeam }
            }
            Some(Hit::SidebarResize | Hit::HistoryResize | Hit::MarginLeft | Hit::MarginRight) => Cursor::ResizeH,
            Some(Hit::SidebarSplit) => Cursor::ResizeV,
            Some(Hit::PanelControl(_)) if self.modal.as_ref().is_some_and(|m| m.text_input_at(hit.unwrap())) => Cursor::IBeam,
            _ => Cursor::Arrow,
        }
    }

    // ---- keyboard --------------------------------------------------------------

    pub fn key_down(&mut self, vk: VIRTUAL_KEY, m: Mods) -> bool {
        self.sync_view();
        // Overlays first; then registry shortcuts; then plain editing keys.
        if self.modal.is_some() {
            let handled = shell::modal_key(self, vk, m);
            win::request_frame(self.hwnd);
            if handled {
                return true;
            }
        }
        if crate::actions::run_shortcut(self, vk, m) {
            win::request_frame(self.hwnd);
            return true;
        }
        if self.modal.is_some() {
            return false;
        }
        if shell::search_key(self, vk, m) {
            win::request_frame(self.hwnd);
            return true;
        }
        let s = app().s();
        let (smart, tab) = (s.smart_lists, s.tab_size as usize);
        let Some(d) = &mut self.doc else { return false };
        let v = &mut d.view;
        let shift = m.shift;
        let handled = match (vk, m.ctrl, m.alt) {
            (VK_LEFT, ctrl, false) => {
                v.move_h(-1, ctrl, shift);
                true
            }
            (VK_RIGHT, ctrl, false) => {
                v.move_h(1, ctrl, shift);
                true
            }
            (VK_UP, false, false) => {
                v.move_v(-1, shift, false);
                true
            }
            (VK_DOWN, false, false) => {
                v.move_v(1, shift, false);
                true
            }
            (VK_PRIOR, false, false) => {
                v.move_v(-1, shift, true);
                true
            }
            (VK_NEXT, false, false) => {
                v.move_v(1, shift, true);
                true
            }
            (VK_HOME, false, false) => {
                v.move_line_edge(false, shift);
                true
            }
            (VK_END, false, false) => {
                v.move_line_edge(true, shift);
                true
            }
            (VK_HOME, true, false) => {
                v.move_to(0, shift);
                true
            }
            (VK_END, true, false) => {
                v.move_to(v.buf.len(), shift);
                true
            }
            (VK_BACK, ctrl, false) => {
                v.backspace(smart, ctrl);
                true
            }
            (VK_DELETE, ctrl, false) => {
                v.delete_forward(ctrl);
                true
            }
            (VK_RETURN, false, false) => {
                v.enter(smart, shift);
                true
            }
            (VK_TAB, false, false) => {
                v.tab(shift, tab.max(1));
                true
            }
            (k, true, false) if k.0 == b'A' as u16 => {
                v.select_all();
                true
            }
            (k, true, false) if k.0 == b'C' as u16 || k.0 == b'X' as u16 => {
                if !v.buf.sel.empty() {
                    crate::clipboard::set_text(self.hwnd, v.selected_text());
                    if k.0 == b'X' as u16 {
                        v.insert("");
                    }
                }
                true
            }
            (k, true, false) if k.0 == b'V' as u16 => {
                self.paste();
                return true;
            }
            _ => false,
        };
        if handled {
            self.after_input();
        }
        handled
    }

    /// Bring decorations and line geometry up to the current text. Keys can
    /// arrive faster than frames; each one must see the text as it is now.
    pub fn sync_view(&mut self) {
        if let Some(d) = &mut self.doc {
            let t = app().theme;
            d.view.relayout(app().gfx(), &t);
        }
    }

    /// Keep the caret visible and the document lifecycle informed after any edit or move.
    pub fn after_input(&mut self) {
        self.sync_view();
        if let Some(d) = &mut self.doc {
            let head = d.view.buf.sel.head;
            d.view.scroll_into_view(head, app().s().typewriter_mode);
            d.view.blink = Instant::now();
        }
        self.edited();
        self.update_title();
        win::request_frame(self.hwnd);
    }

    pub fn char_input(&mut self, c: u16) {
        self.sync_view();
        let s = match (self.surrogate.take(), c) {
            (_, 0xd800..=0xdbff) => {
                self.surrogate = Some(c);
                return;
            }
            (Some(hi), 0xdc00..=0xdfff) => String::from_utf16_lossy(&[hi, c]),
            (_, c) if c < 0x20 || c == 0x7f => return,
            (_, c) => String::from_utf16_lossy(&[c]),
        };
        if self.modal.is_some() {
            shell::modal_text(self, &s);
            win::request_frame(self.hwnd);
            return;
        }
        if shell::text_focus(self) {
            shell::focused_text(self, &s);
            win::request_frame(self.hwnd);
            return;
        }
        let auto = app().s().auto_pair;
        if let Some(d) = &mut self.doc {
            d.view.type_text(&s, auto);
            self.after_input();
        }
    }

    /// IME: the system draws the composition window at the caret; the commit arrives as WM_CHAR.
    pub fn ime(&mut self, msg: u32, _lp: LPARAM) -> bool {
        if msg == WM_IME_STARTCOMPOSITION {
            self.place_ime();
        }
        false
    }

    fn place_ime(&self) {
        let Some((x, y, h)) = shell::caret_rect(self) else { return };
        let s = self.scale;
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.is_invalid() {
                return;
            }
            let pt = POINT { x: (x * s) as i32, y: (y * s) as i32 };
            let cf = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: pt, rcArea: RECT::default() };
            let _ = ImmSetCompositionWindow(himc, &cf);
            let cand = CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: pt, rcArea: RECT { left: pt.x, top: pt.y, right: pt.x + 1, bottom: pt.y + (h * s) as i32 } };
            let _ = ImmSetCandidateWindow(himc, &cand);
            let _ = ImmReleaseContext(self.hwnd, himc);
        }
    }

    pub fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        if let Some(d) = &mut self.doc {
            d.view.focused = focused;
        }
        win::request_frame(self.hwnd);
    }

    pub fn timer(&mut self, id: usize) {
        match id {
            TIMER_BLINK if self.focused => win::request_frame(self.hwnd),
            TIMER_SCRIPT => crate::script::step(self),
            TIMER_LAYOUT => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LAYOUT);
                }
                if let Some(d) = &mut self.doc {
                    d.view.fill_budget = 300;
                    let t = app().theme;
                    d.view.relayout(app().gfx(), &t);
                }
                win::request_frame(self.hwnd);
            }
            TIMER_TOAST => unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_TOAST);
                self.ui.animate((Hit::Panel, 7), 0.0, crate::theme::Dur::Base, crate::theme::Ease::Out);
                win::request_frame(self.hwnd);
            },
            _ => self.doc_timer(id),
        }
    }

    pub fn images_ready(&mut self) {
        win::request_frame(self.hwnd);
    }

    pub fn drop_files(&mut self, drop: HDROP) {
        let paths = hdrop_paths(drop);
        unsafe { windows::Win32::UI::Shell::DragFinish(drop) };
        shell::dropped(self, paths);
        win::request_frame(self.hwnd);
    }

    pub fn paste(&mut self) {
        shell::paste(self);
        self.after_input();
    }

    pub fn follow_link(&mut self, href: &str) {
        shell::follow_link(self, href);
    }

    /// A rendered image is the caret's target (selected by clicking it).
    pub fn image_focused(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.view.image_selected) && crate::actions::image_at(self).is_some()
    }

    pub fn jump_to_fragment(&mut self, fragment: &str) {
        let Some(d) = &mut self.doc else { return };
        let headings = mdwisp_text::outline::extract_headings(&d.view.buf.text);
        let slug = mdwisp_text::outline::slugify(fragment);
        if let Some(h) = headings.iter().find(|h| h.id == fragment || h.id == slug) {
            d.view.move_to(h.offset, false);
            d.view.scroll_to_line_top(h.offset);
        }
        win::request_frame(self.hwnd);
    }
}

fn hdrop_paths(drop: HDROP) -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::DragQueryFileW;
    unsafe {
        let n = DragQueryFileW(drop, u32::MAX, None);
        (0..n)
            .filter_map(|i| {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(drop, i, Some(&mut buf));
                Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
            })
            .collect()
    }
}

fn theme_key(t: &crate::theme::Theme) -> u64 {
    let c = t.accent;
    (t.dark as u64) | ((c.r * 255.0) as u64) << 8 | ((c.g * 255.0) as u64) << 16 | ((c.b * 255.0) as u64) << 24
}
