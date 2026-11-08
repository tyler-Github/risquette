//! Site admin dashboard: site settings, users, bans, default communities,
//! community requests, IP blocks, site-wide comments and analytics.

use crate::{
    icons::Icon,
    modals::{self, Field, FormSpec},
    nav::{self, Target},
    state::{api, api_task, api_view},
    ui,
};
use discuit_api::{AnalyticsEvent, Comment, CommunityRequest, IpBlock, SiteSettings, User};
use gpui::{App, Context, FontWeight, SharedString, Task, WeakEntity, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Site,
    Users,
    Requests,
    IpBlocks,
    Comments,
    Analytics,
}

impl Tab {
    const ALL: [Tab; 6] = [Tab::Site, Tab::Users, Tab::Requests, Tab::IpBlocks, Tab::Comments, Tab::Analytics];
    fn icon(self) -> Icon {
        match self {
            Tab::Site => Icon::Settings,
            Tab::Users => Icon::User,
            Tab::Requests => Icon::Inbox,
            Tab::IpBlocks => Icon::Shield,
            Tab::Comments => Icon::MessageSquare,
            Tab::Analytics => Icon::Activity,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Site => "Site",
            Tab::Users => "Users",
            Tab::Requests => "Community requests",
            Tab::IpBlocks => "IP blocks",
            Tab::Comments => "Comments",
            Tab::Analytics => "Analytics",
        }
    }
}

fn cursor(v: &Option<serde_json::Value>) -> Option<String> {
    match v.as_ref()? {
        serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub struct AdminView {
    tab: Tab,
    settings: Option<SiteSettings>,
    users: Vec<User>,
    users_next: Option<String>,
    requests: Vec<CommunityRequest>,
    blocks: Vec<IpBlock>,
    blocks_next: Option<String>,
    comments: Vec<Comment>,
    comments_next: Option<String>,
    events: Vec<AnalyticsEvent>,
    events_next: Option<String>,
    loading: bool,
    error: Option<SharedString>,
    _tasks: Vec<Task<()>>,
}

impl AdminView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            tab: Tab::Site,
            settings: None,
            users: Vec::new(),
            users_next: None,
            requests: Vec::new(),
            blocks: Vec::new(),
            blocks_next: None,
            comments: Vec::new(),
            comments_next: None,
            events: Vec::new(),
            events_next: None,
            loading: false,
            error: None,
            _tasks: Vec::new(),
        };
        this.load(false, cx);
        this
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.load(false, cx);
    }

    /// Load the current tab; `more` appends the next page.
    fn load(&mut self, more: bool, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let task = match self.tab {
            Tab::Site => api_view(
                cx,
                |c| c.site_settings(),
                |this, r, _| {
                    this.loading = false;
                    match r {
                        Ok(s) => this.settings = Some(s),
                        Err(e) => this.error = Some(e.user_message().into()),
                    }
                },
            ),
            Tab::Users => {
                let next = if more { self.users_next.clone() } else { None };
                api_view(
                    cx,
                    move |c| c.admin_users(next.as_deref()),
                    move |this, r, _| {
                        this.loading = false;
                        match r {
                            Ok(p) => {
                                if !more {
                                    this.users.clear();
                                }
                                this.users.extend(p.users.unwrap_or_default());
                                this.users_next = p.next;
                            }
                            Err(e) => this.error = Some(e.user_message().into()),
                        }
                    },
                )
            }
            Tab::Requests => api_view(
                cx,
                |c| c.community_requests(),
                |this, r, _| {
                    this.loading = false;
                    match r {
                        Ok(v) => this.requests = v,
                        Err(e) => this.error = Some(e.user_message().into()),
                    }
                },
            ),
            Tab::IpBlocks => {
                let next = if more { self.blocks_next.clone() } else { None };
                api_view(
                    cx,
                    move |c| c.ip_blocks(Some(50), next.as_deref()),
                    move |this, r, _| {
                        this.loading = false;
                        match r {
                            Ok(p) => {
                                if !more {
                                    this.blocks.clear();
                                }
                                this.blocks.extend(p.blocks.unwrap_or_default());
                                this.blocks_next = cursor(&p.next);
                            }
                            Err(e) => this.error = Some(e.user_message().into()),
                        }
                    },
                )
            }
            Tab::Comments => {
                let next = if more { self.comments_next.clone() } else { None };
                api_view(
                    cx,
                    move |c| c.admin_comments(next.as_deref()),
                    move |this, r, _| {
                        this.loading = false;
                        match r {
                            Ok(p) => {
                                if !more {
                                    this.comments.clear();
                                }
                                this.comments.extend(p.comments.unwrap_or_default());
                                this.comments_next = p.next;
                            }
                            Err(e) => this.error = Some(e.user_message().into()),
                        }
                    },
                )
            }
            Tab::Analytics => {
                let next = if more { self.events_next.clone() } else { None };
                api_view(
                    cx,
                    move |c| c.analytics_bss(Some(100), next.as_deref()),
                    move |this, r, _| {
                        this.loading = false;
                        match r {
                            Ok(p) => {
                                if !more {
                                    this.events.clear();
                                }
                                this.events.extend(p.events.unwrap_or_default());
                                this.events_next = cursor(&p.next);
                            }
                            Err(e) => this.error = Some(e.user_message().into()),
                        }
                    },
                )
            }
        };
        self._tasks.push(task);
        cx.notify();
    }

    fn reload(me: WeakEntity<Self>, cx: &mut App) {
        me.update(cx, |this, cx| this.load(false, cx)).ok();
    }

    fn save_settings(&mut self, settings: SiteSettings, cx: &mut Context<Self>) {
        self._tasks.push(api_view(
            cx,
            move |c| c.update_site_settings(&settings),
            |this, r, cx| match r {
                Ok(s) => {
                    this.settings = Some(s);
                    nav::toast("Site settings saved", cx);
                }
                Err(e) => nav::toast(e.user_message(), cx),
            },
        ));
    }

    fn render_site(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let s = self.settings.clone().unwrap_or_default();
        let (s1, s2) = (s.clone(), s.clone());
        let simple_action = |id: &'static str,
                             label: &'static str,
                             title: &'static str,
                             field: &'static str,
                             f: fn(&discuit_api::Client, &str) -> discuit_api::Result<serde_json::Value>,
                             cx: &mut Context<Self>| {
            ui::button(id, label, cx).on_click(move |_, _, cx| {
                FormSpec::new(title, label, move |v, cx| {
                    let value = v.texts[0].trim().trim_start_matches('@').to_string();
                    let task = api_task(cx, move |c| f(c, &value));
                    cx.spawn(async move |cx| {
                        task.await?;
                        cx.update(|cx| nav::toast("Done", cx)).ok();
                        Ok(())
                    })
                })
                .field(Field::text(field, ""))
                .open(cx);
            })
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(ui::heading("Site settings", cx))
            .child(ui::checkbox("ad-signups", s.signups_disabled, "Disable sign-ups", cx).on_click(cx.listener(move |this, _, _, cx| {
                let mut n = s1.clone();
                n.signups_disabled = !n.signups_disabled;
                this.save_settings(n, cx);
            })))
            .child(ui::checkbox("ad-tor", s.tor_blocked, "Block Tor exit nodes", cx).on_click(cx.listener(move |this, _, _, cx| {
                let mut n = s2.clone();
                n.tor_blocked = !n.tor_blocked;
                this.save_settings(n, cx);
            })))
            .child(div().h(px(1.)).bg(t.colors.separator))
            .child(ui::heading("Actions", cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(ui::button_danger("ad-ban", "Ban user…", cx).on_click(|_, _, cx| {
                        FormSpec::new("Ban user site-wide", "Ban", |v, cx| {
                            let name = v.texts[0].trim().trim_start_matches('@').to_string();
                            let days = v.texts[1].trim().parse::<u32>().ok().filter(|d| *d > 0);
                            let task = api_task(cx, move |c| c.admin_ban_user(&name, days));
                            cx.spawn(async move |cx| {
                                task.await?;
                                cx.update(|cx| nav::toast("User banned", cx)).ok();
                                Ok(())
                            })
                        })
                        .field(Field::text("Username", ""))
                        .field(Field::text("Delete content from the last N days (optional)", ""))
                        .danger()
                        .open(cx);
                    }))
                    .child(simple_action("ad-unban", "Unban user…", "Unban user", "Username", |c, v| c.admin_unban_user(v), cx))
                    .child(simple_action(
                        "ad-adddef",
                        "Add default community…",
                        "Add default community",
                        "Community name",
                        |c, v| c.admin_set_default_community(v, true),
                        cx,
                    ))
                    .child(simple_action(
                        "ad-rmdef",
                        "Remove default community…",
                        "Remove default community",
                        "Community name",
                        |c, v| c.admin_set_default_community(v, false),
                        cx,
                    ))
                    .child(ui::button("ad-userview", "Inspect user…", cx).on_click(|_, _, cx| {
                        FormSpec::new("Inspect user (admin view)", "Inspect", |v, cx| {
                            let name = v.texts[0].trim().trim_start_matches('@').to_string();
                            let task = api_task(cx, move |c| c.user_admin_view(&name));
                            cx.spawn(async move |cx| {
                                let json = task.await?;
                                cx.update(|cx| {
                                    let pretty = serde_json::to_string_pretty(&json).unwrap_or_default();
                                    // Show the result in a read-only form.
                                    cx.defer(move |cx| {
                                        FormSpec::new("User details", "Close", |_, _| Task::ready(Ok(())))
                                            .width(640.)
                                            .field(Field::multiline("JSON", pretty.clone(), 18))
                                            .open(cx);
                                    });
                                })
                                .ok();
                                Ok(())
                            })
                        })
                        .field(Field::text("Username", ""))
                        .open(cx);
                    })),
            )
    }

    fn render_users(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let mut out = div().flex().flex_col().gap(px(4.));
        for (ix, u) in self.users.clone().into_iter().enumerate() {
            let name = u.username.clone();
            out = out.child(
                div()
                    .id(("ad-u", ix))
                    .p(px(8.))
                    .rounded(t.metrics.radius)
                    .flex()
                    .gap(px(10.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.colors.input))
                    .child(div().flex_1().child(format!("@{}", u.username)))
                    .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(u.email.clone().unwrap_or_default()))
                    .child(div().text_size(t.fs.s).child(ui::plural(u.points, "pt")))
                    .when(u.is_admin, |d| d.child(div().text_color(t.colors.red).text_size(t.fs.xs).child("ADMIN")))
                    .when(u.is_banned, |d| d.child(div().text_color(t.colors.red).text_size(t.fs.xs).child("BANNED")))
                    .on_click(move |_, _, cx| nav::go(Target::User(name.clone()), cx)),
            );
        }
        out
    }

    fn render_requests(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let me = cx.entity().downgrade();
        let mut out = div().flex().flex_col().gap(px(6.));
        for (ix, r) in self.requests.clone().into_iter().enumerate() {
            let (id, me1, me2) = (r.id, me.clone(), me.clone());
            out = out.child(
                ui::card(cx)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().flex().gap(px(8.)).child(div().font_weight(FontWeight::SEMIBOLD).child(r.community_name.clone())).child(
                        div().text_size(t.fs.s).text_color(t.colors.text_light).child(format!(
                            "by @{}{}",
                            r.by_user,
                            if r.community_exists { " · already exists" } else { "" }
                        )),
                    ))
                    .when_some(r.note.clone(), |d, n| d.child(div().text_size(t.fs.s).child(n)))
                    .when_some(r.denied_at, |d, _| {
                        d.child(div().text_size(t.fs.s).text_color(t.colors.red).child(format!("Denied: {}", r.denied_note.clone().unwrap_or_default())))
                    })
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(ui::button(("ad-deny", ix), "Deny…", cx).on_click(move |_, _, cx| {
                                let me = me1.clone();
                                FormSpec::new("Deny request", "Deny", move |v, cx| {
                                    let (note, me) = (v.texts[0].clone(), me.clone());
                                    let task = api_task(cx, move |c| c.admin_deny_community_request(id, &note));
                                    cx.spawn(async move |cx| {
                                        task.await?;
                                        cx.update(|cx| Self::reload(me, cx)).ok();
                                        Ok(())
                                    })
                                })
                                .field(Field::multiline("Reason (sent to the requester)", "", 3))
                                .open(cx);
                            }))
                            .child(ui::button(("ad-delreq", ix), "Delete", cx).on_click(move |_, _, cx| {
                                let me = me2.clone();
                                api(
                                    cx,
                                    move |c| c.delete_community_request(id),
                                    move |r, cx| match r {
                                        Ok(()) => Self::reload(me, cx),
                                        Err(e) => nav::toast(e.user_message(), cx),
                                    },
                                );
                            })),
                    ),
            );
        }
        if self.requests.is_empty() && !self.loading {
            out = out.child(ui::notice("No pending requests.", cx));
        }
        out
    }

    fn render_blocks(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let me = cx.entity().downgrade();
        let (me_add, me_all) = (me.clone(), me.clone());
        let mut out = div().flex().flex_col().gap(px(6.)).child(
            div()
                .flex()
                .justify_end()
                .gap(px(6.))
                .child(ui::button_danger("ad-cancelall", "Cancel all blocks", cx).on_click(move |_, _, cx| {
                    let me = me_all.clone();
                    modals::confirm(
                        "Cancel every IP block?",
                        "All blocks currently in effect will be lifted.",
                        "Cancel all",
                        move |cx| {
                            let me = me.clone();
                            api(
                                cx,
                                |c| c.cancel_all_ip_blocks(),
                                move |r, cx| match r {
                                    Ok(_) => Self::reload(me, cx),
                                    Err(e) => nav::toast(e.user_message(), cx),
                                },
                            );
                        },
                        cx,
                    );
                }))
                .child(ui::button_main_with_icon("ad-block", Icon::Plus, "Block IP", cx).on_click(move |_, _, cx| {
                    let me = me_add.clone();
                    FormSpec::new("Block IP address", "Block", move |v, cx| {
                        let (addr, hours, note, me) = (v.texts[0].trim().to_string(), v.texts[1].trim().parse().unwrap_or(0), v.texts[2].clone(), me.clone());
                        let task = api_task(cx, move |c| c.create_ip_block(&addr, hours, &note));
                        cx.spawn(async move |cx| {
                            task.await?;
                            cx.update(|cx| Self::reload(me, cx)).ok();
                            Ok(())
                        })
                    })
                    .field(Field::text("Address or subnet", "").placeholder("203.0.113.5 or 203.0.113.0/24"))
                    .field(Field::text("Expires in hours (0 = permanent)", "0"))
                    .field(Field::text("Note", ""))
                    .danger()
                    .open(cx);
                })),
        );
        for (ix, b) in self.blocks.clone().into_iter().enumerate() {
            let (id, me) = (b.id, me.clone());
            out = out.child(
                ui::card(cx)
                    .p(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{}/{}", b.ip, b.masked_bits)))
                            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(format!(
                                "{}{}{}",
                                if b.in_effect { "in effect" } else { "inactive" },
                                b.expires_at.map(|e| format!(" · expires {}", e.format("%Y-%m-%d %H:%M"))).unwrap_or_else(|| " · permanent".into()),
                                b.note.clone().map(|n| format!(" · {n}")).unwrap_or_default()
                            )))
                            .when_some(b.associated_users.clone().filter(|u| !u.is_empty()), |d, u| {
                                d.child(div().text_size(t.fs.xs).child(format!("Users: {}", u.join(", "))))
                            }),
                    )
                    .when(b.in_effect, |d| {
                        d.child(ui::button(("ad-cancel", ix), "Cancel", cx).on_click(move |_, _, cx| {
                            let me = me.clone();
                            api(
                                cx,
                                move |c| c.cancel_ip_block(id),
                                move |r, cx| match r {
                                    Ok(_) => Self::reload(me, cx),
                                    Err(e) => nav::toast(e.user_message(), cx),
                                },
                            );
                        }))
                    }),
            );
        }
        if self.blocks.is_empty() && !self.loading {
            out = out.child(ui::notice("No IP blocks.", cx));
        }
        out
    }

    fn render_comments(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let mut out = div().flex().flex_col().gap(px(6.));
        for (ix, c) in self.comments.clone().into_iter().enumerate() {
            out = out.child(crate::post_card::comment_card(&c, &format!("adc-{ix}"), cx));
        }
        out
    }

    fn render_analytics(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let mut out = div().flex().flex_col().gap(px(2.));
        for e in self.events.clone() {
            out = out.child(
                div()
                    .flex()
                    .gap(px(10.))
                    .text_size(t.fs.s)
                    .child(
                        div()
                            .w(px(140.))
                            .flex_none()
                            .text_color(t.colors.text_light)
                            .child(e.created_at.map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()),
                    )
                    .child(div().w(px(90.)).flex_none().font_weight(FontWeight::SEMIBOLD).child(e.name.clone()))
                    .child(div().flex_1().truncate().text_color(t.colors.text_light).child(e.payload.clone().unwrap_or_default())),
            );
        }
        out
    }
}

impl Render for AdminView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let content = match self.tab {
            Tab::Site => self.render_site(cx),
            Tab::Users => self.render_users(cx),
            Tab::Requests => self.render_requests(cx),
            Tab::IpBlocks => self.render_blocks(cx),
            Tab::Comments => self.render_comments(cx),
            Tab::Analytics => self.render_analytics(cx),
        };
        let has_more = match self.tab {
            Tab::Users => self.users_next.is_some(),
            Tab::IpBlocks => self.blocks_next.is_some(),
            Tab::Comments => self.comments_next.is_some(),
            Tab::Analytics => self.events_next.is_some(),
            _ => false,
        };
        // A row whose child grows up to PAGE_MAX, not a centred column with
        // `w_full().max_w()`, which can draw text past the edge (see settings.rs).
        div().id("admin").size_full().overflow_y_scroll().child(
            div().w_full().flex().justify_center().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(crate::layout::PAGE_MAX))
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .child(div().w(px(210.)).flex_none().flex().flex_col().gap(px(2.)).children(Tab::ALL.map(|tab| {
                        let active = self.tab == tab;
                        let color = if active { t.colors.brand } else { t.colors.text_light };
                        ui::selected(ui::button_clear(SharedString::from(format!("ad-tab-{}", tab.label())), cx), active, cx)
                            .w_full()
                            .when(active, |d| d.text_color(t.colors.brand))
                            .child(crate::icons::icon(tab.icon(), color))
                            .child(tab.label())
                            .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
                    })))
                    .child(
                        ui::card(cx)
                            .flex_1()
                            .min_w_0()
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .when_some(self.error.clone(), |d, e| d.child(div().text_color(t.colors.red).text_size(t.fs.s).child(e)))
                            .child(content)
                            .when(self.loading, |d| d.child(ui::notice("Loading…", cx)))
                            .when(has_more && !self.loading, |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .justify_center()
                                        .child(ui::button("ad-more", "Load more", cx).on_click(cx.listener(|this, _, _, cx| this.load(true, cx)))),
                                )
                            }),
                    ),
            ),
        )
    }
}
