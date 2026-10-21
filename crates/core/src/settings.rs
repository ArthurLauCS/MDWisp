use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::{Locale, set_language};
use crate::io_err;

/// `%APPDATA%\mdview`: the folder keeps the pre-rename name so existing
/// settings, sessions and history carry over.
pub fn user_data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("mdview")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MotionLevel {
    Full,
    Reduced,
    Off,
}

/// Which placeholder `plain-md` export leaves where an image was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlainMdImagePolicy {
    Drop,
    AltPlaceholder,
    EmptyRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Bar,
    Block,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageNaming {
    DateHashName,
    Original,
    Hash,
}

/// Persisted as `settings.json`. Numbers are f64 because the file was written
/// by JavaScript, where any of them may hold a fraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: Locale,
    pub theme: ThemeMode,
    pub motion: MotionLevel,
    /// Draw on the GPU; off draws on the CPU (WARP), for broken graphics drivers.
    pub hardware_acceleration: bool,
    pub plain_md_image_policy: PlainMdImagePolicy,
    /// Base reading font size in px.
    pub font_size: f64,
    /// Left/right writing margins as a percentage of the editor width.
    pub page_margin_left: f64,
    pub page_margin_right: f64,
    pub font_ui: Option<String>,
    pub font_read: Option<String>,
    pub font_code: Option<String>,
    pub font_display: Option<String>,
    pub sidebar_visible: bool,
    pub sidebar_width: f64,
    pub history_width: f64,
    /// Images' share of the sidebar height; 0 hides the images, 1 hides the navigation.
    pub assets_ratio: f64,
    pub outline_visible: bool,
    pub typewriter_mode: bool,
    pub highlight_current_line: bool,
    pub read_only: bool,
    /// Plain Markdown source instead of the formatted live view.
    pub source_mode: bool,
    pub code_font_size: f64,
    pub line_height: f64,
    pub cursor_style: CursorStyle,
    /// None keeps the built-in accent.
    pub accent_override: Option<String>,
    pub auto_save: bool,
    pub auto_save_delay_ms: f64,
    pub history_enabled: bool,
    /// Milliseconds between automatic snapshots.
    pub history_interval_ms: f64,
    pub spell_check: bool,
    pub auto_pair: bool,
    pub smart_lists: bool,
    pub tab_size: f64,
    pub image_naming: ImageNaming,
    pub image_dedupe: bool,
    /// Cap in px; 0 writes images at their original size.
    pub image_max_width: f64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: Locale::ZhCn,
            theme: ThemeMode::Dark,
            motion: MotionLevel::Full,
            hardware_acceleration: true,
            plain_md_image_policy: PlainMdImagePolicy::AltPlaceholder,
            font_size: 17.0,
            page_margin_left: 5.0,
            page_margin_right: 5.0,
            font_ui: None,
            font_read: None,
            font_code: None,
            font_display: None,
            sidebar_visible: true,
            sidebar_width: 260.0,
            history_width: 360.0,
            assets_ratio: 0.4,
            outline_visible: true,
            typewriter_mode: false,
            highlight_current_line: true,
            read_only: false,
            source_mode: false,
            code_font_size: 13.5,
            line_height: 1.9,
            cursor_style: CursorStyle::Bar,
            accent_override: None,
            auto_save: true,
            auto_save_delay_ms: 700.0,
            history_enabled: true,
            history_interval_ms: 20000.0,
            spell_check: false,
            auto_pair: true,
            smart_lists: true,
            tab_size: 2.0,
            image_naming: ImageNaming::DateHashName,
            image_dedupe: true,
            image_max_width: 0.0,
        }
    }
}

/// Plain JSON settings file in the user data folder.
pub struct Settings {
    file: PathBuf,
    cache: AppSettings,
}

impl Settings {
    /// `default_language` applies until the user picks one (first launch follows the system).
    pub fn new(user_data: &Path, default_language: Locale) -> Result<Self, String> {
        fs::create_dir_all(user_data).map_err(io_err)?;
        let file = user_data.join("settings.json");
        let cache = load(&file, default_language);
        set_language(cache.language);
        Ok(Self { file, cache })
    }

    pub fn get(&self) -> &AppSettings {
        &self.cache
    }

    pub fn update(&mut self, change: impl FnOnce(&mut AppSettings)) -> Result<&AppSettings, String> {
        self.update_unsaved(change);
        self.persist()?;
        Ok(&self.cache)
    }

    /// Change in memory only; for values that move continuously (a drag), written once by `persist`.
    pub fn update_unsaved(&mut self, change: impl FnOnce(&mut AppSettings)) {
        change(&mut self.cache);
        set_language(self.cache.language);
    }

    pub fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.cache).map_err(|e| e.to_string())?;
        fs::write(&self.file, json).map_err(io_err)
    }
}

/// Saved keys over the defaults, so a file from an older version gains new settings.
fn load(file: &Path, default_language: Locale) -> AppSettings {
    let defaults = AppSettings { language: default_language, ..Default::default() };
    let saved = fs::read_to_string(file).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let Some(serde_json::Value::Object(saved)) = saved else { return defaults };
    let Ok(serde_json::Value::Object(mut merged)) = serde_json::to_value(&defaults) else { return defaults };
    merged.extend(saved);
    serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or(defaults)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::current_language;

    #[test]
    fn keeps_saved_keys_fills_missing_ones_and_writes_pretty_json() {
        let _lang = crate::LANG.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let fresh = Settings::new(dir.path(), Locale::En).unwrap();
        assert_eq!(fresh.get(), &AppSettings { language: Locale::En, ..Default::default() });
        assert_eq!(current_language(), Locale::En);

        fs::write(dir.path().join("settings.json"), r#"{"theme":"light","sidebarWidth":271.5,"imageNaming":"hash","unknown":1}"#).unwrap();
        let mut s = Settings::new(dir.path(), Locale::En).unwrap();
        assert_eq!((s.get().theme, s.get().sidebar_width, s.get().image_naming), (ThemeMode::Light, 271.5, ImageNaming::Hash));
        assert_eq!(s.get().language, Locale::En);
        s.update(|c| c.language = Locale::ZhCn).unwrap();
        assert_eq!(current_language(), Locale::ZhCn);
        let written = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(written.starts_with("{\n  \"language\": \"zh-CN\",\n  \"theme\": \"light\","));
        assert_eq!(Settings::new(dir.path(), Locale::En).unwrap().get().language, Locale::ZhCn);
        set_language(Locale::ZhCn);
    }
}
