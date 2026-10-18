//! Chinese source strings are the keys; English is a lookup table
//! (`locale_en.rs`). A missing key falls back to the Chinese text.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_language(locale: Locale) {
    ENGLISH.store(locale == Locale::En, Ordering::Relaxed);
}

pub fn current_language() -> Locale {
    if ENGLISH.load(Ordering::Relaxed) { Locale::En } else { Locale::ZhCn }
}

fn english() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    MAP.get_or_init(|| crate::locale_en::EN.iter().copied().collect())
}

pub fn t(key: &str) -> &str {
    if ENGLISH.load(Ordering::Relaxed) {
        if let Some(v) = english().get(key) {
            return v;
        }
    }
    key
}

/// `t` with `{0}`, `{1}` … placeholders filled in.
pub fn tf(key: &str, values: &[&dyn std::fmt::Display]) -> String {
    let mut out = t(key).to_string();
    for (i, v) in values.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), &v.to_string());
    }
    out
}
