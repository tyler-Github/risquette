//! Moderator tools for a community: settings & images, rules, moderators,
//! bans, the report queue, and removed/locked posts.

use crate::{
    icons::Icon,
    input::TextInput,
    modals::{self, Field, FormSpec},
    nav::{self, Target},
    state::{api, api_task, api_view},
    ui,
};
use discuit_api::{Community, CommunityRule, Post, Report, ReportDetails, User, UserGroup};
use gpui::{App, Context, Entity, FontWeight, PathPromptOptions, SharedString, Task, WeakEntity, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Settings,
    Rules,
    Mods,
    Banned,
    Reports,
    Removed,
    Locked,
}

impl Tab {
    const ALL: [Tab; 7] = [Tab::Settings, Tab::Rules, Tab::Mods, Tab::Banned, Tab::Reports, Tab::Removed, Tab::Locked];
    fn icon(self) -> Icon {
        match self {
            Tab::Settings => Icon::Settings,
            Tab::Rules => Icon::List,
            Tab::Mods => Icon::Shield,
            Tab::Banned => Icon::VolumeX,
            Tab::Reports => Icon::Flag,
            Tab::Removed => Icon::Trash,
            Tab::Locked => Icon::Lock,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Settings => "Settings",
            Tab::Rules => "Rules",
            Tab::Mods => "Moderators",
            Tab::Banned => "Banned",
            Tab::Reports => "Reports",
            Tab::Removed => "Removed posts",
            Tab::Locked => "Locked posts",
        }
    }
}

pub struct ModToolsView {
    name: String,
    community: Option<Community>,
    tab: Tab,
    about: Entity<TextInput>,
    nsfw: bool,
    restricted: bool,
    rules: Vec<CommunityRule>,
    mods: Vec<User>,
    banned: Vec<User>,
    reports: Vec<Report>,
    report_details: ReportDetails,
    report_filter: &'static str,
    report_page: u32,
    posts: Vec<Post>,
    posts_total: i64,
    posts_page: u32,
    loading: bool,
    error: Option<SharedString>,
    status: Option<SharedString>,
    _tasks: Vec<Task<()>>,
}

impl ModToolsView {
    pub fn new(name: String, cx: &mut Context<Self>) -> Self {
        let about = cx.new(|cx| TextInput::new(cx).multiline(5).placeholder("About this community (markdown)"));
        let mut this = Self {
            name,
            community: None,
            tab: Tab::Settings,
            about,
            nsfw: false,
            restricted: false,
            rules: Vec::new(),
            mods: Vec::new(),
            banned: Vec::new(),
            reports: Vec::new(),
            report_details: ReportDetails::default(),
            report_filter: "all",
            report_page: 1,
            posts: Vec::new(),
            posts_total: 0,
            posts_page: 1,
            loading: false,
            error: None,
            status: None,
            _tasks: Vec::new(),
        };
        let name = this.name.clone();
        this._tasks.push(api_view(
            cx,
            move |c| c.community_by_name(&name),
            |this, r, cx| match r {
                Ok(c) => {
                    this.set_community(c, cx);
                    this.load_tab(cx);
                }
                Err(e) => this.error = Some(e.user_message().into()),
            },
        ));
        this
    }

    fn set_community(&mut self, c: Community, cx: &mut Context<Self>) {
        self.about.update(cx, |i, cx| i.set_text(c.about.clone().unwrap_or_default(), cx));
        self.nsfw = c.nsfw;
        self.restricted = c.posting_restricted;
        if let Some(r) = c.rules.clone() {
            self.rules = r;
        }
        if let Some(m) = c.mods.clone() {
            self.mods = m;
        }
        self.community = Some(c);
    }

    fn id(&self) -> Option<String> {
        self.community.as_ref().map(|c| c.id.clone())
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.status = None;
        self.load_tab(cx);
        cx.notify();
    }

    fn load_tab(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.id() else { return };
        self.loading = true;
        self.error = None;
        let task = match self.tab {
            Tab::Settings => {
                self.loading = false;
                return;
            }
            Tab::Rules => api_view(
                cx,
                move |c| c.rules(&id),
                |this, r, _| {
                    this.loading = false;
                    match r {
                        Ok(v) => this.rules = v,
                        Err(e) => this.error = Some(e.user_message().into()),
                    }
                },
            ),
            Tab::Mods => api_view(
                cx,
                move |c| c.mods(&id),
                |this, r, _| {
                    this.loading = false;
                    match r {
                        Ok(v) => this.mods = v,
                        Err(e) => this.error = Some(e.user_message().into()),
                    }
                },
            ),
            Tab::Banned => api_view(
                cx,
                move |c| c.banned(&id),
                |this, r, _| {
                    this.loading = false;
                    match r {
                        Ok(v) => this.banned = v,
                        Err(e) => this.error = Some(e.user_message().into()),
                    }
                },
            ),
            Tab::Reports => {
                let (filter, page) = (self.report_filter, self.report_page);
                api_view(
                    cx,
                    move |c| c.reports(&id, filter, page, 20),
                    |this, r, _| {
                        this.loading = false;
                        match r {
                            Ok(p) => {
                                this.reports = p.reports.unwrap_or_default();
                                this.report_details = p.details;
                            }
                            Err(e) => this.error = Some(e.user_message().into()),
                        }
                    },
                )
            }
            Tab::Removed | Tab::Locked => {
                let filter = if self.tab == Tab::Removed { "deleted" } else { "locked" };
                let page = self.posts_page;
                api_view(
                    cx,
                    move |c| c.mod_feed(&id, filter, page, 20),
                    |this, r, _| {
                        this.loading = false;
                        match r {
                            Ok(p) => {
                                this.posts = p.posts.unwrap_or_default();
                                this.posts_total = p.no_posts;
                            }
                            Err(e) => this.error = Some(e.user_message().into()),
                        }
                    },
                )
            }
        };
        self._tasks.push(task);
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.id() else { return };
        let (about, nsfw, restricted) = (self.about.read(cx).text().to_string(), self.nsfw, self.restricted);
        self._tasks.push(api_view(
            cx,
            move |c| c.update_community(&id, nsfw, &about, restricted),
            |this, r, cx| match r {
                Ok(c) => {
                    let mut merged = c;
                    if let Some(old) = &this.community {
                        merged.mods = merged.mods.or(old.mods.clone());
                        merged.rules = merged.rules.or(old.rules.clone());
                    }
                    this.set_community(merged.clone(), cx);
                    crate::bus::emit(crate::bus::ContentEvent::CommunityUpdated(Box::new(merged)), cx);
                    this.status = Some("Saved.".into());
                }
                Err(e) => this.status = Some(e.user_message().into()),
            },
        ));
    }

    fn upload_image(&mut self, banner: bool, cx: &mut Context<Self>) {
        let Some(id) = self.id() else { return };
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(if banner { "Choose a banner" } else { "Choose a community icon" }.into()),
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let Ok(client) = cx.update(|cx| crate::state::AppState::client(cx)) else { return };
            let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
            let result = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(&path).map_err(|e| format!("Couldn't read file: {e}"))?;
                    let r = if banner { client.upload_community_banner(&id, &name, &bytes) } else { client.upload_community_pro_pic(&id, &name, &bytes) };
                    r.map_err(|e| e.user_message())
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(c) => {
                        this.community = Some(c);
                        this.status = Some("Image updated.".into());
                    }
                    Err(e) => this.status = Some(e.into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn remove_image(&mut self, banner: bool, cx: &mut Context<Self>) {
        let Some(id) = self.id() else { return };
        self._tasks.push(api_view(
            cx,
            move |c| if banner { c.delete_community_banner(&id) } else { c.delete_community_pro_pic(&id) },
            |this, r, _| match r {
                Ok(c) => {
                    this.community = Some(c);
                    this.status = Some("Image removed.".into());
                }
                Err(e) => this.status = Some(e.user_message().into()),
            },
        ));
    }

    fn reload_after(entity: WeakEntity<Self>, cx: &mut App) {
        entity.update(cx, |this, cx| this.load_tab(cx)).ok();
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let c = self.community.clone().unwrap_or_default();
        let pic = c.pro_pic.as_ref().map(|i| i.best_url(120).to_string());
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().flex_col().gap(px(6.)).child(ui::field_label("About", cx)).child(self.about.clone()))
            .child(ui::checkbox("mt-nsfw", self.nsfw, "NSFW community", cx).on_click(cx.listener(|this, _, _, cx| {
                this.nsfw = !this.nsfw;
                cx.notify();
            })))
            .child(ui::checkbox("mt-restricted", self.restricted, "Only moderators can post", cx).on_click(cx.listener(|this, _, _, cx| {
                this.restricted = !this.restricted;
                cx.notify();
            })))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(ui::button_main("mt-save", "Save settings", cx).on_click(cx.listener(|this, _, _, cx| this.save_settings(cx)))),
            )
            .child(div().h(px(1.)).bg(t.colors.separator))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::avatar(pic.as_deref(), &c.name, px(48.), cx))
                    .child(ui::button_with_icon("mt-pic", Icon::Upload, "Upload icon", cx).on_click(cx.listener(|this, _, _, cx| this.upload_image(false, cx))))
                    .when(c.pro_pic.is_some(), |d| {
                        d.child(ui::button("mt-rmpic", "Remove icon", cx).on_click(cx.listener(|this, _, _, cx| this.remove_image(false, cx))))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(if c.banner_image.is_some() { "Banner set" } else { "No banner" }))
                    .child(
                        ui::button_with_icon("mt-banner", Icon::Upload, "Upload banner", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.upload_image(true, cx))),
                    )
                    .when(c.banner_image.is_some(), |d| {
                        d.child(ui::button("mt-rmbanner", "Remove banner", cx).on_click(cx.listener(|this, _, _, cx| this.remove_image(true, cx))))
                    }),
            )
    }

    fn render_rules(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let id = self.id().unwrap_or_default();
        let me = cx.entity().downgrade();
        let mut out = div().flex().flex_col().gap(px(8.));
        let (add_id, add_me) = (id.clone(), me.clone());
        out = out.child(div().flex().justify_end().child(ui::button_main_with_icon("mt-addrule", Icon::Plus, "Add rule", cx).on_click(move |_, _, cx| {
            let (id, me) = (add_id.clone(), add_me.clone());
            FormSpec::new("Add rule", "Add", move |v, cx| {
                let (id, rule, desc, me) = (id.clone(), v.texts[0].trim().to_string(), v.texts[1].trim().to_string(), me.clone());
                let task = api_task(cx, move |c| c.add_rule(&id, &rule, (!desc.is_empty()).then_some(desc.as_str())));
                cx.spawn(async move |cx| {
                    task.await?;
                    cx.update(|cx| Self::reload_after(me, cx)).ok();
                    Ok(())
                })
            })
            .field(Field::text("Rule", ""))
            .field(Field::multiline("Description (optional)", "", 3))
            .open(cx);
        })));
        for (ix, r) in self.rules.clone().into_iter().enumerate() {
            let (eid, eme, er) = (id.clone(), me.clone(), r.clone());
            let (did, dme, rid) = (id.clone(), me.clone(), r.id);
            out = out.child(
                ui::card(cx)
                    .p(px(12.))
                    .flex()
                    .gap(px(10.))
                    .child(div().flex_none().text_color(t.colors.text_light).child(format!("{}.", ix + 1)))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(r.rule.clone()))
                            .when_some(r.description.clone(), |d, desc| d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(desc))),
                    )
                    .child(ui::button(("mt-er", ix), "Edit", cx).on_click(move |_, _, cx| {
                        let (id, me, r) = (eid.clone(), eme.clone(), er.clone());
                        FormSpec::new("Edit rule", "Save", move |v, cx| {
                            let (id, me, rid) = (id.clone(), me.clone(), r.id);
                            let (rule, desc) = (v.texts[0].trim().to_string(), v.texts[1].trim().to_string());
                            let z = v.texts[2].trim().parse().unwrap_or(r.z_index);
                            let task = api_task(cx, move |c| c.update_rule(&id, rid, &rule, (!desc.is_empty()).then_some(desc.as_str()), z));
                            cx.spawn(async move |cx| {
                                task.await?;
                                cx.update(|cx| Self::reload_after(me, cx)).ok();
                                Ok(())
                            })
                        })
                        .field(Field::text("Rule", r.rule.clone()))
                        .field(Field::multiline("Description", r.description.clone().unwrap_or_default(), 3))
                        .field(Field::text("Order (z-index)", r.z_index.to_string()))
                        .open(cx);
                    }))
                    .child(ui::button(("mt-dr", ix), "Delete", cx).on_click(move |_, _, cx| {
                        let (id, me) = (did.clone(), dme.clone());
                        modals::confirm(
                            "Delete rule?",
                            "",
                            "Delete",
                            move |cx| {
                                let (id, me) = (id.clone(), me.clone());
                                api(
                                    cx,
                                    move |c| c.delete_rule(&id, rid),
                                    move |r, cx| match r {
                                        Ok(_) => Self::reload_after(me, cx),
                                        Err(e) => nav::toast(e.user_message(), cx),
                                    },
                                );
                            },
                            cx,
                        );
                    })),
            );
        }
        if self.rules.is_empty() && !self.loading {
            out = out.child(ui::notice("No rules yet.", cx));
        }
        out
    }

    fn user_rows(
        &self,
        users: &[User],
        action_label: &'static str,
        on_action: impl Fn(String, &mut App) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let t = *cx.theme();
        let mut out = div().flex().flex_col().gap(px(6.));
        for (ix, u) in users.iter().enumerate() {
            let name = u.username.clone();
            let open = u.username.clone();
            let on_action = on_action.clone();
            let pic = u.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
            out = out.child(
                ui::card(cx)
                    .id(SharedString::from(format!("mt-u-{action_label}-{ix}")))
                    .p(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(ui::avatar(pic.as_deref(), &u.username, px(28.), cx))
                    .child(div().flex_1().child(format!("@{}", u.username)))
                    .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(ui::plural(u.points, "point")))
                    .child(ui::button(SharedString::from(format!("mt-ua-{action_label}-{ix}")), action_label, cx).on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        on_action(name.clone(), cx)
                    }))
                    .on_click(move |_, _, cx| nav::go(Target::User(open.clone()), cx)),
            );
        }
        if users.is_empty() && !self.loading {
            out = out.child(ui::notice("Nobody here.", cx));
        }
        out
    }

    fn render_mods(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let id = self.id().unwrap_or_default();
        let me = cx.entity().downgrade();
        let (add_id, add_me) = (id.clone(), me.clone());
        let rows = self.user_rows(
            &self.mods.clone(),
            "Remove",
            move |name, cx| {
                let (id, me) = (id.clone(), me.clone());
                modals::confirm(
                    format!("Remove @{name} as moderator?"),
                    "",
                    "Remove",
                    move |cx| {
                        let (id, me, name) = (id.clone(), me.clone(), name.clone());
                        api(
                            cx,
                            move |c| c.remove_mod(&id, &name),
                            move |r, cx| match r {
                                Ok(_) => Self::reload_after(me, cx),
                                Err(e) => nav::toast(e.user_message(), cx),
                            },
                        );
                    },
                    cx,
                );
            },
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().justify_end().child(ui::button_main_with_icon("mt-addmod", Icon::Plus, "Add moderator", cx).on_click(move |_, _, cx| {
                let (id, me) = (add_id.clone(), add_me.clone());
                FormSpec::new("Add moderator", "Add", move |v, cx| {
                    let (id, me, name) = (id.clone(), me.clone(), v.texts[0].trim().trim_start_matches('@').to_string());
                    let task = api_task(cx, move |c| c.add_mod(&id, &name));
                    cx.spawn(async move |cx| {
                        task.await?;
                        cx.update(|cx| Self::reload_after(me, cx)).ok();
                        Ok(())
                    })
                })
                .field(Field::text("Username", ""))
                .open(cx);
            })))
            .child(rows)
    }

    fn render_banned(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let id = self.id().unwrap_or_default();
        let me = cx.entity().downgrade();
        let (add_id, add_me) = (id.clone(), me.clone());
        let rows = self.user_rows(
            &self.banned.clone(),
            "Unban",
            move |name, cx| {
                let (id, me) = (id.clone(), me.clone());
                api(
                    cx,
                    move |c| c.unban_from_community(&id, &name),
                    move |r, cx| match r {
                        Ok(_) => Self::reload_after(me, cx),
                        Err(e) => nav::toast(e.user_message(), cx),
                    },
                );
            },
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().justify_end().child(ui::button_danger("mt-ban", "Ban user", cx).on_click(move |_, _, cx| {
                let (id, me) = (add_id.clone(), add_me.clone());
                FormSpec::new("Ban from community", "Ban", move |v, cx| {
                    let (id, me, name) = (id.clone(), me.clone(), v.texts[0].trim().trim_start_matches('@').to_string());
                    let expires = v.texts[1].trim().parse::<i64>().ok().filter(|d| *d > 0).map(|d| chrono::Utc::now() + chrono::Duration::days(d));
                    let task = api_task(cx, move |c| c.ban_from_community(&id, &name, expires));
                    cx.spawn(async move |cx| {
                        task.await?;
                        cx.update(|cx| Self::reload_after(me, cx)).ok();
                        Ok(())
                    })
                })
                .field(Field::text("Username", ""))
                .field(Field::text("Days (blank = permanent)", ""))
                .danger()
                .open(cx);
            })))
            .child(rows)
    }

    fn render_reports(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let id = self.id().unwrap_or_default();
        let me = cx.entity().downgrade();
        let d = &self.report_details;
        let filter_tab = |f: &'static str, label: String, this: &Self, cx: &mut Context<Self>| {
            ui::selected(ui::button_clear(SharedString::from(format!("rf-{f}")), cx), this.report_filter == f, cx).child(label).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.report_filter = f;
                    this.report_page = 1;
                    this.load_tab(cx);
                },
            ))
        };
        let mut out = div().flex().flex_col().gap(px(8.)).child(
            div()
                .flex()
                .gap(px(4.))
                .child(filter_tab("all", format!("All ({})", d.no_reports), self, cx))
                .child(filter_tab("posts", format!("Posts ({})", d.no_post_reports), self, cx))
                .child(filter_tab("comments", format!("Comments ({})", d.no_comment_reports), self, cx)),
        );
        for (ix, r) in self.reports.clone().into_iter().enumerate() {
            let target = r.target.clone().unwrap_or_default();
            let is_post = r.kind.as_deref() == Some("post");
            let text = if is_post { target["title"].as_str().unwrap_or("").to_string() } else { target["body"].as_str().unwrap_or("").to_string() };
            let public_id = if is_post { target["publicId"].as_str().map(str::to_string) } else { target["postPublicId"].as_str().map(str::to_string) };
            let author = target["username"].as_str().unwrap_or("").to_string();
            let target_id = r.target_id.clone();
            let remove_pid = public_id.clone().unwrap_or_else(|| target_id.clone());
            let (dismiss_id, dismiss_me, report_id) = (id.clone(), me.clone(), r.id);
            let (rm_id, rm_me) = (id.clone(), me.clone());
            out = out.child(
                ui::card(cx)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(t.fs.s)
                            .text_color(t.colors.text_light)
                            .child(div().text_color(t.colors.red).font_weight(FontWeight::SEMIBOLD).child(r.reason.clone().unwrap_or_else(|| "Report".into())))
                            .child(format!("· {} by @{author}", if is_post { "post" } else { "comment" }))
                            .when_some(r.created_at, |d, at| d.child(format!("· {}", ui::time_ago(at))))
                            .when(r.dealt_at.is_some(), |d| {
                                d.child(
                                    div()
                                        .text_color(t.colors.green)
                                        .child(format!("· handled{}", r.action_taken.clone().map(|a| format!(" ({a})")).unwrap_or_default())),
                                )
                            }),
                    )
                    .child(div().line_clamp(3).child(crate::markdown::plain_preview(&text, 300)))
                    .when_some(r.description.clone().filter(|d| !d.is_empty()), |d, desc| d.child(div().text_size(t.fs.s).italic().child(desc)))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .when_some(public_id, |d, pid| {
                                d.child(ui::button(("rp-open", ix), "Open", cx).on_click(move |_, _, cx| nav::go(Target::Post { public_id: pid.clone() }, cx)))
                            })
                            .child(ui::button(("rp-dismiss", ix), "Dismiss", cx).on_click(move |_, _, cx| {
                                let (id, me) = (dismiss_id.clone(), dismiss_me.clone());
                                api(
                                    cx,
                                    move |c| c.delete_report(&id, report_id),
                                    move |r, cx| match r {
                                        Ok(_) => Self::reload_after(me, cx),
                                        Err(e) => nav::toast(e.user_message(), cx),
                                    },
                                );
                            }))
                            .child(ui::button_danger(("rp-remove", ix), "Remove content", cx).on_click(move |_, _, cx| {
                                let (id, me, target_id, pid) = (rm_id.clone(), rm_me.clone(), target_id.clone(), remove_pid.clone());
                                modals::confirm(
                                    "Remove this content?",
                                    "It will be removed as a moderator and the report resolved.",
                                    "Remove",
                                    move |cx| {
                                        let (id, me, pid, target_id) = (id.clone(), me.clone(), pid.clone(), target_id.clone());
                                        api(
                                            cx,
                                            move |c| {
                                                if is_post {
                                                    c.delete_post(&pid, UserGroup::Mods, false).map(|_| ())?;
                                                } else {
                                                    c.delete_comment(&pid, &target_id, UserGroup::Mods).map(|_| ())?;
                                                }
                                                c.delete_report(&id, report_id).map(|_| ())
                                            },
                                            move |r, cx| match r {
                                                Ok(()) => Self::reload_after(me, cx),
                                                Err(e) => nav::toast(e.user_message(), cx),
                                            },
                                        );
                                    },
                                    cx,
                                );
                            })),
                    ),
            );
        }
        if self.reports.is_empty() && !self.loading {
            out = out.child(ui::notice("No reports. All clear.", cx));
        }
        out.child(self.pager(self.report_page, self.reports.len() == 20, true, cx))
    }

    fn pager(&self, page: u32, has_more: bool, reports: bool, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .justify_center()
            .gap(px(8.))
            .when(page > 1, |d| {
                d.child(ui::button_with_icon("pg-prev", Icon::ChevronLeft, "Previous", cx).on_click(cx.listener(move |this, _, _, cx| {
                    if reports {
                        this.report_page -= 1
                    } else {
                        this.posts_page -= 1
                    }
                    this.load_tab(cx);
                })))
            })
            .when(has_more, |d| {
                d.child(ui::button_with_icon("pg-next", Icon::ChevronRight, "Next", cx).on_click(cx.listener(move |this, _, _, cx| {
                    if reports {
                        this.report_page += 1
                    } else {
                        this.posts_page += 1
                    }
                    this.load_tab(cx);
                })))
            })
    }

    fn render_posts(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let mut out =
            div().flex().flex_col().gap(px(6.)).child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(format!("{} total", self.posts_total)));
        for (ix, p) in self.posts.clone().into_iter().enumerate() {
            let pid = p.public_id.clone();
            let locked = self.tab == Tab::Locked;
            let unlock = p.clone();
            let me = cx.entity().downgrade();
            out = out.child(
                ui::card(cx)
                    .id(("mt-post", ix))
                    .p(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .child(div().flex_1().flex().flex_col().child(div().font_weight(FontWeight::SEMIBOLD).child(p.title.clone())).child(
                        div().text_size(t.fs.xs).text_color(t.colors.text_light).child(format!(
                            "by @{} · {}{}",
                            p.username,
                            ui::time_ago(p.created_at),
                            p.deleted_as.clone().map(|a| format!(" · removed by {a}")).unwrap_or_default()
                        )),
                    ))
                    .when(locked, |d| {
                        d.child(ui::button(("mt-unlock", ix), "Unlock", cx).on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let (pid, me) = (unlock.public_id.clone(), me.clone());
                            api(
                                cx,
                                move |c| c.post_action(&pid, discuit_api::PostAction::Unlock),
                                move |r, cx| match r {
                                    Ok(_) => Self::reload_after(me, cx),
                                    Err(e) => nav::toast(e.user_message(), cx),
                                },
                            );
                        }))
                    })
                    .on_click(move |_, _, cx| nav::go(Target::Post { public_id: pid.clone() }, cx)),
            );
        }
        if self.posts.is_empty() && !self.loading {
            out = out.child(ui::notice("Nothing here.", cx));
        }
        out.child(self.pager(self.posts_page, (self.posts_page as i64) * 20 < self.posts_total, false, cx))
    }
}

impl Render for ModToolsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let body = if self.community.is_none() {
            ui::notice(self.error.clone().unwrap_or_else(|| "Loading…".into()), cx)
        } else {
            let content = match self.tab {
                Tab::Settings => self.render_settings(cx),
                Tab::Rules => self.render_rules(cx),
                Tab::Mods => self.render_mods(cx),
                Tab::Banned => self.render_banned(cx),
                Tab::Reports => self.render_reports(cx),
                Tab::Removed | Tab::Locked => self.render_posts(cx),
            };
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .when_some(self.status.clone(), |d, s| d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(s)))
                .when_some(self.error.clone(), |d, e| d.child(div().text_size(t.fs.s).text_color(t.colors.red).child(e)))
                .when(self.loading, |d| d.child(ui::notice("Loading…", cx)))
                .child(content)
        };
        let name = self.name.clone();
        div().id("modtools").size_full().overflow_y_scroll().flex().flex_col().items_center().child(
            div()
                .w_full()
                .max_w(px(crate::layout::PAGE_MAX))
                .flex_none()
                .px(px(16.))
                .py(px(12.))
                .flex()
                .items_start()
                .gap(px(16.))
                .child(
                    div()
                        .w(px(210.))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            ui::button_with_icon("mt-view", Icon::Compass, "View community", cx)
                                .w_full()
                                .mb(px(8.))
                                .on_click(move |_, _, cx| nav::go(Target::Community(name.clone()), cx)),
                        )
                        .children(Tab::ALL.map(|tab| {
                            let active = self.tab == tab;
                            let color = if active { t.colors.brand } else { t.colors.text_light };
                            ui::selected(ui::button_clear(SharedString::from(format!("mt-tab-{}", tab.label())), cx), active, cx)
                                .w_full()
                                .when(active, |d| d.text_color(t.colors.brand))
                                .child(crate::icons::icon(tab.icon(), color))
                                .child(tab.label())
                                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
                        })),
                )
                .child(ui::card(cx).flex_1().min_w_0().p(px(16.)).child(body)),
        )
    }
}
