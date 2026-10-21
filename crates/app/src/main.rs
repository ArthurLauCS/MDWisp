#![windows_subsystem = "windows"]
//! MDWisp: a Markdown reader and editor, native Win32 + Direct2D.

#[cfg(test)]
mod bench;
mod actions;
mod clipboard;
mod dialog;
mod doc;
mod editor;
mod gfx;
mod keys;
mod script;
mod shell;
mod theme;
mod ui;
mod watch;
mod win;
mod window;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use mdwisp_core::documents::Documents;
use mdwisp_core::history::History;
use mdwisp_core::i18n::Locale;
use mdwisp_core::settings::{AppSettings, Settings, ThemeMode};
use windows::Win32::Foundation::*;
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

use theme::Theme;

/// The device is built on a worker thread while the window is created;
/// first use joins it. Only one thread ever touches it at a time.
struct Built(gfx::Gfx);
unsafe impl Send for Built {}

impl Built {
    fn take(self) -> gfx::Gfx {
        self.0
    }
}

pub struct App {
    gfx: std::cell::OnceCell<gfx::Gfx>,
    gfx_job: Option<std::thread::JoinHandle<Built>>,
    pub theme: Theme,
    pub settings: Settings,
    pub documents: Documents,
    pub history: Arc<History>,
    pub user_data: PathBuf,
    pub windows: Vec<HWND>,
    /// Only the first window restores the last session; later ones start empty.
    restored: bool,
}

static mut APP: *mut App = std::ptr::null_mut();

/// Single UI thread: global state is only touched on it.
pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

static T0: OnceLock<Instant> = OnceLock::new();

/// `MDWISP_TRACE=<file>` appends startup phase timings, for the cold-start benchmark.
pub fn startup_mark(phase: &str) {
    if let Ok(path) = std::env::var("MDWISP_TRACE") {
        use std::io::Write;
        let ms = T0.get().map_or(0.0, |t| t.elapsed().as_micros() as f64 / 1000.0);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{phase} {ms:.1}");
        }
    }
}

impl App {
    pub fn gfx(&mut self) -> &gfx::Gfx {
        if let Some(job) = self.gfx_job.take() {
            let _ = self.gfx.set(job.join().expect("graphics device").0);
            startup_mark("gfx-joined");
        }
        self.gfx.get().unwrap()
    }

    /// Draw with `g` from now on: the GPU once its driver has loaded, or a fresh
    /// device after the GPU was removed or reset (driver update, TDR).
    pub fn set_gfx(&mut self, mut g: gfx::Gfx) {
        if let Some(old) = self.gfx.get() {
            g.dcomp = old.dcomp.clone();
        }
        self.gfx = std::cell::OnceCell::from(g);
        for &h in &self.windows {
            if let Some(w) = win::window(h) {
                w.adopt_device();
            }
        }
    }

    /// Build the GPU or the WARP device off the UI thread (the GPU takes ~170 ms);
    /// the windows keep drawing on the current one until it arrives.
    pub fn switch_device(&self, gpu: bool) {
        let Some(&h) = self.windows.first() else { return };
        let hwnd = h.0 as isize;
        let fonts = resource_dir("fonts");
        std::thread::spawn(move || {
            if let Ok(g) = gfx::Gfx::new(fonts.as_deref(), gpu) {
                let g = Built(g);
                // The switch may have been flipped back while this one was built.
                win::post(hwnd, move |_| {
                    if crate::app().s().hardware_acceleration == gpu {
                        crate::app().set_gfx(g.take());
                    }
                });
            }
        });
    }

    pub fn ui_motion(&self) -> theme::Motion {
        match self.s().motion {
            mdwisp_core::settings::MotionLevel::Full => theme::Motion::Full,
            mdwisp_core::settings::MotionLevel::Reduced => theme::Motion::Reduced,
            mdwisp_core::settings::MotionLevel::Off => theme::Motion::Off,
        }
    }

    pub fn s(&self) -> &AppSettings {
        self.settings.get()
    }

    /// Change settings, persist them, and bring every window up to date.
    pub fn patch(&mut self, change: impl FnOnce(&mut AppSettings)) {
        let gpu = self.s().hardware_acceleration;
        if let Err(e) = self.settings.update(change) {
            startup_mark(&format!("settings write failed: {e}"));
        }
        if self.s().hardware_acceleration != gpu {
            self.switch_device(!gpu);
        }
        self.theme = theme_for(self.settings.get());
        self.gfx().drop_layout_cache();
        for &h in &self.windows {
            if let Some(w) = win::window(h) {
                w.apply_settings();
                win::request_frame(h);
            }
        }
    }

    /// Like `patch`, without writing the file: for drags, persisted on release.
    pub fn patch_live(&mut self, change: impl FnOnce(&mut AppSettings)) {
        self.settings.update_unsaved(change);
        for &h in &self.windows {
            if let Some(w) = win::window(h) {
                w.apply_settings();
                win::request_frame(h);
            }
        }
    }

    pub fn window_closed(&mut self, hwnd: HWND) {
        self.windows.retain(|h| *h != hwnd);
        if self.windows.is_empty() {
            unsafe { PostQuitMessage(0) };
        }
    }

    /// A second launch handed us its command line.
    pub fn forwarded(&mut self, data: &COPYDATASTRUCT) {
        let bytes = unsafe { std::slice::from_raw_parts(data.lpData as *const u16, data.cbData as usize / 2) };
        let text = String::from_utf16_lossy(bytes);
        let mut parts = text.split('\0');
        let cwd = PathBuf::from(parts.next().unwrap_or(""));
        let argv: Vec<String> = parts.filter(|p| !p.is_empty()).map(Into::into).collect();
        open_window(mdwisp_core::launch::document_arg(&argv, &cwd), String::new(), false);
    }
}

fn theme_for(s: &AppSettings) -> Theme {
    let light = match s.theme {
        ThemeMode::Light => true,
        ThemeMode::Dark => false,
        ThemeMode::System => system_light(),
    };
    let base = if light { theme::LIGHT } else { theme::DARK };
    match s.accent_override.as_deref().and_then(theme::parse_hex) {
        Some(accent) => base.with_accent(accent),
        None => base,
    }
}

fn system_light() -> bool {
    use windows::Win32::System::Registry::*;
    let mut v = 0u32;
    let mut len = 4u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), windows::core::w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut _ as _), Some(&mut len))
            .is_ok()
            && v == 1
    }
}

/// Bundled resources: next to the exe when installed, in the repo during development.
pub fn resource_dir(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let installed = exe.parent()?.join(name);
    if installed.exists() {
        return Some(installed);
    }
    let repo = exe.ancestors().find(|p| p.join("Cargo.lock").exists())?;
    let dev = match name {
        "fonts" => repo.join("crates/export/assets/fonts"),
        other => repo.join(other),
    };
    dev.exists().then_some(dev)
}

/// Open a window; `path` None restores the session (first window) or starts a draft.
pub fn open_window(path: Option<PathBuf>, fragment: String, first: bool) {
    let a = app();
    let restore = first && !a.restored;
    a.restored = true;
    let mut w = window::Window::new(restore);
    let target = path.or_else(|| if restore { w.session.state.active_doc.clone().filter(|p| p.exists()) } else { None });
    if let Err(e) = w.load_initial(target.as_deref(), fragment) {
        w.error = Some(e);
        let _ = w.load_initial(None, String::new());
    }
    let hwnd = win::create(w, CW_USEDEFAULT, CW_USEDEFAULT, scaled(1280), scaled(840), false);
    a.windows.push(hwnd);
}

fn scaled(px: i32) -> i32 {
    unsafe { px * GetDpiForSystem() as i32 / 96 }
}

/// Another instance is running: hand it our command line and leave.
fn forward_to_primary() -> bool {
    unsafe {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let name: Vec<u16> = format!("MDWisp-{user}").encode_utf16().chain([0]).collect();
        // GetLastError is only meaningful right after the call: clear what earlier calls left.
        SetLastError(WIN32_ERROR(0));
        let _ = CreateMutexW(None, true, PCWSTR(name.as_ptr()));
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return false;
        }
        let Ok(target) = FindWindowW(win::CLASS, None) else { return false };
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut text = cwd.to_string_lossy().into_owned();
        for a in std::env::args() {
            text.push('\0');
            text.push_str(&a);
        }
        let w: Vec<u16> = text.encode_utf16().collect();
        let cds = COPYDATASTRUCT { dwData: 1, cbData: (w.len() * 2) as u32, lpData: w.as_ptr() as _ };
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        SendMessageW(target, WM_COPYDATA, Some(WPARAM(0)), Some(LPARAM(&cds as *const _ as isize)));
        true
    }
}

fn main() {
    T0.get_or_init(Instant::now);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    // Scripted runs are test instances; they never hand off to the user's own window.
    if std::env::var("MDWISP_SCRIPT").is_err() && forward_to_primary() {
        return;
    }
    startup_mark("start");
    // Language detection compiles ~150 patterns on first use; do that off the UI thread now.
    std::thread::spawn(|| mdwisp_text::detect::detect_language(""));
    let user_data = std::env::var("MDWISP_USER_DATA").map(PathBuf::from).unwrap_or_else(|_| mdwisp_core::settings::user_data_dir());
    let zh = unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x04;
    let settings = Settings::new(&user_data, if zh { Locale::ZhCn } else { Locale::En }).expect("settings");
    let theme = theme_for(settings.get());
    let bg = theme.bg_base;
    win::register((bg.r * 255.0) as u32 | ((bg.g * 255.0) as u32) << 8 | ((bg.b * 255.0) as u32) << 16);
    let fonts = resource_dir("fonts");
    let job = std::thread::spawn(move || {
        let g = gfx::Gfx::new(fonts.as_deref(), false).expect("graphics device");
        startup_mark("gfx");
        Built(g)
    });
    let app = Box::new(App { gfx: std::cell::OnceCell::new(), gfx_job: Some(job), theme, settings, documents: Documents::default(), history: Arc::new(History::new(&user_data)), user_data, windows: vec![], restored: false });
    unsafe { APP = Box::into_raw(app) };
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    open_window(mdwisp_core::launch::document_arg(&args, &cwd), String::new(), true);
    startup_mark("window");
    // WARP drew the first frames without waiting for the GPU driver to load.
    if crate::app().s().hardware_acceleration {
        crate::app().switch_device(true);
    }
    unsafe {
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).into() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}
