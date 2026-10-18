//! The document a window edits: opening, saving, autosave, history snapshots
//! and changes made on disk by other programs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mdwisp_core::documents::{self, DocumentMeta};
use mdwisp_core::history::RevisionKind;
use mdwisp_core::i18n::{t, tf};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dialog::{self, Choice};
use crate::editor::view::View;
use crate::window::Window;
use crate::{app, watch, win};

pub const TIMER_AUTOSAVE: usize = 10;
pub const TIMER_HISTORY: usize = 11;
pub const TIMER_SESSION: usize = 12;

pub struct Doc {
    /// None for an untitled draft.
    pub meta: Option<DocumentMeta>,
    pub view: View,
    /// Editor text as last read from or written to disk (newlines normalised).
    pub saved: String,
    /// The raw disk text, for the "nobody else changed it" check on save.
    disk: String,
    watcher: Option<watch::Watcher>,
    /// Buffer version the autosave timer was armed for.
    armed: u64,
    words: std::cell::Cell<(u64, usize)>,
}

pub fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n")
}

impl Doc {
    pub fn new(meta: Option<DocumentMeta>, text: String) -> Doc {
        let view = View::new(normalise(&text), meta.as_ref().map(|m| m.parent_dir.clone()));
        Doc { saved: view.buf.text.clone(), disk: text, meta, view, watcher: None, armed: 0, words: std::cell::Cell::new((u64::MAX, 0)) }
    }

    pub fn dirty(&self) -> bool {
        self.view.buf.text != self.saved
    }

    /// Word count, recounted only when the text changed.
    pub fn words(&self) -> usize {
        let (v, n) = self.words.get();
        if v == self.view.buf.version {
            return n;
        }
        let n = mdwisp_text::ops::count_words(&self.view.buf.text);
        self.words.set((self.view.buf.version, n));
        n
    }

    pub fn path(&self) -> Option<&Path> {
        self.meta.as_ref().map(|m| m.path.as_path())
    }

    /// Name shown in the caption and in the save question.
    pub fn name(&self) -> String {
        self.meta.as_ref().map_or_else(|| t("未命名").to_string(), |m| m.stem.clone())
    }
}

impl Window {
    /// Put a document into this window (startup, session restore).
    pub fn load(&mut self, path: Option<&Path>) -> Result<(), String> {
        let doc = match path {
            Some(p) => {
                let c = documents::read(p)?;
                Doc::new(Some(c.meta), c.text)
            }
            None => Doc::new(None, String::new()),
        };
        self.set_doc(doc);
        Ok(())
    }

    pub fn set_doc(&mut self, mut doc: Doc) {
        doc.view.images.hwnd = self.hwnd;
        doc.view.images.scale = self.scale;
        if let Some(p) = doc.path().map(Path::to_path_buf) {
            doc.watcher = watch::watch(&p, self.hwnd);
            if let Some(&c) = self.session.state.cursors.get(&p) {
                let c = c.min(doc.view.buf.len());
                if doc.view.buf.text.is_char_boundary(c) {
                    doc.view.buf.sel = crate::editor::buffer::Sel::at(c);
                    self.pending_reveal = true;
                }
            }
        }
        self.doc = Some(doc);
        self.apply_settings();
        self.restart_history();
        self.touch_session();
        win::request_frame(self.hwnd);
    }

    /// Called after any change to the buffer: autosave, title, session.
    pub fn edited(&mut self) {
        let Some(d) = &mut self.doc else { return };
        let s = app().settings.get();
        if d.view.buf.version != d.armed && d.meta.is_some() && s.auto_save && d.dirty() {
            d.armed = d.view.buf.version;
            unsafe { SetTimer(Some(self.hwnd), TIMER_AUTOSAVE, s.auto_save_delay_ms.max(100.0) as u32, None) };
        }
        self.touch_session();
    }

    /// Save; untitled drafts ask where first. Returns whether the text is on disk.
    pub fn save(&mut self, manual: bool) -> bool {
        let hwnd = self.hwnd;
        let Some(d) = &mut self.doc else { return false };
        let path = match d.path() {
            Some(p) => p.to_path_buf(),
            None => match dialog::save_file(hwnd, &format!("{}.md", t("未命名")), &dialog::markdown_filter()) {
                Some(p) => p,
                None => return false,
            },
        };
        let text = d.view.buf.text.clone();
        let expected = d.meta.is_some().then(|| d.disk.clone());
        match app().documents.write(&path, &text, expected.as_deref()) {
            Ok(()) => {
                let first = d.meta.is_none();
                d.saved = text.clone();
                d.disk = text.clone();
                if first {
                    let meta = documents::meta_for(&path);
                    d.view.doc_dir = Some(meta.parent_dir.clone());
                    d.watcher = watch::watch(&meta.path, hwnd);
                    d.meta = Some(meta);
                    self.restart_history();
                }
                self.error = None;
                if manual && app().settings.get().history_enabled {
                    self.snapshot(RevisionKind::Manual);
                }
                self.touch_session();
                win::request_frame(hwnd);
                true
            }
            Err(e) => {
                self.error = Some(tf("保存失败：{0}", &[&e]));
                win::request_frame(hwnd);
                false
            }
        }
    }

    /// Record a history revision off the UI thread.
    pub fn snapshot(&mut self, kind: RevisionKind) {
        let Some(d) = &self.doc else { return };
        let Some(meta) = &d.meta else { return };
        let (id, text) = (meta.id.clone(), d.view.buf.text.clone());
        let gap = if kind == RevisionKind::Auto { app().settings.get().history_interval_ms as u64 } else { 0 };
        let history: Arc<mdwisp_core::history::History> = app().history.clone();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let r = history.record(&id, &text, kind, gap);
            win::post(hwnd, move |w| match r {
                Ok(Some(_)) => w.history_changed(),
                Ok(None) => {}
                Err(e) => w.toast(&tf("历史记录失败：{0}", &[&e])),
            });
        });
    }

    /// Snapshots run on their own interval, independent of typing pauses.
    pub fn restart_history(&mut self) {
        let s = app().settings.get();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_HISTORY);
        }
        if self.doc.as_ref().is_some_and(|d| d.meta.is_some()) && s.history_enabled {
            self.snapshot(RevisionKind::Auto);
            unsafe { SetTimer(Some(self.hwnd), TIMER_HISTORY, s.history_interval_ms.max(1000.0) as u32, None) };
        }
    }

    pub fn doc_timer(&mut self, id: usize) {
        match id {
            TIMER_AUTOSAVE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_AUTOSAVE);
                }
                if self.doc.as_ref().is_some_and(|d| d.dirty() && d.meta.is_some()) && self.modal.is_none() {
                    self.save(false);
                }
            }
            TIMER_HISTORY => self.snapshot(RevisionKind::Auto),
            TIMER_SESSION => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_SESSION);
                }
                self.flush_session();
            }
            _ => {}
        }
    }

    /// Another program wrote the file.
    pub fn disk_changed(&mut self) {
        let Some(d) = &mut self.doc else { return };
        let Some(path) = d.path().map(Path::to_path_buf) else { return };
        let Ok(raw) = std::fs::read_to_string(&path) else { return };
        if raw == d.disk {
            return;
        }
        if d.dirty() {
            self.error = Some(t("文件已被其他程序修改。按 Ctrl+Shift+R 载入磁盘版本，载入后可按 Ctrl+Z 找回本窗口的修改。").into());
        } else {
            self.adopt_disk(raw);
            self.toast(t("已载入其他程序对文件的修改"));
        }
        win::request_frame(self.hwnd);
    }

    /// Show the disk text as one undoable step, so Ctrl+Z brings this window's text back.
    fn adopt_disk(&mut self, raw: String) {
        let Some(d) = &mut self.doc else { return };
        let text = normalise(&raw);
        d.disk = raw;
        d.saved = text.clone();
        d.view.buf.set_text(&text);
        d.view.images.clear();
        self.error = None;
    }

    pub fn reload_from_disk(&mut self) {
        let Some(d) = &self.doc else { return };
        let Some(path) = d.path() else { return };
        let Ok(raw) = std::fs::read_to_string(path) else { return };
        if normalise(&raw) == d.view.buf.text {
            self.toast(t("已是磁盘上的最新内容"));
            return;
        }
        self.adopt_disk(raw);
        self.toast(t("已载入磁盘版本，按 Ctrl+Z 可找回之前的内容"));
    }

    /// Ask before losing unsaved text. False when the user cancelled.
    pub fn confirm_discard(&mut self) -> bool {
        let Some(d) = &self.doc else { return true };
        if !d.dirty() {
            return true;
        }
        let name = d.name();
        match dialog::confirm_save(self.hwnd, &name) {
            Choice::Cancel => false,
            Choice::Discard => true,
            Choice::Save => self.save(true) && !self.doc.as_ref().is_some_and(Doc::dirty),
        }
    }

    pub fn request_close(&mut self) -> bool {
        if !self.force_close && !self.confirm_discard() {
            return false;
        }
        self.flush_session();
        true
    }

    // ---- session -------------------------------------------------------------

    /// Debounced: caret moves fire constantly, and a session file is not worth a write per keystroke.
    pub fn touch_session(&mut self) {
        unsafe { SetTimer(Some(self.hwnd), TIMER_SESSION, 1200, None) };
    }

    pub fn flush_session(&mut self) {
        let mut state = self.session.state.clone();
        state.workspace_root = self.workspace.current().map(|w| w.root_path.clone());
        if let Some(d) = &self.doc {
            if let Some(p) = d.path() {
                state.active_doc = Some(p.to_path_buf());
                state.open_docs = vec![p.to_path_buf()];
                state.cursors.insert(p.to_path_buf(), d.view.buf.sel.head);
                state.scrolls.insert(p.to_path_buf(), d.view.scroll as f64);
            } else {
                state.active_doc = None;
                state.open_docs = vec![];
            }
        }
        self.session.state = state.clone();
        let _ = self.session.store.save(state);
    }

    // ---- opening other documents -------------------------------------------

    /// New, open and cross-document jumps each get their own window.
    pub fn open_in_new_window(&mut self, path: Option<PathBuf>, fragment: String) {
        if let Some(p) = &path {
            if let Err(e) = documents::read(p) {
                self.error = Some(e);
                win::request_frame(self.hwnd);
                return;
            }
        }
        crate::open_window(path, fragment, false);
    }

    pub fn open_dialog(&mut self) {
        let files = dialog::open_file(self.hwnd, t("打开文档"), &dialog::markdown_filter(), false);
        if let Some(p) = files.into_iter().next() {
            self.open_in_new_window(Some(p), String::new());
        }
    }
}
