//! Page layout: a main column that fills the available width (up to a
//! readable maximum) plus an optional right-hand side panel.

use gpui::{div, prelude::*, px, AnyElement, Div, Window};

/// Widest the main column gets.
pub const MAIN_MAX: f32 = 980.;
/// Width of the right-hand panel.
pub const ASIDE_WIDTH: f32 = 320.;
/// Widest a single-column page (settings, forms, lists) gets.
pub const PAGE_MAX: f32 = 1320.;
/// Below this window width the side panel is hidden.
const ASIDE_BREAKPOINT: f32 = 1180.;

pub fn aside_visible(window: &Window) -> bool {
    window.viewport_size().width > px(ASIDE_BREAKPOINT)
}

/// Lay out `main` with an optional side panel that scrolls independently.
/// When the window is too narrow the panel is hidden, or — with `on_top_when_narrow` —
/// shown above `main` in a capped, scrollable strip.
pub fn with_aside(main: impl IntoElement, aside: Option<AnyElement>, on_top_when_narrow: bool, window: &Window) -> Div {
    if !aside_visible(window) && on_top_when_narrow {
        if let Some(aside) = aside {
            return div().size_full().flex().justify_center().px(px(16.)).child(
                div()
                    .size_full()
                    .max_w(px(MAIN_MAX))
                    .flex()
                    .flex_col()
                    .child(div().id("aside-top").flex_none().max_h(px(360.)).overflow_y_scroll().pt(px(12.)).child(aside))
                    .child(div().flex_1().min_h_0().child(main)),
            );
        }
    }
    let aside = aside.filter(|_| aside_visible(window));
    let max = if aside.is_some() { MAIN_MAX + ASIDE_WIDTH + 16. } else { MAIN_MAX };
    div().size_full().flex().justify_center().px(px(16.)).child(
        div()
            .h_full()
            .w_full()
            .max_w(px(max))
            .flex()
            .gap(px(16.))
            .child(div().flex_1().min_w_0().h_full().child(main))
            .when_some(aside, |d, aside| {
                d.child(div().id("aside").w(px(ASIDE_WIDTH)).flex_none().h_full().overflow_y_scroll().pt(px(12.)).pb(px(16.)).child(aside))
            }),
    )
}
