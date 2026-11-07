//! Lucide icons (ISC licensed, see `assets/LICENSE-lucide.txt`), bundled into
//! the binary and served to GPUI through an [`AssetSource`].

use gpui::{prelude::*, px, svg, AssetSource, Hsla, Pixels, SharedString, Svg};
use std::borrow::Cow;

macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        /// An icon from the bundled Lucide set.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            pub fn path(self) -> &'static str {
                match self {
                    $(Icon::$variant => concat!("icons/", $file, ".svg"),)*
                }
            }
        }

        fn bundled(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $file, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $file, ".svg"))),)*
                _ => None,
            }
        }

        const ALL_PATHS: &[&str] = &[$(concat!("icons/", $file, ".svg"),)*];
    };
}

icons! {
    Activity => "activity",
    ArrowBigDown => "arrow-big-down",
    ArrowBigUp => "arrow-big-up",
    Bell => "bell",
    Bookmark => "bookmark",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    Clock => "clock",
    Compass => "compass",
    CircleCheck => "circle-check",
    Copy => "copy",
    Ellipsis => "ellipsis",
    ExternalLink => "external-link",
    Download => "download",
    EyeOff => "eye-off",
    FileText => "file-text",
    Flag => "flag",
    Flame => "flame",
    Globe => "globe",
    House => "house",
    Image => "image",
    Inbox => "inbox",
    LayoutGrid => "layout-grid",
    Link => "link",
    List => "list",
    Lock => "lock",
    LockOpen => "lock-open",
    LogIn => "log-in",
    LogOut => "log-out",
    MailCheck => "mail-check",
    Menu => "menu",
    MessageSquare => "message-square",
    Monitor => "monitor",
    Moon => "moon",
    PanelLeft => "panel-left",
    Pencil => "pencil",
    Pin => "pin",
    Play => "play",
    Plus => "plus",
    RefreshCw => "refresh-cw",
    Reply => "reply",
    Search => "search",
    Server => "server",
    Settings => "settings",
    Shield => "shield",
    ShieldCheck => "shield-check",
    Sparkles => "sparkles",
    Sun => "sun",
    Trash => "trash-2",
    TrendingUp => "trending-up",
    Upload => "upload",
    User => "user",
    VolumeX => "volume-x",
    X => "x",
}

/// Serves the bundled icons to GPUI.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(bundled(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ALL_PATHS.iter().filter(|p| p.starts_with(path)).map(|p| SharedString::from(*p)).collect())
    }
}

/// An icon element. Colour follows `color`; size defaults to 16px.
pub fn icon(icon: Icon, color: Hsla) -> Svg {
    sized(icon, px(16.), color)
}

pub fn sized(icon: Icon, size: Pixels, color: Hsla) -> Svg {
    svg().path(icon.path()).size(size).flex_none().text_color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_bundled() {
        for p in ALL_PATHS {
            let bytes = bundled(p).unwrap();
            assert!(std::str::from_utf8(bytes).unwrap().contains("<svg"), "{p}");
        }
        assert_eq!(Icon::House.path(), "icons/house.svg");
    }
}
