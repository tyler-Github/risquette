//! Post and comment cards shared by every feed.

use crate::{
    actions,
    icons::{self, Icon},
    markdown,
    nav::{self, Target},
    ui,
};
use discuit_api::{Comment, Post, PostType};
use gpui::{div, prelude::*, px, AnyElement, App, ElementId, FontWeight, ObjectFit, SharedString};
use risquette_theme::ActiveTheme;

/// The feed column's content width, used to size image previews.
pub const COLUMN_WIDTH: f32 = 820.;

#[derive(Clone, Copy)]
pub struct CardOptions {
    /// Show the community name (off inside a community's own feed).
    pub show_community: bool,
}

impl Default for CardOptions {
    fn default() -> Self {
        Self { show_community: true }
    }
}

/// Up arrow, points, down arrow, with the viewer's vote highlighted.
pub fn vote_pill(
    id: impl Into<SharedString>,
    points: i64,
    vote: Option<bool>,
    on_vote: impl Fn(bool, &mut App) + Clone + 'static,
    cx: &App,
) -> gpui::Div {
    let t = cx.theme();
    let id: SharedString = id.into();
    let (up_color, down_color) = (t.colors.upvote, t.colors.downvote);
    let count_color = match vote {
        Some(true) => up_color,
        Some(false) => down_color,
        None => t.colors.text,
    };
    let arrow = |dir: bool, glyph: Icon| {
        let on_vote = on_vote.clone();
        let active = vote == Some(dir);
        let color = if dir { up_color } else { down_color };
        div()
            .id(SharedString::from(format!("{id}-{}", if dir { "up" } else { "down" })))
            .px(px(6.))
            .py(px(2.))
            .rounded(px(10.))
            .cursor_pointer()
            .flex()
            .items_center()
            .hover(move |s| s.bg(color.opacity(0.12)))
            .child(icons::sized(glyph, px(17.), if active { color } else { t.colors.text_light }))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                on_vote(dir, cx);
            })
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .px(px(4.))
        .py(px(2.))
        .rounded(px(20.))
        .bg(t.colors.input)
        .font_weight(FontWeight::SEMIBOLD)
        .child(arrow(true, Icon::ArrowBigUp))
        .child(div().min_w(px(18.)).flex().justify_center().text_color(count_color).child(ui::compact(points)))
        .child(arrow(false, Icon::ArrowBigDown))
}

fn link(id: impl Into<ElementId>, text: impl Into<SharedString>, target: Target, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    div()
        .id(id)
        .text_color(t.colors.text)
        .cursor_pointer()
        .hover(|s| s.underline())
        .child(text.into())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            nav::go(target.clone(), cx);
        })
}

fn user_badge(group: Option<&str>, cx: &App) -> Option<AnyElement> {
    let t = cx.theme();
    let (label, color) = match group? {
        "mods" => ("MOD", t.colors.green),
        "admins" => ("ADMIN", t.colors.red),
        _ => return None,
    };
    Some(div().text_size(t.fs.xs).font_weight(FontWeight::BOLD).text_color(color).child(label).into_any_element())
}

pub fn post_card(post: &Post, key: &str, opts: CardOptions, cx: &mut App) -> AnyElement {
    let t = *cx.theme();
    let public_id = post.public_id.clone();

    let pro_pic = post.community_pro_pic.as_ref().map(|i| i.best_url(50).to_string());
    let mut header = div().flex().flex_wrap().items_center().gap(px(6.)).text_size(t.fs.s).text_color(t.colors.text_light);
    if opts.show_community {
        header = header
            .child(ui::avatar(pro_pic.as_deref(), &post.community_name, px(22.), cx))
            .child(link(SharedString::from(format!("{key}-comm")), post.community_name.clone(), Target::Community(post.community_name.clone()), cx).font_weight(FontWeight::SEMIBOLD))
            .child("·");
    }
    header = header
        .child("By")
        .child(link(SharedString::from(format!("{key}-user")), format!("@{}", post.username), Target::User(post.username.clone()), cx).text_color(t.colors.text_light))
        .children(user_badge(post.user_group.as_deref(), cx))
        .child("·")
        .child(ui::time_ago(post.created_at))
        .when(post.edited_at.is_some(), |d| d.child("· edited"))
        .when(post.is_pinned || post.is_pinned_site, |d| d.child(icons::sized(Icon::Pin, px(13.), t.colors.green)).child(div().text_color(t.colors.green).child("Pinned")))
        .when(post.locked, |d| d.child(icons::sized(Icon::Lock, px(13.), t.colors.red)).child(div().text_color(t.colors.red).child("Locked")));

    let title = div()
        .text_size(t.fs.xl + px(2.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if post.deleted { t.colors.text_light } else { t.colors.text })
        .line_height(px(24.))
        .child(if post.deleted && post.deleted_content { "[deleted]".to_string() } else { post.title.clone() });

    let mut content = div().flex().flex_col().gap(px(8.)).child(header);
    // Link thumbnail shown beside the header, title and link (not just the title),
    // so it doesn't make the card taller than its text.
    let mut side_thumb = None;

    match post.kind {
        PostType::Link => {
            let link_data = post.link.clone().unwrap_or_default();
            let host = link_data.hostname.clone().unwrap_or_else(|| link_data.url.clone());
            let url = link_data.url.clone();
            let host_link = div()
                .id(SharedString::from(format!("{key}-host")))
                .text_size(t.fs.s)
                .text_color(t.colors.card_domain)
                .cursor_pointer()
                .hover(|s| s.underline())
                .flex()
                .items_center()
                .gap(px(4.))
                .child(host)
                .child(icons::sized(Icon::ExternalLink, px(12.), t.colors.card_domain))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    cx.open_url(&url);
                });
            let embed = embed_view(key, &link_data.url, &post.title, link_data.image.as_ref(), cx);
            let thumb = if embed.is_none() { link_data.image.as_ref() } else { None };
            if let Some(thumb) = thumb {
                let url = thumb.copy("desktop").map(|c| c.url.clone()).unwrap_or_else(|| thumb.best_url(300).to_string());
                let ph = ui::placeholder_color(thumb, cx);
                side_thumb = Some(div().w(px(112.)).h(px(84.)).flex_none().child(ui::remote_image(&url, ObjectFit::Cover, t.metrics.radius, ph, cx)));
            }
            content = content.child(title).child(host_link.mt(px(-2.)));
            content = content.children(embed);
        }
        PostType::Image => {
            content = content.child(title);
            let images = post_images(post);
            if !images.is_empty() {
                content = content.child(carousel(&format!("{key}-car"), &post.id, &images, 520., false, cx));
            }
        }
        _ => {
            content = content.child(title);
            if let Some(body) = post.body.as_deref().filter(|b| !b.trim().is_empty()) {
                content = content.child(
                    div().text_size(t.fs.normal).text_color(t.colors.text_light).line_height(px(20.)).line_clamp(3).child(markdown::plain_preview(body, 300)),
                );
            }
        }
    }

    let vote_post = post.clone();
    let menu_post = post.clone();
    let footer = div()
        .flex()
        .items_center()
        .justify_between()
        .text_size(t.fs.normal)
        .text_color(t.colors.text_light)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(vote_pill(format!("{key}-vote"), post.points(), post.vote(), move |up, cx| actions::vote_post(&vote_post, up, cx), cx))
                .child(
                    div()
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(20.))
                        .bg(t.colors.input)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icons::sized(Icon::MessageSquare, px(15.), t.colors.text_light))
                        .child(ui::plural(post.no_comments, "comment"))
                        .when(post.new_comments > 0, |d| d.child(div().text_color(t.colors.brand).child(format!(" (+{} new)", post.new_comments)))),
                ),
        )
        .child(ui::menu_button(SharedString::from(format!("{key}-menu")), move |cx| actions::post_menu(&menu_post, cx), cx));

    let content = match side_thumb {
        Some(thumb) => div().flex().items_start().gap(t.metrics.gap).child(content.flex_1().min_w_0()).child(thumb),
        None => content,
    };

    let hover_ring = t.colors.card_border;
    ui::card(cx)
        .id(SharedString::from(format!("{key}-card")))
        .p(t.metrics.gap)
        .flex()
        .flex_col()
        .gap(px(12.))
        .cursor_pointer()
        .hover(move |s| s.border_color(hover_ring))
        .child(content)
        .child(footer)
        .on_click(move |_, _, cx| nav::go(Target::Post { public_id: public_id.clone() }, cx))
        .into_any_element()
}

/// A comment shown outside its thread (profiles, saved lists).
pub fn comment_card(comment: &Comment, key: &str, cx: &mut App) -> AnyElement {
    let t = *cx.theme();
    let public_id = comment.post_public_id.clone();
    let community = comment.community_name.clone().unwrap_or_default();
    let vote_c = comment.clone();
    let menu_c = comment.clone();
    let community_id = String::new();
    let body = if comment.deleted { "[deleted]".to_string() } else { comment.body.clone() };
    ui::card(cx)
        .id(SharedString::from(format!("{key}-ccard")))
        .p(t.metrics.gap)
        .flex()
        .flex_col()
        .gap(px(8.))
        .cursor_pointer()
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .text_size(t.fs.s)
                .text_color(t.colors.text_light)
                .child(link(SharedString::from(format!("{key}-cu")), format!("@{}", comment.username), Target::User(comment.username.clone()), cx).font_weight(FontWeight::SEMIBOLD))
                .child("commented in")
                .child(link(SharedString::from(format!("{key}-cc")), community.clone(), Target::Community(community.clone()), cx).font_weight(FontWeight::SEMIBOLD))
                .child("·")
                .child(ui::time_ago(comment.created_at)),
        )
        .child(div().text_size(t.fs.normal).child(markdown::render(format!("{key}-cbody"), &body, cx)))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(t.fs.normal)
                .child(vote_pill(format!("{key}-cvote"), comment.points(), comment.vote(), move |up, cx| actions::vote_comment(&vote_c, up, cx), cx))
                .child(ui::menu_button(SharedString::from(format!("{key}-cmenu")), move |cx| actions::comment_menu(&menu_c, &community_id, cx), cx)),
        )
        .on_click(move |_, _, cx| {
            if let Some(id) = public_id.clone() {
                nav::go(Target::Post { public_id: id }, cx);
            }
        })
        .into_any_element()
}

/// A post's images with captions (newer posts use `images`, older ones `image`).
pub fn post_images(post: &Post) -> Vec<(discuit_api::Image, Option<String>)> {
    match &post.images {
        Some(imgs) if !imgs.is_empty() => imgs.iter().filter_map(|i| i.image.clone().map(|img| (img, i.caption.clone()))).collect(),
        _ => post.image.clone().into_iter().map(|i| (i, None)).collect(),
    }
}

/// Which image each post's carousel is showing, shared by feed cards and the post page.
#[derive(Default)]
struct Carousels(std::collections::HashMap<String, usize>);

impl gpui::Global for Carousels {}

fn set_slide(post_id: &str, ix: usize, cx: &mut App) {
    cx.default_global::<Carousels>().0.insert(post_id.to_string(), ix);
    cx.refresh_windows();
}

/// An image carousel: one image at a time with arrows, a counter, dots and the caption.
/// A single image renders without controls. `open_on_click` opens the full image in the browser.
pub fn carousel(key: &str, post_id: &str, images: &[(discuit_api::Image, Option<String>)], max_h: f32, open_on_click: bool, cx: &mut App) -> AnyElement {
    let t = *cx.theme();
    let n = images.len();
    if n == 0 {
        return div().into_any_element();
    }
    let ix = cx.try_global::<Carousels>().and_then(|c| c.0.get(post_id).copied()).unwrap_or(0).min(n - 1);
    let (image, caption) = &images[ix];
    // A fixed height (the tallest slide) so the card doesn't jump while browsing.
    let h = images
        .iter()
        .map(|(img, _)| if img.width > 0 { COLUMN_WIDTH * img.height as f32 / img.width as f32 } else { 360. })
        .fold(0f32, f32::max)
        .clamp(140., max_h);
    let url = image.best_url(COLUMN_WIDTH as u32).to_string();
    let ph = ui::placeholder_color(image, cx);
    let full = crate::state::AppState::client(cx).absolute_url(&image.url).map(|u| u.to_string()).unwrap_or_default();

    let arrow = |dir: i32| {
        let target = if dir < 0 { (ix + n - 1) % n } else { (ix + 1) % n };
        let id = post_id.to_string();
        div()
            .id(SharedString::from(format!("{key}-{}", if dir < 0 { "prev" } else { "next" })))
            .absolute()
            .top(px(h / 2. - 18.))
            .when(dir < 0, |d| d.left(px(10.)))
            .when(dir > 0, |d| d.right(px(10.)))
            .size(px(36.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.55))
            .hover(|s| s.bg(gpui::black().opacity(0.75)))
            .cursor_pointer()
            .child(icons::sized(if dir < 0 { Icon::ChevronLeft } else { Icon::ChevronRight }, px(20.), gpui::white()))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                set_slide(&id, target, cx);
            })
            .into_any_element()
    };

    let mut frame = div()
        .id(SharedString::from(format!("{key}-frame")))
        .relative()
        .w_full()
        .h(px(h))
        .rounded(t.metrics.radius)
        .bg(t.colors.skeleton)
        .child(ui::remote_image(&url, ObjectFit::Contain, t.metrics.radius, ph, cx))
        .when(open_on_click, |d| d.cursor_pointer().on_click(move |_, _, cx| cx.open_url(&full)));
    if n > 1 {
        let prev = arrow(-1);
        let next = arrow(1);
        frame = frame
            .child(prev)
            .child(next)
            .child(
                div()
                    .absolute()
                    .top(px(10.))
                    .right(px(10.))
                    .px(px(9.))
                    .py(px(2.))
                    .rounded(px(10.))
                    .bg(gpui::black().opacity(0.6))
                    .text_color(gpui::white())
                    .text_size(t.fs.xs)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{} / {n}", ix + 1)),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(10.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .gap(px(6.))
                    .children((0..n).map(|i| {
                        let id = post_id.to_string();
                        div()
                            .id(SharedString::from(format!("{key}-dot-{i}")))
                            .size(px(8.))
                            .rounded_full()
                            .cursor_pointer()
                            .bg(if i == ix { gpui::white() } else { gpui::white().opacity(0.45) })
                            .border_1()
                            .border_color(gpui::black().opacity(0.3))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                set_slide(&id, i, cx);
                            })
                    })),
            );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(frame)
        .when_some(caption.clone().or(image.alt_text.clone()).filter(|c| !c.trim().is_empty()), |d, c| {
            d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(c))
        })
        .into_any_element()
}

/// Whether the viewer has embeds enabled (Settings → "Disable embeds").
pub fn embeds_enabled(cx: &App) -> bool {
    !crate::session::Session::get(cx).user().is_some_and(|u| u.embeds_off)
}

/// An embed for a link: a video thumbnail with a play button (opens the
/// player window), or an inline image. `None` if the link isn't embeddable.
pub fn embed_view(key: &str, link: &str, title: &str, fallback_thumb: Option<&discuit_api::Image>, cx: &mut App) -> Option<AnyElement> {
    if !embeds_enabled(cx) {
        return None;
    }
    let t = *cx.theme();
    match crate::embed::detect(link)? {
        crate::embed::Embed::Video { provider, player, thumbnail } => {
            let thumb_url = thumbnail.or_else(|| fallback_thumb.map(|i| i.best_url(COLUMN_WIDTH as u32).to_string()));
            let ph = fallback_thumb.map(|i| ui::placeholder_color(i, cx)).unwrap_or(gpui::black());
            let image = match &thumb_url {
                Some(u) => ui::remote_image(u, ObjectFit::Cover, t.metrics.radius, ph, cx),
                None => div().size_full().rounded(t.metrics.radius).bg(gpui::black()).into_any_element(),
            };
            let title = title.to_string();
            let hover_bg = t.colors.brand;
            Some({
                let mut frame = div();
                frame.style().aspect_ratio = Some(16. / 9.);
                frame
                    .id(SharedString::from(format!("{key}-embed")))
                    .group(SharedString::from(format!("{key}-embed-g")))
                    .relative()
                    .w_full()
                    // 16:9 from the actual width, so thumbnails fill it without letterboxing.
                    .max_h(px(560.))
                    .rounded(t.metrics.radius)
                    .overflow_hidden()
                    .cursor_pointer()
                    .child(image)
                    .child(div().absolute().inset_0().bg(gpui::black().opacity(0.25)))
                    .child(
                        div().absolute().inset_0().flex().items_center().justify_center().child(
                            div()
                                .size(px(68.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(gpui::black().opacity(0.65))
                                .group_hover(SharedString::from(format!("{key}-embed-g")), move |s| s.bg(hover_bg))
                                .child(icons::sized(Icon::Play, px(30.), gpui::white())),
                        ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .bottom(px(10.))
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(4.))
                            .bg(gpui::black().opacity(0.7))
                            .text_color(gpui::white())
                            .text_size(t.fs.xs)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(provider),
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        if crate::storage::load().videos_in_browser {
                            cx.open_url(player.url());
                        } else {
                            crate::player::open(player.clone(), title.clone(), cx);
                        }
                    })
                    .into_any_element()
            })
        }
        crate::embed::Embed::Image { url } => {
            let open = url.clone();
            Some(
                div()
                    .id(SharedString::from(format!("{key}-embed")))
                    .w_full()
                    .h(px(420.))
                    .rounded(t.metrics.radius)
                    .bg(t.colors.skeleton)
                    .cursor_pointer()
                    .child(ui::remote_image(&url, ObjectFit::Contain, t.metrics.radius, t.colors.skeleton, cx))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        cx.open_url(&open);
                    })
                    .into_any_element(),
            )
        }
    }
}
