//! Key-chord parsing. A binding string is normalised into a canonical combo
//! (`ctrl+shift+key`) so a keystroke is matched with one map lookup.

use std::sync::LazyLock;

use mdwisp_core::i18n::t;
use regex::Regex;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub key: String,
}

impl Chord {
    /// `Ctrl+Shift+Digit1` style, as written in the action registry.
    pub fn parse(binding: &str) -> Chord {
        let (mods, key) = binding.rsplit_once('+').unwrap_or(("", binding));
        let has = |m: &str| mods.split('+').any(|p| p.eq_ignore_ascii_case(m));
        Chord { ctrl: has("ctrl"), shift: has("shift"), alt: has("alt"), meta: has("meta"), key: normalise_key(key) }
    }

    /// Canonical lookup form: modifiers in a fixed order, then the key.
    pub fn combo(&self) -> String {
        let mods = [(self.ctrl, "ctrl+"), (self.alt, "alt+"), (self.shift, "shift+"), (self.meta, "meta+")];
        mods.iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect::<String>() + &self.key
    }
}

/// `Digit1` and `1` are the same physical key; `ArrowUp` and `up` likewise.
/// Accepts DOM `code` names or the typed character.
pub fn normalise_key(key: &str) -> String {
    let alias = match key {
        " " => "space",
        "\\" | "|" => "backslash",
        "," | "<" => "comma",
        ")" => "0",
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        _ => "",
    };
    if !alias.is_empty() {
        return alias.to_string();
    }
    let bare = ["Digit", "Key", "Arrow"].iter().find_map(|p| key.strip_prefix(p)).unwrap_or(key);
    bare.to_lowercase()
}

/// Human-readable form for menus: `Ctrl+Shift+Digit1` → `Ctrl + Shift + 1`.
pub fn pretty_key(binding: &str) -> String {
    static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Digit(\d)").unwrap());
    let mut s = DIGIT.replace_all(binding, "$1").into_owned();
    for (from, to) in [
        ("ArrowUp", "↑"),
        ("ArrowDown", "↓"),
        ("ArrowLeft", "←"),
        ("ArrowRight", "→"),
        ("Backspace", "⌫"),
        ("Enter", "↵"),
        ("Space", t("空格")),
        ("Escape", "Esc"),
        ("Backslash", "\\"),
        ("Slash", "/"),
        ("Equal", "="),
        ("Minus", "-"),
        ("Meta", "Cmd"),
        ("+", " + "),
    ] {
        s = s.replace(from, to);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(binding: &str, key: &str) -> String {
        Chord { key: normalise_key(key), ..Chord::parse(binding) }.combo()
    }

    #[test]
    fn parses_bindings() {
        assert_eq!(
            Chord::parse("Ctrl+Shift+Digit1"),
            Chord { ctrl: true, shift: true, key: "1".into(), ..Chord::default() }
        );
        assert_eq!(Chord::parse("Alt+ArrowUp").combo(), "alt+up");
        assert_eq!(Chord::parse("Ctrl+Shift+Alt+Meta+KeyE").combo(), "ctrl+alt+shift+meta+e");
        assert_eq!(Chord::parse("F2").combo(), "f2");
    }

    #[test]
    fn matches_punctuation_spaces_and_shifted_digits() {
        let combo = |b: &str| Chord::parse(b).combo();
        assert_eq!(typed("Ctrl+Shift+Digit1", "!"), combo("Ctrl+Shift+Digit1"));
        assert_eq!(typed("Ctrl+Shift+Digit8", "*"), combo("Ctrl+Shift+Digit8"));
        assert_eq!(typed("Ctrl+Backslash", "\\"), combo("Ctrl+Backslash"));
        assert_eq!(typed("Ctrl+Shift+Space", " "), combo("Ctrl+Shift+Space"));
        assert_eq!(typed("Ctrl+,", ","), combo("Ctrl+,"));
        assert_eq!(combo("Ctrl+,"), "ctrl+comma");
        assert_eq!(typed("Ctrl+Shift+E", "KeyE"), combo("Ctrl+Shift+E"));
    }

    #[test]
    fn pretty() {
        assert_eq!(pretty_key("Ctrl+Shift+Digit1"), "Ctrl + Shift + 1");
        assert_eq!(pretty_key("Ctrl+Backslash"), "Ctrl + \\");
        assert_eq!(pretty_key("Meta+ArrowUp"), "Cmd + ↑");
        assert_eq!(pretty_key("Ctrl+Shift+Space"), "Ctrl + Shift + 空格");
    }
}
