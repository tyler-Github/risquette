//! A community page: a compact header above the feed, and a side panel with
//! about, rules and moderators. The panel is reused on post pages.

use crate::{
    actions,
    bus::{self, ContentEvent},
    feed::{FeedView, Source},
    icons::{self, Icon},
    markdown,
    nav::{self, MenuItem, Target},
    session::Session,
    state::api,
    ui,
};
use discuit_api::Community;
use gpui::{App, Context, Entity, FontWeight, ObjectFit, SharedString, Subscription, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;
use std::collections::HashSet;

pub struct CommunityView {
    name: String,
    feed: Option<Entity<FeedView>>,
    error: Option<SharedString>,
    _task: Option<Task<()>>,
}

impl CommunityView {
    pub fn new(name: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self { name, feed: None, error: None, _task: None };
        this.refresh(cx);
        this
    }

    pub fn title(&self) -> SharedString {
        self.name.clone().into()
    }

    pub fn feed(&self) -> Option<Entity<FeedView>> {
        self.feed.clone()
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let name = self.name.clone();
        self.error = None;
        self._task = Some(crate::state::api_view(
            cx,
            move |c| c.community_by_name(&name),
            |this, r, cx| match r {
                Ok(community) => {
                    this.name = community.name.clone();
                    let source = Source::Community { id: community.id.clone(), name: community.name.clone() };
                    let header = cx.new(|cx| CommunityHeader::new(community.clone(), cx));
                    let aside = cx.new(|cx| CommunityAside::new(community, false, cx));
                    this.feed = Some(cx.new(|cx| FeedView::new(source, cx).with_header(header).with_aside(aside)));
                }
                Err(e) => this.error = Some(e.user_message().into()),
            },
        ));
        cx.notify();
    }
}

impl Render for CommunityView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match (&self.feed, &self.error) {
            (Some(feed), _) => div().size_full().child(feed.clone()),
            (None, Some(e)) => div().size_full().child(ui::notice(format!("Couldn't open {}: {e}", self.name), cx)),
            (None, None) => div().size_full().child(ui::notice("Loading…", cx)),
        }
    }
}

/// Keep a local copy of a community in sync with bus updates (keeping mods/rules).
fn merge_update(local: &mut Community, ev: &ContentEvent) -> bool {
    let ContentEvent::CommunityUpdated(c) = ev else { return false };
    if c.id != local.id {
        return false;
    }
    let (mods, rules) = (local.mods.clone(), local.rules.clone());
    *local = (**c).clone();
    if local.mods.is_none() {
        local.mods = mods;
    }
    if local.rules.is_none() {
        local.rules = rules;
    }
    true
}

/// Communities with a join/leave request in flight (shared by header and side panel).
#[derive(Default)]
struct JoinsInFlight(HashSet<String>);

impl gpui::Global for JoinsInFlight {}

pub fn toggle_join(community: &Community, cx: &mut App) {
    if !actions::require_login(cx) {
        return;
    }
    if !cx.default_global::<JoinsInFlight>().0.insert(community.id.clone()) {
        return;
    }
    let old = community.clone();
    let leave = old.user_joined == Some(true);
    let id = old.id.clone();
    cx.refresh_windows();
    api(
        cx,
        move |c| c.join_community(&id, leave),
        move |r, cx| {
            cx.default_global::<JoinsInFlight>().0.remove(&old.id);
            match r {
                Ok(c) => {
                    let mut updated = old.clone();
                    updated.user_joined = c.user_joined.or(Some(!leave));
                    updated.no_members = c.no_members;
                    Session::entity(cx).update(cx, |s, cx| s.community_updated(updated.clone(), cx));
                    bus::emit(ContentEvent::CommunityUpdated(Box::new(updated)), cx);
                }
                Err(e) => nav::toast(e.user_message(), cx),
            }
            cx.refresh_windows();
        },
    );
}

pub fn join_button(id: &'static str, c: &Community, cx: &App) -> gpui::Stateful<gpui::Div> {
    let busy = cx.try_global::<JoinsInFlight>().is_some_and(|j| j.0.contains(&c.id));
    let joined = c.user_joined == Some(true);
    let el = if joined {
        ui::button_with_icon(id, Icon::Check, if busy { "Leaving…" } else { "Joined" }, cx)
    } else {
        ui::button_main_with_icon(id, Icon::Plus, if busy { "Joining…" } else { "Join" }, cx)
    };
    let c = c.clone();
    ui::disabled(el, busy).on_click(move |_, _, cx| toggle_join(&c, cx))
}

fn community_menu(c: &Community, cx: &App) -> Vec<MenuItem> {
    let session = Session::get(cx);
    let mut items = Vec::new();
    let web = crate::state::AppState::client(cx).web_url(&format!("/{}", c.name));
    items.push(MenuItem::new("Open in browser", move |_, cx| cx.open_url(&web)));
    if session.is_logged_in() {
        let muted = session.mutes().is_some_and(|m| m.is_community_muted(&c.id));
        let (id, name) = (c.id.clone(), c.name.clone());
        if muted {
            items.push(MenuItem::new(format!("Unmute {name}"), move |_, cx| {
                let id = id.clone();
                api(
                    cx,
                    move |cl| cl.unmute_community(&id).and_then(|_| cl.mutes()),
                    |r, cx| match r {
                        Ok(m) => {
                            Session::entity(cx).update(cx, |s, cx| s.set_mutes(m, cx));
                            nav::toast("Unmuted", cx);
                        }
                        Err(e) => nav::toast(e.user_message(), cx),
                    },
                );
            }));
        } else {
            items.push(MenuItem::new(format!("Mute {name}"), move |_, cx| actions::mute_community(id.clone(), name.clone(), cx)));
        }
    }
    if session.is_admin() {
        let name = c.name.clone();
        items.push(MenuItem::new("Make default community (admin)", move |_, cx| {
            let n = name.clone();
            api(
                cx,
                move |cl| cl.admin_set_default_community(&n, true),
                |r, cx| nav::toast(r.map(|_| "Added to defaults".to_string()).unwrap_or_else(|e| e.user_message()), cx),
            );
        }));
        let name = c.name.clone();
        items.push(MenuItem::new("Remove from defaults (admin)", move |_, cx| {
            let n = name.clone();
            api(
                cx,
                move |cl| cl.admin_set_default_community(&n, false),
                |r, cx| nav::toast(r.map(|_| "Removed from defaults".to_string()).unwrap_or_else(|e| e.user_message()), cx),
            );
        }));
    }
    items
}

/// Banner, icon, name and actions — shown at the top of the community feed.
pub struct CommunityHeader {
    community: Community,
    _sub: Subscription,
}

impl CommunityHeader {
    fn new(community: Community, cx: &mut Context<Self>) -> Self {
        let sub = cx.subscribe(&bus::entity(cx), |this, _, ev: &ContentEvent, cx| {
            if merge_update(&mut this.community, ev) {
                cx.notify();
            }
        });
        Self { community, _sub: sub }
    }
}

impl Render for CommunityHeader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let c = self.community.clone();
        let is_mod = Session::get(cx).is_mod_of(&c.id);
        let pic = c.pro_pic.as_ref().map(|i| i.best_url(120).to_string());
        let new_post_name = c.name.clone();
        let mod_name = c.name.clone();
        let menu_c = c.clone();

        let mut card = ui::card(cx).mt(px(12.)).mb(px(12.)).flex().flex_col();
        if let Some(b) = &c.banner_image {
            let url = b.copy("large").or(b.copy("small")).map(|x| x.url.clone()).unwrap_or_else(|| b.url.clone());
            let ph = ui::placeholder_color(b, cx);
            card = card.child(div().w_full().h(px(150.)).child(ui::remote_image(&url, ObjectFit::Cover, t.metrics.radius, ph, cx)));
        }
        card.child(
            div()
                .px(px(14.))
                .py(px(12.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(ui::avatar(pic.as_deref(), &c.name, px(52.), cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_size(t.fs.xxl - px(3.)).font_weight(FontWeight::BOLD).truncate().child(c.name.clone()))
                        .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(format!(
                            "{}{}",
                            ui::plural(c.no_members, "member"),
                            if c.nsfw { " · NSFW" } else { "" }
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .when(is_mod, |d| {
                            d.child(
                                ui::button_with_icon("c-modtools", Icon::Shield, "Mod tools", cx)
                                    .on_click(move |_, _, cx| nav::go(Target::ModTools(mod_name.clone()), cx)),
                            )
                        })
                        .child(
                            ui::button_with_icon("c-newpost", Icon::Plus, "New post", cx)
                                .on_click(move |_, _, cx| nav::go(Target::NewPost { community: Some(new_post_name.clone()) }, cx)),
                        )
                        .child(join_button("c-join", &c, cx))
                        .child(ui::menu_button("c-menu", move |cx| community_menu(&menu_c, cx), cx)),
                ),
        )
    }
}

/// The side panel: about, stats, rules and moderators.
pub struct CommunityAside {
    community: Community,
    /// Show the identity/join block (on post pages, where there's no header).
    standalone: bool,
    show_rules: bool,
    _sub: Subscription,
}

impl CommunityAside {
    pub fn new(community: Community, standalone: bool, cx: &mut Context<Self>) -> Self {
        let sub = cx.subscribe(&bus::entity(cx), |this, _, ev: &ContentEvent, cx| {
            if merge_update(&mut this.community, ev) {
                cx.notify();
            }
        });
        Self { community, standalone, show_rules: true, _sub: sub }
    }
}

pub fn aside_section(title: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let t = cx.theme();
    ui::card(cx)
        .p(px(14.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().text_size(t.fs.xs).font_weight(FontWeight::BOLD).text_color(t.colors.text_light).child(title.into()))
}

impl Render for CommunityAside {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let c = self.community.clone();
        let mut col = div().flex().flex_col().gap(px(12.));

        let mut about = aside_section(if self.standalone { "COMMUNITY" } else { "ABOUT" }, cx);
        if self.standalone {
            let pic = c.pro_pic.as_ref().map(|i| i.best_url(80).to_string());
            let name = c.name.clone();
            about = about.child(
                div()
                    .id("ca-name")
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(ui::avatar(pic.as_deref(), &c.name, px(36.), cx))
                    .child(div().font_weight(FontWeight::BOLD).text_size(t.fs.l).hover(|s| s.underline()).child(c.name.clone()))
                    .on_click(move |_, _, cx| nav::go(Target::Community(name.clone()), cx)),
            );
        }
        if let Some(text) = c.about.clone().filter(|a| !a.trim().is_empty()) {
            about = about.child(div().text_size(t.fs.s).child(markdown::render("ca-about", &text, cx)));
        }
        about = about.child(
            div()
                .flex()
                .gap(px(16.))
                .text_size(t.fs.s)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::BOLD).child(ui::compact(c.no_members)))
                        .child(div().text_color(t.colors.text_light).child("members")),
                )
                .when_some(c.created_at, |d, at| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::BOLD).child(at.format("%b %Y").to_string()))
                            .child(div().text_color(t.colors.text_light).child("created")),
                    )
                }),
        );
        if c.posting_restricted {
            about = about.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(t.fs.xs)
                    .text_color(t.colors.text_light)
                    .child(icons::sized(Icon::Lock, px(12.), t.colors.text_light))
                    .child("Only moderators can post"),
            );
        }
        if self.standalone {
            about = about.child(div().flex().gap(px(6.)).child(join_button("ca-join", &c, cx)));
        }
        col = col.child(about);

        if let Some(rules) = c.rules.clone().filter(|r| !r.is_empty()) {
            let show = self.show_rules;
            let mut sec = ui::card(cx).p(px(14.)).flex().flex_col().gap(px(8.)).child(
                div()
                    .id("ca-rules")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_size(t.fs.xs)
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.colors.text_light)
                    .child(icons::sized(if show { Icon::ChevronDown } else { Icon::ChevronRight }, px(13.), t.colors.text_light))
                    .child(format!("RULES ({})", rules.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_rules = !this.show_rules;
                        cx.notify();
                    })),
            );
            if show {
                for (ix, r) in rules.iter().enumerate() {
                    sec = sec.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(t.fs.s)
                            .child(div().flex_none().w(px(16.)).text_color(t.colors.text_light).child(format!("{}.", ix + 1)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(div().font_weight(FontWeight::SEMIBOLD).child(markdown::render(format!("ca-rule-{ix}"), &r.rule, cx)))
                                    .when_some(r.description.clone().filter(|d| !d.is_empty()), |d, desc| {
                                        d.child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(markdown::render(
                                            format!("ca-rule-d-{ix}"),
                                            &desc,
                                            cx,
                                        )))
                                    }),
                            ),
                    );
                }
            }
            col = col.child(sec);
        }

        if let Some(mods) = c.mods.clone().filter(|m| !m.is_empty()) {
            let hover = t.colors.button;
            let mut sec = aside_section(format!("MODERATORS ({})", mods.len()), cx);
            for (ix, m) in mods.into_iter().enumerate() {
                let pic = m.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
                let u = m.username.clone();
                sec = sec.child(
                    div()
                        .id(("ca-mod", ix))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(4.))
                        .py(px(3.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .text_size(t.fs.s)
                        .child(ui::avatar(pic.as_deref(), &m.username, px(22.), cx))
                        .child(div().truncate().child(m.username.clone()))
                        .on_click(move |_, _, cx| nav::go(Target::User(u.clone()), cx)),
                );
            }
            col = col.child(sec);
        }
        col
    }
}
