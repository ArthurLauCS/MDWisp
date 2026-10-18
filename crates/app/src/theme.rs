//! Design tokens — the single source of every colour, space and motion value,
//! ported from `tokens.css`. Dark is the primary theme; light lifts lightness
//! in the same hue. No colour literal belongs anywhere else in the app.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub type Color = D2D1_COLOR_F;

const fn hex(v: u32) -> Color {
    Color { r: ((v >> 16) & 255) as f32 / 255.0, g: ((v >> 8) & 255) as f32 / 255.0, b: (v & 255) as f32 / 255.0, a: 1.0 }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

/// `color-mix(in srgb, a p%, b)`.
pub fn mix(a: Color, b: Color, p: f32) -> Color {
    Color { r: a.r * p + b.r * (1.0 - p), g: a.g * p + b.g * (1.0 - p), b: a.b * p + b.b * (1.0 - p), a: a.a * p + b.a * (1.0 - p) }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg_base: Color,
    pub bg_raised: Color,
    pub bg_overlay: Color,
    pub bg_inset: Color,
    pub bg_hover: Color,
    pub bg_active: Color,
    pub border_soft: Color,
    pub border_strong: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub text_disabled: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_press: Color,
    pub accent_soft: Color,
    pub accent_line: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub link: Color,
    pub hl_keyword: Color,
    pub hl_string: Color,
    pub hl_number: Color,
    pub hl_comment: Color,
    pub hl_func: Color,
    pub hl_type: Color,
    pub hl_punct: Color,
    pub hl_attr: Color,
    pub shadow: f32,
}

pub const DARK: Theme = Theme {
    dark: true,
    bg_base: hex(0x1f1e1d),
    bg_raised: hex(0x262624),
    bg_overlay: hex(0x30302e),
    bg_inset: hex(0x191918),
    bg_hover: rgba(245, 244, 239, 0.045),
    bg_active: rgba(245, 244, 239, 0.075),
    border_soft: hex(0x33322f),
    border_strong: hex(0x45443f),
    text_primary: hex(0xf5f4ef),
    text_secondary: hex(0xb8b5ad),
    text_tertiary: hex(0x8a877e),
    text_disabled: hex(0x5c5a54),
    accent: hex(0xd97757),
    accent_hover: hex(0xe08263),
    accent_press: hex(0xc96a4c),
    accent_soft: rgba(217, 119, 87, 0.14),
    accent_line: rgba(217, 119, 87, 0.35),
    success: hex(0x7ba88a),
    warning: hex(0xd9a95c),
    danger: hex(0xc9645c),
    link: hex(0x8ab4c4),
    hl_keyword: hex(0xc98a6b),
    hl_string: hex(0x9cb380),
    hl_number: hex(0xd9a95c),
    hl_comment: hex(0x6e6b64),
    hl_func: hex(0x7fb3c8),
    hl_type: hex(0xb79bd1),
    hl_punct: hex(0x8a877e),
    hl_attr: hex(0xd9a95c),
    shadow: 1.0,
};

pub const LIGHT: Theme = Theme {
    dark: false,
    bg_base: hex(0xfaf9f5),
    bg_raised: hex(0xffffff),
    bg_overlay: hex(0xffffff),
    bg_inset: hex(0xf2f0ea),
    bg_hover: rgba(31, 30, 29, 0.04),
    bg_active: rgba(31, 30, 29, 0.07),
    border_soft: hex(0xe6e3da),
    border_strong: hex(0xd3cfc4),
    text_primary: hex(0x1f1e1d),
    text_secondary: hex(0x5c5a54),
    text_tertiary: hex(0x8a877e),
    text_disabled: hex(0xb8b5ad),
    accent: hex(0xc2603f),
    accent_hover: hex(0xd97757),
    accent_press: hex(0xa94f33),
    accent_soft: rgba(194, 96, 63, 0.12),
    accent_line: rgba(194, 96, 63, 0.32),
    success: hex(0x7ba88a),
    warning: hex(0xd9a95c),
    danger: hex(0xc9645c),
    link: hex(0x3f6f85),
    hl_keyword: hex(0xa8502c),
    hl_string: hex(0x4f7a3a),
    hl_number: hex(0x95681c),
    hl_comment: hex(0x9a968c),
    hl_func: hex(0x2f6d8c),
    hl_type: hex(0x7a53a8),
    hl_punct: hex(0x6e6b64),
    hl_attr: hex(0x95681c),
    shadow: 0.33,
};

/// The Windows close button: the one place a system colour is correct.
pub const CLOSE_HOVER: Color = hex(0xc42b1c);
pub const CLOSE_PRESS: Color = hex(0xb1271a);
pub const WHITE: Color = hex(0xffffff);

impl Theme {
    /// The accent is one value to the user and five tokens to the drawing code.
    pub fn with_accent(mut self, accent: Color) -> Theme {
        let white = hex(0xffffff);
        let black = hex(0x000000);
        let clear = Color { a: 0.0, ..accent };
        self.accent = accent;
        self.accent_hover = mix(accent, white, 0.88);
        self.accent_press = mix(accent, black, 0.88);
        self.accent_soft = mix(accent, clear, 0.16);
        self.accent_line = mix(accent, clear, 0.34);
        self
    }
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().strip_prefix('#')?;
    let v = match s.len() {
        3 => s.chars().map(|c| c.to_digit(16).map(|d| d * 17)).collect::<Option<Vec<_>>>()?.iter().fold(0, |a, d| a << 8 | d),
        6 => u32::from_str_radix(s, 16).ok()?,
        _ => return None,
    };
    Some(hex(v))
}

// ---- type scale (px at 96 DPI) ---------------------------------------------
pub const TEXT_XS: f32 = 11.5;
pub const TEXT_SM: f32 = 13.0;
pub const TEXT_BASE: f32 = 15.0;
pub const TEXT_LG: f32 = 19.0;
pub const TEXT_XL: f32 = 24.0;

// ---- space -------------------------------------------------------------------
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S6: f32 = 24.0;
pub const S8: f32 = 32.0;
pub const S16: f32 = 64.0;

pub const RADIUS_SM: f32 = 5.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_XL: f32 = 16.0;

pub const TITLEBAR_H: f32 = 44.0;
pub const NAV_ROW_H: f32 = 32.0;
pub const SIDE_PANEL_MIN_W: f32 = 200.0;
pub const SIDE_PANEL_MAX_W: f32 = 480.0;
pub const SIDE_PANEL_HEAD_H: f32 = 90.0;

// ---- motion ------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
pub enum Motion {
    Full,
    Reduced,
    Off,
}

#[derive(Clone, Copy)]
pub enum Dur {
    Instant,
    Fast,
    Base,
    Slow,
}

impl Dur {
    pub fn ms(self, motion: Motion) -> f32 {
        match (motion, self) {
            (Motion::Off, _) => 1.0,
            (_, Dur::Instant) => 90.0,
            (_, Dur::Fast) => 160.0,
            (Motion::Reduced, Dur::Base) => 140.0,
            (_, Dur::Base) => 240.0,
            (Motion::Reduced, Dur::Slow) => 200.0,
            (_, Dur::Slow) => 420.0,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Ease {
    Out,
    Spring,
}

impl Ease {
    pub fn at(self, t: f32) -> f32 {
        let (x1, y1, x2, y2) = match self {
            Ease::Out => (0.16, 1.0, 0.3, 1.0),
            Ease::Spring => (0.34, 1.56, 0.64, 1.0),
        };
        cubic_bezier(x1, y1, x2, y2, t.clamp(0.0, 1.0))
    }
}

/// CSS `cubic-bezier`: solve x(s) = t by Newton steps, then return y(s).
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let bez = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s) * (1.0 - s) + 3.0 * b * s * s * (1.0 - s) + s * s * s;
    let mut s = t;
    for _ in 0..8 {
        let x = bez(x1, x2, s) - t;
        let dx = 3.0 * x1 * (1.0 - s) * (1.0 - s) + 6.0 * (x2 - x1) * s * (1.0 - s) + 3.0 * (1.0 - x2) * s * s;
        if dx.abs() < 1e-6 {
            break;
        }
        s = (s - x / dx).clamp(0.0, 1.0);
    }
    bez(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_hits_the_ends_and_overshoots_only_for_spring() {
        for e in [Ease::Out, Ease::Spring] {
            assert!(e.at(0.0).abs() < 1e-4 && (e.at(1.0) - 1.0).abs() < 1e-4);
        }
        assert!(Ease::Out.at(0.5) > 0.8);
        assert!((0..100).any(|i| Ease::Spring.at(i as f32 / 100.0) > 1.0));
    }

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(parse_hex("#fff").map(|c| c.r), Some(1.0));
        assert_eq!(parse_hex("#d97757").map(|c| (c.r * 255.0).round()), Some(217.0));
        assert!(parse_hex("red").is_none());
    }
}
