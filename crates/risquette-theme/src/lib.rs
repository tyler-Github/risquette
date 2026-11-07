//! Discuit's design tokens (from Discuit's `ui/src/scss/_base.scss`) as GPUI values.
//!
//! Colours mirror `:root` (light) and `html.theme-dark` (dark). Sizes follow
//! the CSS where `1rem = 10px` (`--fs-base`).

use gpui::{px, App, Global, Hsla, Pixels, Rgba};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl Mode {
    pub fn toggled(self) -> Self {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
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
        let colors = match mode {
            Mode::Light => light(),
            Mode::Dark => dark(),
        };
        Theme {
            mode,
            colors,
            fs: FontSizes {
                xs: px(12.),
                s: px(13.),
                normal: px(14.),
                l: px(15.),
                xl: px(16.),
                xxl: px(25.),
                xxxl: px(28.),
            },
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

    pub fn set_mode(mode: Mode, cx: &mut App) {
        cx.set_global(Theme::new(mode));
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
}
