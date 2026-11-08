//! Browse and search communities; create or request new ones.

use crate::{
    actions,
    icons::Icon,
    input::{InputEvent, TextInput},
    modals::{Field, FormSpec},
    nav::{self, Target},
    session::Session,
    state::{api_task, api_view},
    ui,
};
use discuit_api::Community;
use gpui::{Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;
use std::{collections::HashSet, time::Duration};

pub struct CommunitiesView {
    search: Entity<TextInput>,
    set: &'static str,
    items: Option<Vec<Community>>,
    error: Option<SharedString>,
    busy: HashSet<String>,
    generation: u64,
    _sub: Subscription,
    _tasks: Vec<Task<()>>,
}

impl CommunitiesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(cx).icon(Icon::Search).placeholder("Search communities"));
        let sub = cx.subscribe(&search, |this: &mut Self, _, ev: &InputEvent, cx| {
            if *ev == InputEvent::Changed {
                this.reload(true, cx);
            }
        });
        let mut this = Self { search, set: "all", items: None, error: None, busy: HashSet::new(), generation: 0, _sub: sub, _tasks: Vec::new() };
        this.reload(false, cx);
        this
    }

    fn reload(&mut self, debounce: bool, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let q = self.search.read(cx).text().trim().to_string();
        let set = self.set;
        let client = crate::state::AppState::client(cx);
        self._tasks.push(cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor().timer(Duration::from_millis(250)).await;
            }
            if this.update(cx, |this, _| this.generation != generation).unwrap_or(true) {
                return;
            }
            let result =
                cx.background_executor().spawn(async move { client.communities(Some(set), (!q.is_empty()).then_some(q.as_str()), None, Some(100)) }).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(list) => {
                        this.items = Some(list);
                        this.error = None;
                    }
                    Err(e) => this.error = Some(e.user_message().into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn toggle_join(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !actions::require_login(cx) {
            return;
        }
        let Some(c) = self.items.as_ref().and_then(|l| l.get(ix)).cloned() else { return };
        if !self.busy.insert(c.id.clone()) {
            return;
        }
        let leave = c.user_joined == Some(true);
        let id = c.id.clone();
        self._tasks.push(api_view(
            cx,
            move |cl| cl.join_community(&id, leave),
            move |this, r, cx| {
                this.busy.remove(&c.id);
                match r {
                    Ok(updated) => {
                        let mut merged = c.clone();
                        merged.user_joined = updated.user_joined.or(Some(!leave));
                        merged.no_members = updated.no_members;
                        if let Some(x) = this.items.as_mut().and_then(|l| l.iter_mut().find(|x| x.id == merged.id)) {
                            *x = merged.clone();
                        }
                        Session::entity(cx).update(cx, |s, cx| s.community_updated(merged, cx));
                    }
                    Err(e) => nav::toast(e.user_message(), cx),
                }
            },
        ));
        cx.notify();
    }
}

pub fn open_create_community(cx: &mut gpui::App) {
    if !actions::require_login(cx) {
        return;
    }
    FormSpec::new("Create community", "Create", |v, cx| {
        let (name, about) = (v.texts[0].trim().to_string(), v.texts[1].clone());
        if name.is_empty() {
            return Task::ready(Err("Pick a name.".into()));
        }
        let task = api_task(cx, move |c| c.create_community(&name, &about));
        cx.spawn(async move |cx| {
            let c = task.await?;
            cx.update(|cx| {
                Session::entity(cx).update(cx, |s, cx| s.refresh(cx));
                nav::go(Target::Community(c.name), cx);
            })
            .ok();
            Ok(())
        })
    })
    .description("Creating a community requires a minimum number of points.")
    .field(Field::text("Name", "").placeholder("3–21 letters, numbers or _"))
    .field(Field::multiline("About (optional)", "", 4))
    .open(cx);
}

pub fn open_request_community(cx: &mut gpui::App) {
    if !actions::require_login(cx) {
        return;
    }
    FormSpec::new("Request a community", "Send request", |v, cx| {
        let (name, note) = (v.texts[0].trim().to_string(), v.texts[1].clone());
        if name.is_empty() {
            return Task::ready(Err("Enter the community name.".into()));
        }
        let task = api_task(cx, move |c| c.request_community(&name, &note));
        cx.spawn(async move |cx| {
            task.await?;
            cx.update(|cx| nav::toast("Request sent to the admins.", cx)).ok();
            Ok(())
        })
    })
    .description("Ask the admins to create a community.")
    .field(Field::text("Community name", ""))
    .field(Field::multiline("Why should it exist?", "", 4))
    .open(cx);
}

impl Render for CommunitiesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let logged_in = Session::get(cx).is_logged_in();
        let set_tab = |set: &'static str, label: &'static str, this: &Self, cx: &mut Context<Self>| {
            ui::selected(ui::button_clear(SharedString::from(format!("cs-{set}")), cx), this.set == set, cx).child(label).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.set = set;
                    this.items = None;
                    this.reload(false, cx);
                },
            ))
        };
        // As many ~380px columns as fit beside the sidebar.
        let usable = (f32::from(window.viewport_size().width) - 320.).min(crate::layout::PAGE_MAX);
        let cols = ((usable / 380.).floor() as u16).clamp(1, 4);
        let body: gpui::AnyElement = match &self.items {
            None if self.error.is_some() => ui::notice(self.error.clone().unwrap(), cx).into_any_element(),
            None => ui::notice("Loading…", cx).into_any_element(),
            Some(items) if items.is_empty() => ui::notice("No communities found.", cx).into_any_element(),
            Some(items) => {
                let mut grid = div().grid().grid_cols(cols).gap(px(10.));
                for (ix, c) in items.clone().into_iter().enumerate() {
                    let pic = c.pro_pic.as_ref().map(|i| i.best_url(80).to_string());
                    let name = c.name.clone();
                    let joined = c.user_joined == Some(true);
                    let busy = self.busy.contains(&c.id);
                    let about = c.about.clone().filter(|a| !a.trim().is_empty()).map(|a| crate::markdown::plain_preview(&a, 180));
                    grid = grid.child(
                        ui::card(cx)
                            .id(("comm", ix))
                            .h(px(124.))
                            .p(px(12.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.border_color(t.colors.card_border))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(ui::avatar(pic.as_deref(), &c.name, px(38.), cx))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(c.name.clone()))
                                            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(format!(
                                                "{}{}",
                                                ui::plural(c.no_members, "member"),
                                                if c.nsfw { " · NSFW" } else { "" }
                                            ))),
                                    )
                                    .child(
                                        ui::disabled(
                                            if joined {
                                                ui::button_with_icon(("cj", ix), Icon::Check, "Joined", cx)
                                            } else {
                                                ui::button_main_with_icon(("cj", ix), Icon::Plus, "Join", cx)
                                            },
                                            busy,
                                        )
                                        .text_size(t.fs.s)
                                        .px(px(10.))
                                        .py(px(4.))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.toggle_join(ix, cx);
                                        })),
                                    ),
                            )
                            .child(
                                div().text_size(t.fs.s).text_color(t.colors.text_light).line_clamp(3).child(about.unwrap_or_else(|| "No description.".into())),
                            )
                            .on_click(move |_, _, cx| nav::go(Target::Community(name.clone()), cx)),
                    );
                }
                grid.into_any_element()
            }
        };
        // A row whose child grows up to PAGE_MAX, not a centred column with
        // `w_full().max_w()`, which can draw text past the edge (see settings.rs).
        div().id("communities").size_full().overflow_y_scroll().child(
            div().w_full().flex().justify_center().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(crate::layout::PAGE_MAX))
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(div().flex_1().min_w(px(200.)).flex().flex_col().child(self.search.clone()))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(2.))
                                    .child(set_tab("all", "All", self, cx))
                                    .child(set_tab("default", "Default", self, cx))
                                    .when(logged_in, |d| d.child(set_tab("subscribed", "Joined", self, cx))),
                            )
                            .child(ui::button_with_icon("c-request", Icon::Inbox, "Request one", cx).on_click(|_, _, cx| open_request_community(cx)))
                            .child(ui::button_main_with_icon("c-create", Icon::Plus, "Create community", cx).on_click(|_, _, cx| open_create_community(cx))),
                    )
                    .child(body),
            ),
        )
    }
}
