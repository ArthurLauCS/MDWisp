//! Everything MDWisp does that is not drawing: files, history, export,
//! markdown analysis and the table engine. No window or GPU types here, so
//! all of it is covered by `cargo test -p mdwisp-core`.

pub mod assets;
pub mod documents;
pub mod history;
pub mod i18n;
pub mod launch;
mod locale_en;
pub mod organize;
pub mod paths;
pub mod plugin;
pub mod session;
pub mod settings;
pub mod workspace;

pub(crate) fn io_err(e: std::io::Error) -> String {
    e.to_string()
}

/// Milliseconds since the Unix epoch, the unit every persisted timestamp uses.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The UI language is process-wide; tests that switch it or match a Chinese
/// message hold this so they don't see each other's language.
#[cfg(test)]
pub(crate) static LANG: std::sync::Mutex<()> = std::sync::Mutex::new(());
