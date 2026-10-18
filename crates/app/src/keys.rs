//! Key chords as written in the action registry (`Ctrl+Shift+K`, `Alt+ArrowUp`).

use windows::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The registry's key name for a virtual key, matching `KeyboardEvent.key`
/// for letters (upper-cased) and `KeyboardEvent.code`-style names otherwise.
pub fn key_name(vk: VIRTUAL_KEY) -> Option<&'static str> {
    const LETTERS: [&str; 26] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const F: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
    let v = vk.0;
    Some(match vk {
        _ if (0x41..=0x5a).contains(&v) => LETTERS[(v - 0x41) as usize],
        _ if (0x30..=0x39).contains(&v) => DIGITS[(v - 0x30) as usize],
        _ if (VK_F1.0..=VK_F12.0).contains(&v) => F[(v - VK_F1.0) as usize],
        VK_RETURN => "Enter",
        VK_ESCAPE => "Escape",
        VK_TAB => "Tab",
        VK_BACK => "Backspace",
        VK_DELETE => "Delete",
        VK_INSERT => "Insert",
        VK_HOME => "Home",
        VK_END => "End",
        VK_PRIOR => "PageUp",
        VK_NEXT => "PageDown",
        VK_LEFT => "ArrowLeft",
        VK_RIGHT => "ArrowRight",
        VK_UP => "ArrowUp",
        VK_DOWN => "ArrowDown",
        VK_SPACE => "Space",
        VK_OEM_COMMA => "Comma",
        VK_OEM_PERIOD => "Period",
        VK_OEM_2 => "Slash",
        VK_OEM_1 => "Semicolon",
        VK_OEM_7 => "Quote",
        VK_OEM_4 => "BracketLeft",
        VK_OEM_6 => "BracketRight",
        VK_OEM_5 => "Backslash",
        VK_OEM_3 => "Backquote",
        VK_OEM_MINUS => "Minus",
        VK_OEM_PLUS => "Equal",
        _ => return None,
    })
}

/// `Ctrl+Shift+K` → (mods, "K"). Case-insensitive for the modifier names.
pub fn parse(chord: &str) -> (Mods, String) {
    let mut m = Mods::default();
    let mut key = String::new();
    for part in chord.split('+').filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "mod" | "cmd" => m.ctrl = true,
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            _ => key = part.to_string(),
        }
    }
    // `Ctrl++` splits into nothing after the plus.
    if chord.ends_with("++") {
        key = "+".into();
    }
    (m, key)
}

pub fn matches(chord: &str, mods: Mods, name: &str) -> bool {
    let (m, k) = parse(chord);
    m == mods && normalise(&k) == normalise(name)
}

/// `Digit1` and `1` are the same physical key; `ArrowUp` and `Up` likewise.
pub fn normalise(key: &str) -> String {
    let alias = match key {
        " " => "space",
        "\\" | "|" => "backslash",
        "," | "<" => "comma",
        "/" => "slash",
        "." => "period",
        _ => "",
    };
    if !alias.is_empty() {
        return alias.into();
    }
    let k = key.strip_prefix("Digit").or_else(|| key.strip_prefix("Key")).or_else(|| key.strip_prefix("Arrow")).unwrap_or(key);
    k.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chords() {
        assert_eq!(parse("Ctrl+Shift+K"), (Mods { ctrl: true, shift: true, alt: false }, "K".into()));
        assert_eq!(parse("Alt+ArrowUp").1, "ArrowUp");
        assert!(matches("Ctrl+s", Mods { ctrl: true, ..Default::default() }, "S"));
        assert!(!matches("Ctrl+S", Mods { ctrl: true, shift: true, alt: false }, "S"));
        let ctrl = Mods { ctrl: true, ..Default::default() };
        assert!(matches("Ctrl+Backslash", ctrl, key_name(VK_OEM_5).unwrap()));
        assert!(matches("Ctrl+,", ctrl, key_name(VK_OEM_COMMA).unwrap()));
        assert!(matches("Ctrl+Shift+Digit1", Mods { ctrl: true, shift: true, alt: false }, key_name(VIRTUAL_KEY(0x31)).unwrap()));
        assert!(matches("Alt+ArrowUp", Mods { alt: true, ..Default::default() }, key_name(VK_UP).unwrap()));
    }
}
