//! Design tokens — the single source of every colour, space and motion value,
//! ported from `tokens.css`, in the `paper` style (light is the primary theme). No colour literal belongs anywhere else in the app.

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
pub const fn mix(a: Color, b: Color, p: f32) -> Color {
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
    /// Text and glyphs on a filled accent.
    pub on_accent: Color,
    pub accent_hover: Color,
    pub accent_press: Color,
    pub accent_soft: Color,
    pub accent_line: Color,
    /// A faint accent fill behind a block that should stand out: the H1 band.
    pub accent_wash: Color,
    /// The title bar: the page tinted with the accent, so the window reads as MDWisp's.
    pub chrome: Color,
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
}

/// Style `paper` (LaTeX paper), see the ui-style catalog: warm off-white
/// stock, ink text, booktabs rules, radius 0, no shadows. The accent is
/// MDWisp's own hue (muted grape); links share it, so there is one accent.
pub const DARK: Theme = Theme {
    dark: true,
    bg_base: hex(0x1a1a18),
    bg_raised: hex(0x212120),
    bg_overlay: hex(0x2a2a27),
    bg_inset: hex(0x212120),
    bg_hover: rgba(232, 230, 223, 0.045),
    bg_active: rgba(232, 230, 223, 0.08),
    border_soft: hex(0x3d3d39),
    border_strong: hex(0x56564f),
    text_primary: hex(0xe8e6df),
    text_secondary: hex(0xc4c2b9),
    text_tertiary: hex(0xa09e95),
    text_disabled: hex(0x989689),
    accent: hex(0xb49ddc),
    on_accent: hex(0x111111),
    accent_hover: hex(0xbda9e0),
    accent_press: hex(0x9e8ac2),
    accent_soft: rgba(180, 157, 220, SEL_DARK),
    accent_line: rgba(180, 157, 220, 0.34),
    accent_wash: rgba(180, 157, 220, WASH_DARK),
    chrome: mix(hex(0xb49ddc), hex(0x1a1a18), CHROME_DARK),
    success: hex(0x8cc28e),
    warning: hex(0xe09a5a),
    danger: hex(0xea7a72),
    link: hex(0xb49ddc),
    hl_keyword: hex(0x80b2e2),
    hl_string: hex(0xea7a72),
    hl_number: hex(0xe09a5a),
    hl_comment: hex(0x8a8880),
    hl_func: hex(0xe8e6df),
    hl_type: hex(0x8cc28e),
    hl_punct: hex(0xa09e95),
    hl_attr: hex(0xd9bd62),
};

pub const LIGHT: Theme = Theme {
    dark: false,
    bg_base: hex(0xfdfdfb),
    bg_raised: hex(0xf5f5f0),
    bg_overlay: hex(0xfdfdfb),
    bg_inset: hex(0xf5f5f0),
    bg_hover: rgba(17, 17, 17, 0.04),
    bg_active: rgba(17, 17, 17, 0.07),
    border_soft: hex(0xd4d4d0),
    border_strong: hex(0xb0b0aa),
    text_primary: hex(0x111111),
    text_secondary: hex(0x45453f),
    text_tertiary: hex(0x66665f),
    text_disabled: hex(0x6e6e67),
    accent: hex(0x6a4c9c),
    on_accent: hex(0xffffff),
    accent_hover: hex(0x7c61a8),
    accent_press: hex(0x5d4389),
    accent_soft: rgba(106, 76, 156, SEL_LIGHT),
    accent_line: rgba(106, 76, 156, 0.34),
    accent_wash: rgba(106, 76, 156, WASH_LIGHT),
    chrome: mix(hex(0x6a4c9c), hex(0xfdfdfb), CHROME_LIGHT),
    success: hex(0x2e6b30),
    warning: hex(0xa85010),
    danger: hex(0xb0231f),
    link: hex(0x6a4c9c),
    hl_keyword: hex(0x0b5394),
    hl_string: hex(0xb0231f),
    hl_number: hex(0xa85010),
    hl_comment: hex(0x66665f),
    hl_func: hex(0x111111),
    hl_type: hex(0x2e6b30),
    hl_punct: hex(0x66665f),
    hl_attr: hex(0x856600),
};

/// Selection alpha of the style: `#RRGGBB24` in light, `#RRGGBB47` in dark.
const SEL_LIGHT: f32 = 0x24 as f32 / 255.0;
const SEL_DARK: f32 = 0x47 as f32 / 255.0;
const WASH_LIGHT: f32 = 0.1;
const WASH_DARK: f32 = 0.14;
const CHROME_LIGHT: f32 = 0.18;
const CHROME_DARK: f32 = 0.22;

/// The Windows close button: the one place a system colour is correct.
pub const CLOSE_HOVER: Color = hex(0xc42b1c);
pub const CLOSE_PRESS: Color = hex(0xb1271a);
pub const WHITE: Color = hex(0xffffff);

impl Theme {
    /// The accent is one value to the user and seven tokens to the drawing code.
    pub fn with_accent(mut self, accent: Color) -> Theme {
        let white = hex(0xffffff);
        let black = hex(0x000000);
        let clear = Color { a: 0.0, ..accent };
        self.accent = accent;
        self.accent_hover = mix(accent, white, 0.88);
        self.accent_press = mix(accent, black, 0.88);
        self.accent_soft = mix(accent, clear, if self.dark { SEL_DARK } else { SEL_LIGHT });
        self.accent_line = mix(accent, clear, 0.34);
        self.accent_wash = mix(accent, clear, if self.dark { WASH_DARK } else { WASH_LIGHT });
        self.chrome = mix(accent, self.bg_base, if self.dark { CHROME_DARK } else { CHROME_LIGHT });
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
pub const TEXT_XS: f32 = 12.0;
pub const TEXT_SM: f32 = 13.0;
pub const TEXT_BASE: f32 = 14.0;
pub const TEXT_LG: f32 = 20.0;

// ---- space -------------------------------------------------------------------
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S6: f32 = 24.0;
pub const S8: f32 = 32.0;
pub const S16: f32 = 64.0;

/// Paper has no rounded corners; the names stay so a shape can be given one later.
pub const RADIUS_SM: f32 = 0.0;
pub const RADIUS_MD: f32 = 0.0;
pub const RADIUS_LG: f32 = 0.0;
pub const RADIUS_XL: f32 = 0.0;

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
            // Paper: colour and opacity changes of 150 ms at most.
            (_, Dur::Instant) => 90.0,
            (_, Dur::Fast) => 120.0,
            (Motion::Reduced, Dur::Base) => 120.0,
            (_, Dur::Base) => 150.0,
            (_, Dur::Slow) => 150.0,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Ease {
    Out,
}

impl Ease {
    pub fn at(self, t: f32) -> f32 {
        let (x1, y1, x2, y2) = match self {
            Ease::Out => (0.16, 1.0, 0.3, 1.0),
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
    fn easing_hits_the_ends_and_never_overshoots() {
        assert!(Ease::Out.at(0.0).abs() < 1e-4 && (Ease::Out.at(1.0) - 1.0).abs() < 1e-4);
        assert!(Ease::Out.at(0.5) > 0.8);
        assert!((0..100).all(|i| Ease::Out.at(i as f32 / 100.0) <= 1.0 + 1e-4));
    }

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(parse_hex("#fff").map(|c| c.r), Some(1.0));
        assert_eq!(parse_hex("#6a4c9c").map(|c| (c.r * 255.0).round()), Some(106.0));
        assert!(parse_hex("red").is_none());
    }

    #[test]
    fn base_accent_tokens_follow_with_accent() {
        for th in [DARK, LIGHT] {
            let d = th.with_accent(th.accent);
            for (a, b) in [(th.accent_hover, d.accent_hover), (th.accent_press, d.accent_press), (th.accent_soft, d.accent_soft), (th.accent_line, d.accent_line), (th.accent_wash, d.accent_wash), (th.chrome, d.chrome)] {
                assert!([a.r - b.r, a.g - b.g, a.b - b.b, a.a - b.a].iter().all(|v| v.abs() <= 0.5 / 255.0 + 1e-6));
            }
        }
    }
}
