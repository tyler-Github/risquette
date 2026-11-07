//! Small reusable building blocks styled after `_components.scss`.

use crate::{
    icons::{self, Icon},
    state::ImageCache,
};
use chrono::{DateTime, Utc};
use gpui::{AnyElement, App, ElementId, FontWeight, ObjectFit, SharedString, div, img, prelude::*, px};
use risquette_theme::ActiveTheme;

/// `.card`: background, radius and the 1px ring Discuit uses as a border.
pub fn card(cx: &App) -> gpui::Div {
    let t = cx.theme();
    div().bg(t.colors.card).rounded(t.metrics.radius).border_1().border_color(t.colors.card_ring)
}

fn button_base(
    id: impl Into<ElementId>,
    leading: Option<Icon>,
    label: impl Into<SharedString>,
    bg: gpui::Hsla,
    fg: gpui::Hsla,
    hover: gpui::Hsla,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .px(px(12.))
        .py(px(6.))
        .rounded(t.metrics.radius)
        .bg(bg)
        .text_color(fg)
        .text_size(t.fs.normal)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .when_some(leading, |d, i| d.gap(px(6.)).child(icons::icon(i, fg)))
        .child(label.into())
}

/// Plain grey `.button`.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    button_base(id, None, label, t.colors.button, t.colors.button_text, t.colors.button_hover, cx)
}

/// `.button-main`: the brand-coloured primary button.
pub fn button_main(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    button_base(id, None, label, t.colors.button_main, t.colors.button_main_text, t.colors.button_main_hover, cx)
}

/// `.button-red`: destructive actions.
pub fn button_danger(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    button_base(id, None, label, t.colors.red, gpui::white(), t.colors.red.opacity(0.85), cx)
}

/// A disabled-looking version of any button (clicks still need to be ignored by the caller).
pub fn disabled(el: gpui::Stateful<gpui::Div>, is_disabled: bool) -> gpui::Stateful<gpui::Div> {
    if is_disabled { el.opacity(0.6).cursor_default() } else { el }
}

/// A checkbox with a label.
pub fn checkbox(id: impl Into<ElementId>, checked: bool, label: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .text_size(t.fs.normal)
        .text_color(t.colors.text)
        .child(
            div()
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .border_color(if checked { t.colors.brand } else { t.colors.text_light })
                .bg(if checked { t.colors.brand } else { gpui::transparent_black() })
                .text_color(gpui::white())
                .text_size(px(11.))
                .when(checked, |d| d.child(icons::sized(Icon::Check, px(12.), gpui::white()))),
        )
        .child(label.into())
}

/// A small label above a form field.
pub fn field_label(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let t = cx.theme();
    div().text_size(t.fs.s).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text_light).child(text.into())
}

/// A centered message (empty states, errors).
pub fn notice(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let t = cx.theme();
    div().py(px(24.)).flex().justify_center().text_color(t.colors.text_light).text_size(t.fs.normal).child(text.into())
}

/// A section heading inside a card.
pub fn heading(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let t = cx.theme();
    div().text_size(t.fs.l + px(1.)).font_weight(FontWeight::BOLD).text_color(t.colors.text).child(text.into())
}

/// A "…" button that opens a context menu at the mouse.
pub fn menu_button(id: impl Into<ElementId>, items: impl Fn(&App) -> Vec<crate::nav::MenuItem> + 'static, cx: &App) -> gpui::Stateful<gpui::Div> {
    let color = cx.theme().colors.text_light;
    button_clear(id, cx).px(px(6.)).child(icons::icon(Icon::Ellipsis, color)).on_click(move |ev, window, cx| {
        cx.stop_propagation();
        let pos = if ev.position() == gpui::Point::default() { window.mouse_position() } else { ev.position() };
        let list = items(cx);
        crate::nav::open_menu(pos, list, cx);
    })
}

/// A square, icon-only button with a tooltip.
pub fn icon_button(id: impl Into<ElementId>, icon: Icon, tip: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    let hover = t.colors.button;
    let tip: SharedString = tip.into();
    div()
        .id(id)
        .size(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(t.metrics.radius)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(icons::sized(icon, px(18.), t.colors.text))
        .tooltip(tooltip(tip))
}

/// A grey button with a leading icon.
pub fn button_with_icon(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    button_base(id, Some(icon), label, t.colors.button, t.colors.button_text, t.colors.button_hover, cx)
}

/// The primary button with a leading icon.
pub fn button_main_with_icon(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    button_base(id, Some(icon), label, t.colors.button_main, t.colors.button_main_text, t.colors.button_main_hover, cx)
}

/// Build a tooltip for `.tooltip(...)`.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut gpui::Window, &mut App) -> gpui::AnyView + 'static {
    let text: SharedString = text.into();
    move |_, cx| {
        let text = text.clone();
        gpui::AppContext::new(cx, |_| Tooltip(text)).into()
    }
}

pub struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .bg(t.colors.fg.opacity(0.9))
            .text_color(t.colors.bg)
            .text_size(t.fs.xs)
            .font_family(t.font_family)
            .child(self.0.clone())
    }
}

/// `.button-clear`: transparent until hovered.
pub fn button_clear(id: impl Into<ElementId>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    let hover = t.colors.button;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(6.))
        .rounded(t.metrics.radius)
        .text_color(t.colors.text)
        .text_size(t.fs.normal)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
}

/// Whether a button is in its "selected" state (e.g. the active feed sort).
pub fn selected(el: gpui::Stateful<gpui::Div>, on: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    if on {
        let t = cx.theme();
        el.bg(t.colors.button).font_weight(FontWeight::SEMIBOLD)
    } else {
        el
    }
}

/// A remote image, or a placeholder in the image's average colour while it downloads.
///
/// GPUI doesn't clip images to a parent's rounded corners, so the radius is
/// applied to the image (and placeholder) itself.
pub fn remote_image(url: &str, fit: ObjectFit, radius: gpui::Pixels, placeholder: gpui::Hsla, cx: &mut App) -> AnyElement {
    // Absolutely positioned so the image always takes its box's size instead of
    // growing to its natural aspect ratio.
    let inner = match ImageCache::get(url, cx) {
        Some(image) => img(image).absolute().top_0().left_0().size_full().rounded(radius).object_fit(fit).into_any_element(),
        None => div().absolute().top_0().left_0().size_full().rounded(radius).bg(placeholder).into_any_element(),
    };
    div().relative().size_full().overflow_hidden().rounded(radius).child(inner).into_any_element()
}

/// A round avatar: community/user picture or a lettered fallback.
pub fn avatar(url: Option<&str>, name: &str, size: gpui::Pixels, cx: &mut App) -> AnyElement {
    let t = cx.theme();
    let (bg, fg) = (t.colors.brand, t.colors.button_main_text);
    let skeleton = t.colors.skeleton;
    let inner = match url {
        Some(u) => remote_image(u, ObjectFit::Cover, size / 2., skeleton, cx),
        None => div()
            .size_full()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(bg)
            .text_color(fg)
            .text_size(size * 0.5)
            .font_weight(FontWeight::BOLD)
            .child(name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())
            .into_any_element(),
    };
    div().size(size).flex_none().rounded_full().overflow_hidden().child(inner).into_any_element()
}

/// "5m ago" style relative time, matching Discuit's `timeAgo`.
pub fn time_ago(t: DateTime<Utc>) -> String {
    let secs = (Utc::now() - t).num_seconds().max(0);
    let (n, unit) = match secs {
        s if s < 60 => return "Just now".into(),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3600, "hour"),
        s if s < 2_592_000 => (s / 86_400, "day"),
        s if s < 31_536_000 => (s / 2_592_000, "month"),
        s => (s / 31_536_000, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// "1.2k" style counts.
pub fn compact(n: i64) -> String {
    let a = n.abs() as f64;
    let s = if a >= 1_000_000.0 {
        format!("{:.1}m", a / 1_000_000.0)
    } else if a >= 10_000.0 {
        format!("{:.0}k", a / 1000.0)
    } else if a >= 1000.0 {
        format!("{:.1}k", a / 1000.0)
    } else {
        return n.to_string();
    };
    let s = s.replace(".0", "");
    if n < 0 { format!("-{s}") } else { s }
}

pub fn plural(n: i64, word: &str) -> String {
    format!("{} {word}{}", compact(n), if n == 1 { "" } else { "s" })
}

/// Parse an image's `averageColor` into a placeholder colour.
pub fn placeholder_color(image: &discuit_api::Image, cx: &App) -> gpui::Hsla {
    image
        .average_rgb()
        .map(|(r, g, b)| gpui::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. }.into())
        .unwrap_or(cx.theme().colors.skeleton)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_counts() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1000), "1k");
        assert_eq!(compact(1240), "1.2k");
        assert_eq!(compact(15_300), "15k");
        assert_eq!(compact(-2000), "-2k");
        assert_eq!(plural(1, "comment"), "1 comment");
        assert_eq!(plural(3, "comment"), "3 comments");
    }
}
