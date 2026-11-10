//! Discuit's design tokens (from Discuit's `ui/src/scss/_base.scss`) as GPUI values.
//!
//! Colours mirror `:root` (light) and `html.theme-dark` (dark). Sizes follow
//! the CSS where `1rem = 10px` (`--fs-base`). High contrast is Risquette's own,
//! for light and dark: pure black and white, strong borders, and accents whose
//! contrast with the background meets WCAG AAA (7:1).

use gpui::{App, Global, Hsla, Pixels, Rgba, px};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl Mode {
    /// The light/dark toggle (Ctrl+Shift+L).
    pub fn toggled(self) -> Self {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
        }
    }

    pub fn is_dark(self) -> bool {
        self == Mode::Dark
    }

    /// The mode for the saved theme setting (`"light"`, `"dark"`, or none to follow the system).
    pub fn from_setting(setting: Option<&str>, system_dark: bool) -> Self {
        match setting {
            Some("light") => Mode::Light,
            Some("dark") => Mode::Dark,
            _ if system_dark => Mode::Dark,
            _ => Mode::Light,
        }
    }

    /// The value saved for this mode.
    pub fn setting(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }
}

/// Font sizes: `--fs-*`.
#[derive(Debug, Clone, Copy)]
pub struct FontSizes {
    pub xs: Pixels,
    pub s: Pixels,
    pub normal: Pixels,
    pub l: Pixels,
    pub xl: Pixels,
    pub xxl: Pixels,
    pub xxxl: Pixels,
}

/// Layout metrics shared by both modes.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// `--gap` (1.5rem)
    pub gap: Pixels,
    /// `--border-radius`
    pub radius: Pixels,
    /// `--navbar-height`
    pub navbar_height: Pixels,
    /// `--sidebar-left-width`
    pub sidebar_width: Pixels,
    /// `--page-width-desktop`
    pub page_width: Pixels,
    /// `--post-votes-size`
    pub post_votes_size: Pixels,
    /// `--input-height` (3.2rem)
    pub input_height: Pixels,
    /// `--button-icon-size`
    pub icon_size: Pixels,
}

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    /// `--color-body`: the page background behind cards.
    pub body: Hsla,
    /// `--color-bg` / `--color-card`.
    pub bg: Hsla,
    pub card: Hsla,
    pub card_border: Hsla,
    /// `--color-card-domain`: link post hostname.
    pub card_domain: Hsla,
    /// `--color-fg`.
    pub fg: Hsla,
    pub text: Hsla,
    pub text_light: Hsla,
    pub link: Hsla,
    pub brand: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub gray: Hsla,
    pub button: Hsla,
    pub button_hover: Hsla,
    pub button_text: Hsla,
    pub button_main: Hsla,
    pub button_main_hover: Hsla,
    pub button_main_text: Hsla,
    pub input: Hsla,
    /// Outline of a text box that isn't focused (none, except in high contrast).
    pub input_border: Hsla,
    pub input_border_hover: Hsla,
    pub input_border_focus: Hsla,
    pub skeleton: Hsla,
    pub comment_line: Hsla,
    /// `--seperator`: 1px rgba(fg, .1)
    pub separator: Hsla,
    /// `--card-shadow-style-1` ring used as a border in light mode.
    pub card_ring: Hsla,
    pub scrollbar: Hsla,
    pub mini_footer_text: Hsla,
    pub modal_overlay: Hsla,
    pub supporter: Hsla,
    /// Upvote / downvote accents (Discuit uses brand for up, red for down).
    pub upvote: Hsla,
    pub downvote: Hsla,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub mode: Mode,
    /// The high-contrast version of `mode`.
    pub high_contrast: bool,
    pub colors: Colors,
    pub fs: FontSizes,
    pub metrics: Metrics,
    /// Font family: Discuit uses InterVariable with system fallbacks.
    pub font_family: &'static str,
}

impl Global for Theme {}

fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    rgba(r, g, b, 1.0)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }.into()
}

fn hex(v: u32) -> Hsla {
    rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// `#rrggbbaa`
fn hexa(v: u32) -> Hsla {
    rgba((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, (v & 0xff) as f32 / 255.0)
}

/// `--base-N` steps: 0,25,50,…,255.
fn base(n: u8) -> u8 {
    if n >= 10 { 255 } else { n * 25 }
}

fn gray(n: u8) -> Hsla {
    let v = base(n);
    rgb(v, v, v)
}

fn gray_a(n: u8, a: f32) -> Hsla {
    let v = base(n);
    rgba(v, v, v, a)
}

impl Theme {
    pub fn new(mode: Mode) -> Self {
        Self::with_contrast(mode, false)
    }

    /// `mode`, in its high-contrast version if `high_contrast`.
    pub fn with_contrast(mode: Mode, high_contrast: bool) -> Self {
        let colors = match (mode, high_contrast) {
            (Mode::Light, false) => light(),
            (Mode::Dark, false) => dark(),
            (Mode::Light, true) => high_contrast_light(),
            (Mode::Dark, true) => high_contrast_dark(),
        };
        Theme {
            mode,
            high_contrast,
            colors,
            fs: FontSizes { xs: px(12.), s: px(13.), normal: px(14.), l: px(15.), xl: px(16.), xxl: px(25.), xxxl: px(28.) },
            metrics: Metrics {
                gap: px(15.),
                radius: px(5.),
                navbar_height: px(50.),
                sidebar_width: px(180.),
                page_width: px(1200.),
                post_votes_size: px(50.),
                input_height: px(32.),
                icon_size: px(21.),
            },
            font_family: if cfg!(target_os = "windows") {
                "Segoe UI"
            } else if cfg!(target_os = "macos") {
                ".SystemUIFont"
            } else {
                "Inter"
            },
        }
    }

    pub fn global(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    /// Switch light/dark, keeping high contrast as it is.
    pub fn set_mode(mode: Mode, cx: &mut App) {
        let high_contrast = cx.try_global::<Theme>().is_some_and(|t| t.high_contrast);
        cx.set_global(Theme::with_contrast(mode, high_contrast));
        cx.refresh_windows();
    }

    /// Turn high contrast on or off, keeping light/dark as it is.
    pub fn set_high_contrast(high_contrast: bool, cx: &mut App) {
        let mode = cx.try_global::<Theme>().map_or(Mode::Light, |t| t.mode);
        cx.set_global(Theme::with_contrast(mode, high_contrast));
        cx.refresh_windows();
    }
}

fn light() -> Colors {
    let brand = rgb(107, 2, 242);
    let fg = gray(0);
    Colors {
        body: hex(0xeeeeee),
        bg: gray(10),
        card: gray(10),
        card_border: hexa(0x00000029),
        card_domain: hex(0x8f8f8f),
        fg,
        text: gray(2),
        text_light: gray(4),
        link: brand,
        brand,
        red: rgb(236, 72, 72),
        green: rgb(34, 164, 34),
        gray: gray(5),
        button: gray_a(8, 0.7),
        button_hover: gray_a(8, 0.9),
        button_text: gray(2),
        button_main: brand,
        button_main_hover: hex(0x5800c9),
        button_main_text: gray(10),
        input: gray_a(4, 0.15),
        input_border: rgba(0, 0, 0, 0.),
        input_border_hover: hex(0xd7d7d7),
        input_border_focus: hex(0x7f6bff),
        skeleton: rgba(0, 0, 0, 0.05),
        comment_line: hex(0xd3d3d3),
        separator: rgba(0, 0, 0, 0.1),
        card_ring: rgba(0, 0, 0, 0.09),
        scrollbar: gray(5),
        mini_footer_text: hex(0x667b8c),
        modal_overlay: gray_a(9, 0.59),
        supporter: rgb(101, 54, 140),
        upvote: brand,
        downvote: rgb(236, 72, 72),
    }
}

fn dark() -> Colors {
    let brand = rgb(117, 110, 255);
    Colors {
        body: rgb(15, 15, 15),
        bg: rgb(32, 32, 32),
        card: rgb(32, 32, 32),
        card_border: hexa(0xffffff14),
        card_domain: hex(0x8f8f8f),
        fg: gray(10),
        text: rgb(210, 210, 210),
        text_light: gray(8),
        link: rgb(128, 122, 255),
        brand,
        red: rgb(236, 72, 72),
        green: rgb(34, 164, 34),
        gray: gray(5),
        button: gray_a(4, 0.7),
        button_hover: gray(4),
        button_text: gray(10),
        button_main: rgb(80, 72, 238),
        button_main_hover: rgb(62, 52, 238),
        button_main_text: gray(10),
        input: gray_a(4, 0.25),
        input_border: rgba(0, 0, 0, 0.),
        input_border_hover: hex(0x4f4f4f),
        input_border_focus: hex(0x7f6bff),
        skeleton: rgba(255, 255, 255, 0.05),
        comment_line: hexa(0xd3d3d35e),
        separator: rgba(255, 255, 255, 0.1),
        card_ring: hexa(0xffffff14),
        scrollbar: gray(5),
        mini_footer_text: gray(6),
        modal_overlay: gray_a(2, 0.59),
        supporter: hex(0xe9c8ff),
        upvote: brand,
        downvote: rgb(236, 72, 72),
    }
}

/// Black on white with dark borders. Accents stay distinct from the text: a
/// deep violet brand, dark blue links.
fn high_contrast_light() -> Colors {
    let brand = rgb(74, 0, 180);
    let blue = rgb(0, 60, 170);
    Colors {
        body: gray(10),
        bg: gray(10),
        card: gray(10),
        card_border: rgb(0, 0, 0),
        card_domain: rgb(50, 50, 50),
        fg: rgb(0, 0, 0),
        text: rgb(0, 0, 0),
        text_light: rgb(40, 40, 40),
        link: blue,
        brand,
        red: rgb(170, 0, 0),
        green: rgb(0, 100, 0),
        gray: rgb(60, 60, 60),
        button: rgb(225, 225, 225),
        button_hover: rgb(200, 200, 200),
        button_text: rgb(0, 0, 0),
        button_main: brand,
        button_main_hover: rgb(0, 0, 0),
        button_main_text: gray(10),
        input: rgb(235, 235, 235),
        input_border: rgb(70, 70, 70),
        input_border_hover: rgb(0, 0, 0),
        input_border_focus: blue,
        skeleton: rgba(0, 0, 0, 0.12),
        comment_line: rgb(90, 90, 90),
        separator: rgba(0, 0, 0, 0.6),
        card_ring: rgba(0, 0, 0, 0.75),
        scrollbar: rgb(60, 60, 60),
        mini_footer_text: rgb(40, 40, 40),
        modal_overlay: rgba(0, 0, 0, 0.6),
        supporter: rgb(90, 0, 150),
        upvote: brand,
        downvote: rgb(170, 0, 0),
    }
}

/// White on black with bright borders. Accents stay distinct from the text: a
/// light violet brand, yellow links.
fn high_contrast_dark() -> Colors {
    let brand = rgb(196, 181, 253);
    let yellow = rgb(255, 214, 10);
    Colors {
        body: rgb(0, 0, 0),
        bg: rgb(0, 0, 0),
        card: rgb(0, 0, 0),
        card_border: gray(10),
        card_domain: gray(9),
        fg: gray(10),
        text: gray(10),
        text_light: rgb(225, 225, 225),
        link: yellow,
        brand,
        red: rgb(255, 120, 120),
        green: rgb(120, 230, 120),
        gray: gray(8),
        button: rgb(40, 40, 40),
        button_hover: rgb(75, 75, 75),
        button_text: gray(10),
        button_main: brand,
        button_main_hover: gray(10),
        button_main_text: rgb(0, 0, 0),
        input: rgb(34, 34, 34),
        input_border: gray(8),
        input_border_hover: gray(10),
        input_border_focus: yellow,
        skeleton: rgba(255, 255, 255, 0.15),
        comment_line: gray(8),
        separator: rgba(255, 255, 255, 0.6),
        card_ring: rgba(255, 255, 255, 0.75),
        scrollbar: gray(9),
        mini_footer_text: gray(9),
        modal_overlay: rgba(0, 0, 0, 0.85),
        supporter: rgb(233, 200, 255),
        upvote: brand,
        downvote: rgb(255, 120, 120),
    }
}

/// Convenience accessor: `cx.theme()` on anything that derefs to `App`.
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        Theme::global(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_conversions() {
        assert_eq!(base(4), 100);
        assert_eq!(base(10), 255);
        let c: Rgba = hexa(0x00000029).into();
        assert!((c.a - 0x29 as f32 / 255.0).abs() < 1e-6);
        let b: Rgba = light().brand.into();
        assert!((b.r * 255.0 - 107.0).abs() < 0.5);
    }

    #[test]
    fn theme_settings() {
        for mode in [Mode::Light, Mode::Dark] {
            assert_eq!(Mode::from_setting(Some(mode.setting()), false), mode);
        }
        assert_eq!(Mode::from_setting(None, true), Mode::Dark);
        assert_eq!(Mode::from_setting(None, false), Mode::Light);
        assert!(Mode::Dark.is_dark() && Mode::Dark.toggled() == Mode::Light);
        let t = Theme::with_contrast(Mode::Light, true);
        assert!(t.high_contrast && t.mode == Mode::Light);
    }

    /// WCAG contrast ratio between two colours.
    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let lum = |c: Hsla| {
            let c: Rgba = c.into();
            let ch = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn high_contrast_meets_aaa() {
        for (theme, c) in [("light", high_contrast_light()), ("dark", high_contrast_dark())] {
            // WCAG AAA asks for 7:1 for normal text.
            for (name, fg) in [("text", c.text), ("text_light", c.text_light), ("link", c.link), ("brand", c.brand), ("red", c.red), ("green", c.green)] {
                assert!(contrast(fg, c.card) >= 7.0, "{theme} {name}: {:.1}", contrast(fg, c.card));
            }
            assert!(contrast(c.button_main_text, c.button_main) >= 7.0, "{theme} main button");
            assert!(contrast(c.button_text, c.button) >= 7.0 && contrast(c.button_text, c.button_hover) >= 7.0, "{theme} button");
            assert!(contrast(c.text, c.input) >= 7.0, "{theme} input");
        }
    }
}
