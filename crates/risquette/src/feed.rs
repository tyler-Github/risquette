//! A paginated feed of posts (and comments, for profiles and lists) with
//! infinite scroll. Used for the home/all feeds, community feeds, user
//! profiles and saved lists. An optional header view scrolls with the feed.

use crate::{
    bus::{self, ContentEvent},
    icons::{self, Icon},
    layout,
    post_card::{self, CardOptions},
    session::{Session, SessionEvent},
    state::api_view,
    storage, ui,
};
use discuit_api::{FeedItem, FeedQuery, FeedSort, FeedType, ListItem};
use gpui::{AnyElement, AnyView, Context, FontWeight, ListAlignment, ListState, SharedString, Subscription, Task, Window, div, list, prelude::*, px};
use risquette_theme::ActiveTheme;

const PAGE_SIZE: u32 = 20;
/// Start fetching the next page when this many items remain below the viewport.
const PREFETCH: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Site(FeedType),
    Community { id: String, name: String },
    User(String),
    List(i64),
}

pub struct FeedView {
    source: Source,
    title: Option<SharedString>,
    sort: FeedSort,
    items: Vec<FeedItem>,
    /// List item ids (for removal from lists), parallel to `items` when `source` is a list.
    list_item_ids: Vec<i64>,
    next: Option<String>,
    loading: bool,
    exhausted: bool,
    error: Option<SharedString>,
    list: ListState,
    header: Option<AnyView>,
    aside: Option<AnyView>,
    aside_on_top: bool,
    /// Bumped whenever the query changes, so stale responses are dropped.
    generation: u64,
    _load: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl FeedView {
    pub fn new(source: Source, cx: &mut Context<Self>) -> Self {
        let list = ListState::new(1, ListAlignment::Top, px(1000.));
        let weak = cx.entity().downgrade();
        list.set_scroll_handler(move |ev, _window, cx| {
            if ev.visible_range.end + PREFETCH >= ev.count {
                weak.update(cx, |this, cx| this.load_more(cx)).ok();
            }
        });
        let sort = storage::load().feed_sort.and_then(|s| FeedSort::ALL.into_iter().find(|f| f.as_query() == s)).unwrap_or_default();

        let mut subs = vec![cx.subscribe(&bus::entity(cx), |this, _, ev: &ContentEvent, cx| this.on_content(ev, cx))];
        let mut last_user = Session::get(cx).user().map(|u| u.id.clone());
        subs.push(cx.subscribe(&Session::entity(cx), move |this, session, _: &SessionEvent, cx| {
            let user = session.read(cx).user().map(|u| u.id.clone());
            if user != last_user {
                last_user = user;
                this.refresh(cx);
            }
        }));

        let mut this = Self {
            source,
            title: None,
            sort,
            items: Vec::new(),
            list_item_ids: Vec::new(),
            next: None,
            loading: false,
            exhausted: false,
            error: None,
            list,
            header: None,
            aside: None,
            aside_on_top: false,
            generation: 0,
            _load: None,
            _subs: subs,
        };
        this.load_more(cx);
        this
    }

    pub fn with_header(mut self, header: impl Into<AnyView>) -> Self {
        self.header = Some(header.into());
        self.list.reset(self.row_count());
        self
    }

    pub fn with_aside(mut self, aside: impl Into<AnyView>) -> Self {
        self.aside = Some(aside.into());
        self
    }

    /// In narrow windows, show the side panel above the feed instead of hiding it.
    pub fn aside_on_top_when_narrow(mut self) -> Self {
        self.aside_on_top = true;
        self
    }

    pub fn with_title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn title(&self) -> SharedString {
        if let Some(t) = &self.title {
            return t.clone();
        }
        match &self.source {
            Source::Site(FeedType::Home) => "Home".into(),
            Source::Site(FeedType::Moderating) => "Moderating".into(),
            Source::Site(_) => "All".into(),
            Source::Community { name, .. } => name.clone().into(),
            Source::User(u) => format!("@{u}").into(),
            Source::List(_) => "List".into(),
        }
    }

    fn header_rows(&self) -> usize {
        usize::from(self.header.is_some())
    }

    fn row_count(&self) -> usize {
        self.header_rows() + self.items.len() + 1
    }

    fn sortable(&self) -> bool {
        matches!(self.source, Source::Site(_) | Source::Community { .. })
    }

    /// The sort actually requested: community feeds only support some sorts.
    fn effective_sort(&self) -> FeedSort {
        match self.source {
            Source::Community { .. } if !self.sort.works_for_community() => FeedSort::Hot,
            _ => self.sort,
        }
    }

    fn sort_options(&self) -> &'static [FeedSort] {
        match self.source {
            Source::Community { .. } => &FeedSort::COMMUNITY,
            _ => &FeedSort::ALL,
        }
    }

    pub fn set_sort(&mut self, sort: FeedSort, cx: &mut Context<Self>) {
        if sort == self.effective_sort() {
            return;
        }
        self.sort = sort;
        storage::update(|s| s.feed_sort = Some(sort.as_query().to_string()));
        self.refresh(cx);
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.items.clear();
        self.list_item_ids.clear();
        self.next = None;
        self.exhausted = false;
        self.loading = false;
        self.error = None;
        self.list.reset(self.row_count());
        self.load_more(cx);
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.exhausted || self.error.is_some() {
            return;
        }
        self.loading = true;
        let generation = self.generation;
        let next = self.next.clone();
        let source = self.source.clone();
        let sort = self.effective_sort();
        self._load = Some(api_view(
            cx,
            move |c| -> discuit_api::Result<(Vec<FeedItem>, Vec<i64>, Option<String>)> {
                match source {
                    Source::Site(feed) => {
                        let page = c.feed(&FeedQuery { feed, sort, community_id: None, limit: Some(PAGE_SIZE), next })?;
                        Ok((page.posts.unwrap_or_default().into_iter().map(|p| FeedItem::Post(Box::new(p))).collect(), Vec::new(), page.next))
                    }
                    Source::Community { id, .. } => {
                        let page = c.feed(&FeedQuery { feed: FeedType::Community, sort, community_id: Some(id), limit: Some(PAGE_SIZE), next })?;
                        Ok((page.posts.unwrap_or_default().into_iter().map(|p| FeedItem::Post(Box::new(p))).collect(), Vec::new(), page.next))
                    }
                    Source::User(name) => {
                        let page = c.user_feed(&name, None, Some(PAGE_SIZE), next.as_deref())?;
                        Ok((page.items.unwrap_or_default(), Vec::new(), page.next))
                    }
                    Source::List(id) => {
                        let page = c.list_items(id, Some(PAGE_SIZE), next.as_deref(), None)?;
                        let (items, ids): (Vec<_>, Vec<_>) = page
                            .items
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|li: ListItem| {
                                let item = match (li.post(), li.comment()) {
                                    (Some(p), _) => FeedItem::Post(Box::new(p)),
                                    (_, Some(c)) => FeedItem::Comment(Box::new(c)),
                                    _ => return None,
                                };
                                Some((item, li.id))
                            })
                            .unzip();
                        Ok((items, ids, page.next))
                    }
                }
            },
            move |this, result, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                let footer = this.header_rows() + this.items.len();
                match result {
                    Ok((new, ids, next)) => {
                        let new = this.filter_muted(new, cx);
                        this.list.splice(footer..footer + 1, new.len() + 1);
                        this.items.extend(new);
                        this.list_item_ids.extend(ids);
                        this.exhausted = next.is_none();
                        this.next = next;
                    }
                    Err(err) => {
                        log::error!("feed: {err}");
                        this.error = Some(err.user_message().into());
                        this.list.splice(footer..footer + 1, 1);
                    }
                }
            },
        ));
        cx.notify();
    }

    fn filter_muted(&self, items: Vec<FeedItem>, cx: &Context<Self>) -> Vec<FeedItem> {
        // Profiles and lists show everything; feeds hide muted authors/communities.
        if matches!(self.source, Source::User(_) | Source::List(_)) {
            return items;
        }
        let mutes = Session::get(cx).mutes().cloned();
        items
            .into_iter()
            .filter(|i| match i {
                FeedItem::Post(p) => {
                    !(p.is_author_muted
                        || p.is_community_muted
                        || mutes.as_ref().is_some_and(|m| m.is_user_muted(&p.user_id) || m.is_community_muted(&p.community_id)))
                }
                FeedItem::Comment(_) => true,
            })
            .collect()
    }

    fn on_content(&mut self, ev: &ContentEvent, cx: &mut Context<Self>) {
        match ev {
            ContentEvent::PostUpdated(p) => {
                for item in &mut self.items {
                    if let FeedItem::Post(old) = item
                        && old.id == p.id
                    {
                        **old = crate::actions::merge_post((**p).clone(), old);
                    }
                }
                cx.notify();
            }
            ContentEvent::PostRemoved(id) => {
                if let Some(ix) = self.items.iter().position(|i| matches!(i, FeedItem::Post(p) if &p.id == id)) {
                    self.items.remove(ix);
                    if ix < self.list_item_ids.len() {
                        self.list_item_ids.remove(ix);
                    }
                    let row = self.header_rows() + ix;
                    self.list.splice(row..row + 1, 0);
                    cx.notify();
                }
            }
            ContentEvent::CommentAdded(c) => {
                for item in &mut self.items {
                    if let FeedItem::Post(p) = item
                        && p.id == c.post_id
                    {
                        p.no_comments += 1;
                    }
                }
                cx.notify();
            }
            ContentEvent::CommentUpdated(c) => {
                for item in &mut self.items {
                    if let FeedItem::Comment(old) = item
                        && old.id == c.id
                    {
                        let author = old.author.clone();
                        **old = (**c).clone();
                        if old.author.is_none() {
                            old.author = author;
                        }
                    }
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        self.load_more(cx);
    }

    fn remove_from_list(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Source::List(list_id) = self.source else { return };
        let Some(&item_id) = self.list_item_ids.get(ix) else { return };
        self._load = Some(api_view(
            cx,
            move |c| c.delete_list_item(list_id, item_id),
            move |this, r, cx| match r {
                Ok(_) => {
                    if let Some(pos) = this.list_item_ids.iter().position(|&i| i == item_id) {
                        this.items.remove(pos);
                        this.list_item_ids.remove(pos);
                        let row = this.header_rows() + pos;
                        this.list.splice(row..row + 1, 0);
                    }
                }
                Err(e) => crate::nav::toast(e.user_message(), cx),
            },
        ));
    }

    fn render_row(&mut self, ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let gap = cx.theme().metrics.gap;
        if self.header.is_some() && ix == 0 {
            return div().w_full().child(self.header.clone().unwrap()).into_any_element();
        }
        let item_ix = ix - self.header_rows();
        // Without a header, pad the first card so it lines up with the side panel.
        let first_pad = item_ix == 0 && self.header.is_none();
        let inner = match self.items.get(item_ix).cloned() {
            Some(item) => {
                let key = format!("feed-{item_ix}");
                let card = match &item {
                    FeedItem::Post(p) => post_card::post_card(p, &key, CardOptions { show_community: !matches!(self.source, Source::Community { .. }) }, cx),
                    FeedItem::Comment(c) => post_card::comment_card(c, &key, cx),
                };
                if matches!(self.source, Source::List(_)) && self.is_own_list(cx) {
                    div()
                        .relative()
                        .child(card)
                        .child(div().absolute().top(px(10.)).right(px(10.)).child(
                            ui::button_clear(("unsave", item_ix), cx).text_size(cx.theme().fs.s).child("Remove").on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_from_list(item_ix, cx);
                                },
                            )),
                        ))
                        .into_any_element()
                } else {
                    card
                }
            }
            None => self.render_footer(cx),
        };
        let _ = gap;
        div().w_full().when(first_pad, |d| d.pt(px(12.))).pb(px(12.)).child(inner).into_any_element()
    }

    fn is_own_list(&self, cx: &Context<Self>) -> bool {
        // Lists opened from the user's own menu are editable; the API rejects others anyway.
        Session::get(cx).is_logged_in()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let base = div().py(px(20.)).flex().flex_col().items_center().gap(px(10.)).text_color(t.colors.text_light).text_size(t.fs.normal);
        if let Some(err) = &self.error {
            base.child(format!("Couldn't load: {err}"))
                .child(ui::button("feed-retry", "Retry", cx).on_click(cx.listener(|this, _, _, cx| this.retry(cx))))
                .into_any_element()
        } else if self.exhausted {
            let msg = match (&self.source, self.items.is_empty()) {
                (Source::Site(FeedType::Home), true) => "Your home feed is empty. Join some communities!",
                (_, true) => "Nothing here yet.",
                _ => "You've reached the end.",
            };
            base.child(msg).into_any_element()
        } else {
            let sk = t.colors.skeleton;
            div()
                .flex()
                .flex_col()
                .gap(t.metrics.gap)
                .children((0..if self.items.is_empty() { 4 } else { 1 }).map(|_| {
                    ui::card(cx).p(t.metrics.gap).flex().flex_col().gap(px(10.)).children([
                        div().h(px(14.)).w(px(180.)).rounded(px(3.)).bg(sk),
                        div().h(px(20.)).w(px(480.)).rounded(px(3.)).bg(sk),
                        div().h(px(14.)).w(px(120.)).rounded(px(3.)).bg(sk),
                    ])
                }))
                .into_any_element()
        }
    }
}

impl Render for FeedView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = list(self.list.clone(), cx.processor(Self::render_row)).size_full();
        layout::with_aside(main, self.aside.clone().map(|a| a.into_any_element()), self.aside_on_top, window)
    }
}

fn sort_icon(sort: FeedSort) -> Icon {
    match sort {
        FeedSort::Hot => Icon::Flame,
        FeedSort::Latest => Icon::Clock,
        FeedSort::Activity => Icon::Activity,
        _ => Icon::TrendingUp,
    }
}

/// The sort dropdown for a feed, shown in the toolbar (None for unsortable feeds).
pub fn sort_control(feed: &gpui::Entity<FeedView>, cx: &gpui::App) -> Option<AnyElement> {
    let f = feed.read(cx);
    if !f.sortable() {
        return None;
    }
    let weak = feed.downgrade();
    Some(
        sort_dropdown(
            f.effective_sort(),
            f.sort_options(),
            move |sort, cx| {
                weak.update(cx, |f, cx| f.set_sort(sort, cx)).ok();
            },
            cx,
        )
        .into_any_element(),
    )
}

/// "Hot v" — opens a menu of sort options.
pub fn sort_dropdown(
    current: FeedSort,
    options: &'static [FeedSort],
    on_select: impl Fn(FeedSort, &mut gpui::App) + Clone + 'static,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    ui::button_clear("sort-dropdown", cx)
        .border_1()
        .border_color(t.colors.separator)
        .child(icons::sized(sort_icon(current), px(15.), t.colors.text_light))
        .child(div().text_color(t.colors.text_light).child("Sort:"))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(current.label()))
        .child(icons::sized(Icon::ChevronDown, px(14.), t.colors.text_light))
        .on_click(move |ev, window, cx| {
            let pos = if ev.position() == gpui::Point::default() { window.mouse_position() } else { ev.position() };
            let items = options
                .iter()
                .copied()
                .map(|sort| {
                    let on_select = on_select.clone();
                    crate::nav::MenuItem::new(sort.label(), move |_, cx| on_select(sort, cx)).icon(if sort == current { Icon::Check } else { sort_icon(sort) })
                })
                .collect();
            crate::nav::open_menu(pos, items, cx);
        })
}
