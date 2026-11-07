//! A user profile: details, badges, moderated communities, and their posts and comments.

use crate::{
    feed::{FeedView, Source},
    icons::Icon,
    markdown,
    modals::{self, Field, FormSpec},
    nav::{self, MenuItem, Target},
    session::Session,
    state::{api, api_task, api_view},
    ui,
};
use discuit_api::User;
use gpui::{Context, Entity, FontWeight, SharedString, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

pub struct UserView {
    username: String,
    feed: Option<Entity<FeedView>>,
    error: Option<SharedString>,
    _task: Option<Task<()>>,
}

impl UserView {
    pub fn new(username: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self { username, feed: None, error: None, _task: None };
        this.refresh(cx);
        this
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let name = self.username.clone();
        self.error = None;
        self._task = Some(api_view(
            cx,
            move |c| c.user(&name),
            |this, r, cx| match r {
                Ok(user) => {
                    let source = Source::User(user.username.clone());
                    let header = cx.new(|_| UserHeader { user: user.clone() });
                    let aside = cx.new(|_| UserAside { user, header });
                    this.feed = Some(cx.new(|cx| FeedView::new(source, cx).with_aside(aside).aside_on_top_when_narrow()));
                }
                Err(e) => this.error = Some(e.user_message().into()),
            },
        ));
        cx.notify();
    }
}

impl Render for UserView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match (&self.feed, &self.error) {
            (Some(feed), _) => div().size_full().child(feed.clone()),
            (None, Some(e)) => div().size_full().child(ui::notice(format!("Couldn't load @{}: {e}", self.username), cx)),
            (None, None) => div().size_full().child(ui::notice("Loading…", cx)),
        }
    }
}

pub struct UserHeader {
    user: User,
}

impl UserHeader {
    fn menu(&self, cx: &gpui::App) -> Vec<MenuItem> {
        let u = self.user.clone();
        let session = Session::get(cx);
        let is_self = session.user().is_some_and(|me| me.id == u.id);
        let mut items = Vec::new();
        let web = crate::state::AppState::client(cx).web_url(&format!("/@{}", u.username));
        items.push(MenuItem::new("Open in browser", move |_, cx| cx.open_url(&web)));
        let name = u.username.clone();
        items.push(MenuItem::new("Public lists", move |_, cx| nav::go(Target::Lists(name.clone()), cx)));
        if session.is_logged_in() && !is_self {
            let muted = session.mutes().is_some_and(|m| m.is_user_muted(&u.id));
            let (id, name) = (u.id.clone(), u.username.clone());
            if muted {
                items.push(MenuItem::new(format!("Unmute @{name}"), move |_, cx| {
                    let id = id.clone();
                    api(
                        cx,
                        move |c| c.unmute_user(&id).and_then(|_| c.mutes()),
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
                items.push(MenuItem::new(format!("Mute @{name}"), move |_, cx| crate::actions::mute_user(id.clone(), name.clone(), cx)));
            }
        }
        if session.is_admin() && !is_self {
            let name = u.username.clone();
            if u.is_banned {
                items.push(MenuItem::new("Unban site-wide (admin)", move |_, cx| {
                    let n = name.clone();
                    modals::confirm(
                        format!("Unban @{n}?"),
                        "They will be able to log in and post again.",
                        "Unban",
                        move |cx| {
                            let n = n.clone();
                            api(
                                cx,
                                move |c| c.admin_unban_user(&n),
                                |r, cx| nav::toast(r.map(|_| "User unbanned".to_string()).unwrap_or_else(|e| e.user_message()), cx),
                            );
                        },
                        cx,
                    );
                }));
            } else {
                items.push(
                    MenuItem::new("Ban site-wide (admin)", move |_, cx| {
                        let n = name.clone();
                        FormSpec::new(format!("Ban @{n}"), "Ban", move |v, cx| {
                            let days = v.texts[0].trim().parse::<u32>().ok().filter(|d| *d > 0);
                            let n = n.clone();
                            let task = api_task(cx, move |c| c.admin_ban_user(&n, days));
                            cx.spawn(async move |cx| {
                                task.await?;
                                cx.update(|cx| nav::toast("User banned", cx)).ok();
                                Ok(())
                            })
                        })
                        .description("Bans the account from the whole site and logs it out everywhere.")
                        .field(Field::text("Also delete content from the last N days (optional)", "").placeholder("e.g. 7"))
                        .danger()
                        .open(cx);
                    })
                    .danger(),
                );
            }
            let name = u.username.clone();
            items.push(MenuItem::new("Add badge (admin)", move |_, cx| {
                let n = name.clone();
                FormSpec::new("Add badge", "Add", move |v, cx| {
                    let (n, kind) = (n.clone(), v.texts[0].trim().to_string());
                    let task = api_task(cx, move |c| c.add_badge(&n, &kind));
                    cx.spawn(async move |cx| {
                        task.await?;
                        cx.update(|cx| nav::toast("Badge added", cx)).ok();
                        Ok(())
                    })
                })
                .field(Field::text("Badge type", "").placeholder("e.g. supporter"))
                .open(cx);
            }));
            let name = u.username.clone();
            items.push(MenuItem::new("Remove badge (admin)", move |_, cx| {
                let n = name.clone();
                FormSpec::new("Remove badge", "Remove", move |v, cx| {
                    let (n, kind) = (n.clone(), v.texts[0].trim().to_string());
                    let by_type = kind.parse::<i64>().is_err();
                    let task = api_task(cx, move |c| c.remove_badge(&n, &kind, by_type));
                    cx.spawn(async move |cx| {
                        task.await?;
                        cx.update(|cx| nav::toast("Badge removed", cx)).ok();
                        Ok(())
                    })
                })
                .field(Field::text("Badge id or type", ""))
                .danger()
                .open(cx);
            }));
        }
        items
    }
}

impl Render for UserHeader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let u = self.user.clone();
        let is_self = Session::get(cx).user().is_some_and(|me| me.id == u.id);
        let pic = u.pro_pic.as_ref().map(|i| i.best_url(120).to_string());
        let menu = self.menu(cx);
        let stats = [(ui::compact(u.points), "points"), (ui::compact(u.no_posts), "posts"), (ui::compact(u.no_comments), "comments")];
        let label = |text: &'static str, color: gpui::Hsla| div().text_color(color).font_weight(FontWeight::BOLD).text_size(t.fs.xs).child(text);
        ui::card(cx)
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().items_start().justify_between().child(ui::avatar(pic.as_deref(), &u.username, px(64.), cx)).child(ui::menu_button(
                "u-menu",
                move |_| menu.clone(),
                cx,
            )))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(t.fs.xl + px(4.)).font_weight(FontWeight::BOLD).truncate().child(format!("@{}", u.username)))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.))
                            .when(u.is_admin, |d| d.child(label("ADMIN", t.colors.red)))
                            .when(u.is_banned, |d| d.child(label("BANNED", t.colors.red)))
                            .when(u.deleted, |d| d.child(label("DELETED", t.colors.text_light))),
                    ),
            )
            .child(div().flex().children(stats.map(|(n, l)| {
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::BOLD).text_size(t.fs.l).child(n))
                    .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(l))
            })))
            .when_some(u.created_at, |d, at| {
                d.child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(format!("Joined {}", at.format("%b %-d, %Y"))))
            })
            .when_some(u.badges.clone().filter(|b| !b.is_empty()), |d, badges| {
                d.child(div().flex().flex_wrap().gap(px(6.)).children(badges.into_iter().map(|b| {
                    div()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(10.))
                        .bg(t.colors.brand.opacity(0.12))
                        .text_color(t.colors.brand)
                        .text_size(t.fs.xs)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(b.kind)
                })))
            })
            .when_some(u.about_me.clone().filter(|a| !a.trim().is_empty()), |d, text| {
                d.child(div().h(px(1.)).bg(t.colors.separator)).child(div().text_size(t.fs.s).child(markdown::render("u-about", &text, cx)))
            })
            .when_some(u.last_seen_month.clone(), |d, month| {
                d.child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(format!("Last seen {month}")))
            })
            .when(is_self, |d| {
                d.child(ui::button_with_icon("u-edit", Icon::Pencil, "Edit profile", cx).w_full().on_click(|_, _, cx| nav::go(Target::Settings, cx)))
            })
    }
}

/// Side panel on profiles: about text and moderated communities.
pub struct UserAside {
    user: User,
    header: Entity<UserHeader>,
}

impl Render for UserAside {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let u = self.user.clone();
        let mut col = div().flex().flex_col().gap(px(12.)).child(self.header.clone());
        if let Some(mods) = u.modding_list.clone().filter(|m| !m.is_empty()) {
            let hover = t.colors.button;
            let mut sec = crate::community::aside_section(format!("MODERATES ({})", mods.len()), cx);
            for (ix, c) in mods.into_iter().enumerate() {
                let pic = c.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
                let name = c.name.clone();
                sec = sec.child(
                    div()
                        .id(("u-mod", ix))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(4.))
                        .py(px(3.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .text_size(t.fs.s)
                        .child(ui::avatar(pic.as_deref(), &c.name, px(22.), cx))
                        .child(div().flex_1().truncate().child(c.name.clone()))
                        .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(ui::compact(c.no_members)))
                        .on_click(move |_, _, cx| nav::go(Target::Community(name.clone()), cx)),
                );
            }
            col = col.child(sec);
        }
        col
    }
}
