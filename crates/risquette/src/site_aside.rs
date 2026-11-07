//! Side panel for the Home / All feeds: site info, popular communities and links.

use crate::{
    community::{aside_section, join_button},
    icons::Icon,
    nav::{self, Target},
    session::Session,
    state::{AppState, api_view},
    ui,
};
use discuit_api::Community;
use gpui::{Context, FontWeight, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

pub struct SiteAside {
    popular: Vec<Community>,
    _task: Option<Task<()>>,
}

impl SiteAside {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self { popular: Vec::new(), _task: None };
        this._task = Some(api_view(
            cx,
            |c| c.communities(Some("default"), None, None, Some(50)),
            |this, r, _| {
                if let Ok(mut list) = r {
                    list.sort_by_key(|c| std::cmp::Reverse(c.no_members));
                    list.truncate(10);
                    this.popular = list;
                }
            },
        ));
        this
    }
}

impl Render for SiteAside {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let session = Session::get(cx);
        let users = session.initial.as_ref().map_or(0, |i| i.no_users);
        let logged_in = session.is_logged_in();

        let about = aside_section("DISCUIT", cx)
            .child(
                div()
                    .text_size(t.fs.s)
                    .child("A free and open-source community platform. Join communities, share posts and talk with people who share your interests."),
            )
            .when(users > 0, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::BOLD).text_size(t.fs.l).child(ui::compact(users)))
                        .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child("people on Discuit")),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(ui::button_with_icon("sa-explore", Icon::Compass, "Explore", cx).on_click(|_, _, cx| nav::go(Target::Communities, cx)))
                    .when(logged_in, |d| {
                        d.child(
                            ui::button_main_with_icon("sa-new", Icon::Plus, "New post", cx)
                                .on_click(|_, _, cx| nav::go(Target::NewPost { community: None }, cx)),
                        )
                    }),
            );

        let mut popular = aside_section("POPULAR COMMUNITIES", cx);
        if self.popular.is_empty() {
            popular = popular.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child("Loading…"));
        }
        let hover = t.colors.button;
        for (ix, c) in self.popular.clone().into_iter().enumerate() {
            let pic = c.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
            let name = c.name.clone();
            popular = popular.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id(("sa-c", ix))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(4.))
                            .py(px(3.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .child(div().w(px(16.)).text_size(t.fs.xs).text_color(t.colors.text_light).child(format!("{}", ix + 1)))
                            .child(ui::avatar(pic.as_deref(), &c.name, px(24.), cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(div().truncate().text_size(t.fs.s).font_weight(FontWeight::MEDIUM).child(c.name.clone()))
                                    .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(ui::plural(c.no_members, "member"))),
                            )
                            .on_click(move |_, _, cx| nav::go(Target::Community(name.clone()), cx)),
                    )
                    .when(logged_in, |d| {
                        d.child(
                            join_button(
                                if ix < 10 { ["sa-j0", "sa-j1", "sa-j2", "sa-j3", "sa-j4", "sa-j5", "sa-j6", "sa-j7", "sa-j8", "sa-j9"][ix] } else { "sa-jx" },
                                &c,
                                cx,
                            )
                            .text_size(t.fs.xs)
                            .px(px(8.))
                            .py(px(3.)),
                        )
                    }),
            );
        }

        let client = AppState::client(cx);
        let links = [("About", "/about"), ("Guidelines", "/guidelines"), ("Terms", "/terms"), ("Privacy", "/privacy-policy")];
        let footer = div()
            .px(px(4.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().flex().flex_wrap().gap(px(10.)).text_size(t.fs.xs).children(links.into_iter().map(|(label, path)| {
                let url = client.web_url(path);
                div()
                    .id(label)
                    .text_color(t.colors.text_light)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(label)
                    .on_click(move |_, _, cx| cx.open_url(&url))
            })))
            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(concat!(
                "Risquette ",
                env!("CARGO_PKG_VERSION"),
                " · an open-source Discuit client"
            )));

        div().flex().flex_col().gap(px(12.)).child(about).child(popular).child(footer)
    }
}
