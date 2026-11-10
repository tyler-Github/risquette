//! Custom feeds: the header at the top of one (its communities, adding and
//! removing them, renaming and deleting the feed), and the dialogs and menu
//! entries that create feeds and add communities to them. The feed itself is a
//! [`crate::feed::FeedView`] with [`crate::feed::Source::Custom`]; the data
//! lives in [`crate::personal`].

use crate::{
    feed::FeedView,
    icons::{self, Icon},
    input::{InputEvent, TextInput},
    modals::{Field, FormSpec},
    nav::{self, MenuItem, Target},
    personal,
    storage::FeedCommunity,
    ui,
};
use discuit_api::Community;
use gpui::{App, Context, Entity, FontWeight, SharedString, Subscription, Task, WeakEntity, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;
use std::time::Duration;

/// Ask for a name and create a feed (with `first` in it, if given), then open it.
pub fn open_new_feed(first: Option<FeedCommunity>, cx: &mut App) {
    let description = match &first {
        Some(c) => format!("A feed of posts from several communities, newest first. {} will be its first community.", c.name),
        None => "A feed of posts from several communities, newest first. You can add communities once it's made.".to_string(),
    };
    FormSpec::new("New custom feed", "Create", move |values, cx| {
        let name = values.texts.first().map(|s| s.trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            return Task::ready(Err("Give the feed a name.".into()));
        }
        let id = personal::create_custom_feed(&name, first.clone().into_iter().collect(), cx);
        nav::go(Target::CustomFeed(id), cx);
        Task::ready(Ok(()))
    })
    .description(description)
    .field(Field::text("Name", "").placeholder("e.g. Tech"))
    .open(cx);
}

pub fn open_rename(id: u64, cx: &mut App) {
    let current = personal::custom_feed(id, cx).map(|f| f.name).unwrap_or_default();
    FormSpec::new("Rename custom feed", "Rename", move |values, cx| {
        let name = values.texts.first().map(|s| s.trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            return Task::ready(Err("Give the feed a name.".into()));
        }
        personal::rename_custom_feed(id, &name, cx);
        Task::ready(Ok(()))
    })
    .field(Field::text("Name", current))
    .open(cx);
}

pub fn confirm_delete(id: u64, cx: &mut App) {
    let name = personal::custom_feed(id, cx).map(|f| f.name).unwrap_or_default();
    crate::modals::confirm(
        "Delete custom feed?",
        format!("\"{name}\" will be removed. The communities in it aren't affected."),
        "Delete",
        move |cx| {
            personal::delete_custom_feed(id, cx);
            nav::go(Target::Home, cx);
        },
        cx,
    );
}

/// Menu entries for adding `community` to custom feeds (or taking it out), and for a new feed with it.
pub fn menu_items(community: FeedCommunity, cx: &App) -> Vec<MenuItem> {
    let mut items: Vec<MenuItem> = personal::custom_feeds(cx)
        .into_iter()
        .map(|feed| {
            let has = personal::feed_has(&feed, &community.id);
            let label = if has { format!("Remove from {}", feed.name) } else { format!("Add to {}", feed.name) };
            let c = community.clone();
            MenuItem::new(label, move |_, cx| personal::toggle_feed_community(feed.id, c.clone(), cx)).icon(Icon::Layers)
        })
        .collect();
    let c = community.clone();
    items.push(MenuItem::new("New custom feed with this community…", move |_, cx| open_new_feed(Some(c.clone()), cx)).icon(Icon::Plus));
    // A line between these and whatever comes before them in the menu.
    items[0] = items.remove(0).separated();
    items
}

/// The top of a custom feed: name, communities and the controls to change them.
pub struct CustomFeedHeader {
    id: u64,
    search: Entity<TextInput>,
    suggestions: Vec<Community>,
    search_gen: u64,
    feed: Option<WeakEntity<FeedView>>,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl CustomFeedHeader {
    pub fn new(id: u64, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(cx).icon(Icon::Search).placeholder("Add a community…"));
        let subs = vec![cx.subscribe(&search, |this: &mut Self, _, ev: &InputEvent, cx| match ev {
            InputEvent::Changed => this.search(cx),
            // Enter adds the first suggestion.
            InputEvent::Submit => {
                if let Some(c) = this.suggestions.first().cloned() {
                    this.add(c, cx);
                }
            }
        })];
        Self { id, search, suggestions: Vec::new(), search_gen: 0, feed: None, _task: None, _subs: subs }
    }

    /// The feed to reload when the communities change.
    pub fn set_feed(&mut self, feed: WeakEntity<FeedView>) {
        self.feed = Some(feed);
    }

    fn reload_feed(&self, cx: &mut Context<Self>) {
        if let Some(feed) = self.feed.as_ref().and_then(|f| f.upgrade()) {
            feed.update(cx, |f, cx| f.refresh(cx));
        }
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        self.search_gen += 1;
        let generation = self.search_gen;
        let q = self.search.read(cx).text().trim().to_string();
        if q.is_empty() {
            self.suggestions.clear();
            cx.notify();
            return;
        }
        let client = crate::state::AppState::client(cx);
        self._task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            if !this.update(cx, |this, _| this.search_gen == generation).unwrap_or(false) {
                return;
            }
            let result = cx.background_executor().spawn(async move { client.communities(Some("all"), Some(&q), None, Some(8)) }).await;
            this.update(cx, |this, cx| {
                if this.search_gen == generation
                    && let Ok(list) = result
                {
                    this.suggestions = list;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn add(&mut self, c: Community, cx: &mut Context<Self>) {
        let already = personal::custom_feed(self.id, cx).is_some_and(|f| personal::feed_has(&f, &c.id));
        if !already {
            personal::toggle_feed_community(self.id, FeedCommunity { id: c.id, name: c.name }, cx);
            self.reload_feed(cx);
        }
        self.search_gen += 1;
        self.suggestions.clear();
        self.search.update(cx, |i, cx| i.clear(cx));
        cx.notify();
    }

    fn remove(&mut self, c: FeedCommunity, cx: &mut Context<Self>) {
        personal::toggle_feed_community(self.id, c, cx);
        self.reload_feed(cx);
        cx.notify();
    }
}

impl Render for CustomFeedHeader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let id = self.id;
        let Some(feed) = personal::custom_feed(id, cx) else {
            return ui::card(cx).p(px(16.)).child("This custom feed was deleted.").into_any_element();
        };
        let chips = feed
            .communities
            .iter()
            .enumerate()
            .map(|(ix, c)| {
                let community = c.clone();
                let name = c.name.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .py(px(3.))
                    .rounded_full()
                    .bg(t.colors.input)
                    .text_size(t.fs.s)
                    .child(
                        div()
                            .id(SharedString::from(format!("cf-open-{ix}")))
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child(name.clone())
                            .on_click(move |_, _, cx| nav::go(Target::Community(name.clone()), cx)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("cf-remove-{ix}")))
                            .p(px(3.))
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|s| s.bg(t.colors.button_hover))
                            .tooltip(ui::tooltip("Remove from this feed"))
                            .child(icons::sized(Icon::X, px(12.), t.colors.text_light))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove(community.clone(), cx))),
                    )
            })
            .collect::<Vec<_>>();
        let suggestions = (!self.suggestions.is_empty()).then(|| {
            ui::card(cx).flex().flex_col().py(px(4.)).children(self.suggestions.iter().cloned().enumerate().map(|(ix, c)| {
                let added = personal::feed_has(&feed, &c.id);
                let pic = c.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
                div()
                    .id(SharedString::from(format!("cf-sugg-{ix}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.colors.button))
                    .child(ui::avatar(pic.as_deref(), &c.name, px(22.), cx))
                    .child(div().flex_1().min_w_0().truncate().child(c.name.clone()))
                    .when(added, |d| d.child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child("Added")))
                    .on_click(cx.listener(move |this, _, _, cx| this.add(c.clone(), cx)))
            }))
        });
        let card = ui::card(cx)
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icons::sized(Icon::Layers, px(22.), t.colors.brand))
                    .child(div().flex_1().min_w_0().truncate().text_size(t.fs.xl).font_weight(FontWeight::BOLD).child(feed.name.clone()))
                    .child(ui::button_with_icon("cf-rename", Icon::Pencil, "Rename", cx).on_click(move |_, _, cx| open_rename(id, cx)))
                    .child(ui::button_with_icon("cf-delete", Icon::Trash, "Delete", cx).on_click(move |_, _, cx| confirm_delete(id, cx))),
            )
            .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(match feed.communities.len() {
                0 => "Add communities to see their posts here, newest first.".to_string(),
                1 => "Posts from 1 community, newest first.".to_string(),
                n => format!("Posts from {n} communities, newest first."),
            }))
            .when(!feed.communities.is_empty(), |d| d.child(div().flex().flex_wrap().gap(px(6.)).children(chips)))
            .child(div().flex().flex_col().gap(px(4.)).child(self.search.clone()).children(suggestions));
        // The same gaps as between the feed's posts: above it, and before the first post.
        div().pt(px(12.)).pb(px(12.)).child(card).into_any_element()
    }
}
