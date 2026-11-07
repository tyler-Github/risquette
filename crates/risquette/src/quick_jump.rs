//! The toolbar's quick-jump box: type to find a community (or `@user`) and
//! open it. Up/Down choose a result, Enter opens it, Esc clears.

use crate::{
    icons::{self, Icon},
    input::{self, InputEvent, TextInput},
    nav::{self, Target},
    state::AppState,
    ui,
};
use discuit_api::Community;
use gpui::{Context, Entity, FontWeight, SharedString, Subscription, Task, Window, deferred, div, prelude::*, px};
use risquette_theme::ActiveTheme;
use std::time::Duration;

#[derive(Clone)]
enum Hit {
    Community(Box<Community>),
    User(String),
}

pub struct QuickJump {
    input: Entity<TextInput>,
    hits: Vec<Hit>,
    selected: usize,
    generation: u64,
    _sub: Subscription,
    _task: Option<Task<()>>,
}

impl QuickJump {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(cx).icon(Icon::Search).placeholder("Jump to a community or @user"));
        let sub = cx.subscribe(&input, |this: &mut Self, _, ev: &InputEvent, cx| match ev {
            InputEvent::Changed => this.search(cx),
            InputEvent::Submit => this.open_selected(cx),
        });
        Self { input, hits: Vec::new(), selected: 0, generation: 0, _sub: sub, _task: None }
    }

    fn query(&self, cx: &Context<Self>) -> String {
        self.input.read(cx).text().trim().to_string()
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let q = self.query(cx);
        self.selected = 0;
        if q.is_empty() {
            self.hits.clear();
            cx.notify();
            return;
        }
        // Offer the user immediately; communities arrive after a short debounce.
        let user = q.trim_start_matches('@').to_string();
        self.hits = if q.starts_with('@') { vec![Hit::User(user.clone())] } else { Vec::new() };
        cx.notify();
        if q.starts_with('@') {
            return;
        }
        let client = AppState::client(cx);
        self._task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            if this.update(cx, |this, _| this.generation != generation).unwrap_or(true) {
                return;
            }
            let result = cx.background_executor().spawn(async move { client.communities(Some("all"), Some(&q), None, Some(8)) }).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let mut hits: Vec<Hit> = result.unwrap_or_default().into_iter().map(|c| Hit::Community(Box::new(c))).collect();
                hits.push(Hit::User(user));
                this.hits = hits;
                this.selected = 0;
                cx.notify();
            })
            .ok();
        }));
    }

    fn open(&mut self, hit: Hit, cx: &mut Context<Self>) {
        let target = match hit {
            Hit::Community(c) => Target::Community(c.name),
            Hit::User(u) => Target::User(u),
        };
        nav::go(target, cx);
        self.generation += 1;
        self.hits.clear();
        self.input.update(cx, |i, cx| i.clear(cx));
        cx.notify();
    }

    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(hit) = self.hits.get(self.selected).cloned() {
            self.open(hit, cx);
        }
    }

    fn up(&mut self, _: &input::Up, _: &mut Window, cx: &mut Context<Self>) {
        self.selected = self.selected.saturating_sub(1);
        cx.notify();
    }

    fn down(&mut self, _: &input::Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected + 1 < self.hits.len() {
            self.selected += 1;
        }
        cx.notify();
    }

    fn clear(&mut self, _: &crate::Back, _: &mut Window, cx: &mut Context<Self>) {
        if self.hits.is_empty() && self.query(cx).is_empty() {
            cx.propagate();
            return;
        }
        self.generation += 1;
        self.hits.clear();
        self.input.update(cx, |i, cx| i.clear(cx));
        cx.notify();
    }
}

impl Render for QuickJump {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let focused = self.input.read(cx).is_focused(window);
        let show = focused && !self.hits.is_empty();
        let mut results = div()
            .id("qj-results")
            .absolute()
            .top(px(40.))
            .left_0()
            .right_0()
            .occlude()
            .py(px(4.))
            .bg(t.colors.card)
            .rounded(t.metrics.radius)
            .border_1()
            .border_color(t.colors.card_border)
            .shadow_lg()
            .flex()
            .flex_col();
        if show {
            for (ix, hit) in self.hits.clone().into_iter().enumerate() {
                let active = ix == self.selected;
                let hover = t.colors.button;
                let (avatar, label, detail): (gpui::AnyElement, SharedString, SharedString) = match &hit {
                    Hit::Community(c) => {
                        let pic = c.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
                        (ui::avatar(pic.as_deref(), &c.name, px(22.), cx), c.name.clone().into(), ui::plural(c.no_members, "member").into())
                    }
                    Hit::User(u) => (
                        div()
                            .size(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icons::sized(Icon::User, px(16.), t.colors.text_light))
                            .into_any_element(),
                        format!("@{u}").into(),
                        "Open profile".into(),
                    ),
                };
                results = results.child(
                    div()
                        .id(("qj-hit", ix))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(7.))
                        .cursor_pointer()
                        .when(active, |d| d.bg(hover))
                        .hover(move |s| s.bg(hover))
                        .child(avatar)
                        .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(label))
                        .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(detail))
                        // Mouse-down, so it fires before the input loses focus.
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _, cx| this.open(hit.clone(), cx))),
                );
            }
        }
        div()
            .key_context("QuickJump")
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::clear))
            .relative()
            .w_full()
            .child(self.input.clone())
            .when(show, |d| d.child(deferred(results).with_priority(1)))
    }
}
