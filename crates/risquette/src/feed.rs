//! A paginated feed of posts (and comments, for profiles and lists) with
//! infinite scroll. Used for the home/all feeds, community feeds, custom
//! feeds, user profiles and saved lists. An optional header view scrolls with
//! the feed.

use crate::{
    bus::{self, ContentEvent},
    icons::{self, Icon},
    layout,
    post_card::{self, CardOptions},
    session::{Session, SessionEvent},
    state::api_view,
    storage, ui,
};
use discuit_api::{Client, FeedItem, FeedQuery, FeedSort, FeedType, ListItem, Post};
use gpui::{AnyElement, AnyView, Context, FontWeight, ListAlignment, ListState, SharedString, Subscription, Task, Window, div, list, prelude::*, px};
use risquette_theme::ActiveTheme;
use std::collections::VecDeque;

const PAGE_SIZE: u32 = 20;
/// Start fetching the next page when this many items remain below the viewport.
const PREFETCH: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Site(FeedType),
    Community {
        id: String,
        name: String,
    },
    User(String),
    List(i64),
    /// A custom feed (see [`crate::personal`]): several communities, newest first.
    Custom(u64),
}

impl Source {
    /// Where this feed's own sort is kept when "remember the sort per feed" is on.
    fn sort_key(&self) -> Option<String> {
        match self {
            Source::Site(FeedType::Home) => Some("home".into()),
            Source::Site(FeedType::Moderating) => Some("moderating".into()),
            Source::Site(_) => Some("all".into()),
            Source::Community { id, .. } => Some(format!("community:{id}")),
            _ => None,
        }
    }
}

/// One loaded page: items, their list item ids (lists only), the next cursor,
/// and a custom feed's paging state.
type Page = (Vec<FeedItem>, Vec<i64>, Option<String>, Option<Merge>);

/// Pages through several community feeds at once, newest first, for custom
/// feeds. Each community has a queue of posts (sorted newest first by the
/// server); the next post is the newest at the front of any queue. A queue that
/// runs dry while its community has more posts stops the page, since its next
/// post could be the newest of all, so that the order is exact across pages.
#[derive(Debug, Clone, Default)]
struct Merge {
    lanes: Vec<Lane>,
}

#[derive(Debug, Clone)]
struct Lane {
    community_id: String,
    queue: VecDeque<Post>,
    next: Option<String>,
    done: bool,
}

impl Merge {
    fn new(community_ids: impl IntoIterator<Item = String>) -> Self {
        Merge { lanes: community_ids.into_iter().map(|community_id| Lane { community_id, queue: VecDeque::new(), next: None, done: false }).collect() }
    }

    /// Fetch a page for every community whose queue is running low.
    fn fill(&mut self, c: &Client) -> discuit_api::Result<()> {
        for lane in &mut self.lanes {
            if lane.done || lane.queue.len() >= PAGE_SIZE as usize {
                continue;
            }
            let query = FeedQuery {
                feed: FeedType::Community,
                sort: FeedSort::Latest,
                community_id: Some(lane.community_id.clone()),
                limit: Some(PAGE_SIZE),
                next: lane.next.take(),
            };
            match c.feed(&query) {
                Ok(page) => {
                    // Pinned posts would break the time order; they stay on the community's own page.
                    lane.queue.extend(page.posts.unwrap_or_default().into_iter().filter(|p| !p.is_pinned && !p.is_pinned_site));
                    lane.done = page.next.is_none();
                    lane.next = page.next;
                }
                // A community that's gone (or private) is skipped rather than failing the feed.
                Err(e) if !e.is_network() => {
                    log::warn!("custom feed: community {}: {e}", lane.community_id);
                    lane.done = true;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Up to `n` posts, newest first.
    fn take(&mut self, n: usize) -> Vec<Post> {
        let mut out = Vec::new();
        while out.len() < n {
            if self.lanes.iter().any(|l| l.queue.is_empty() && !l.done) {
                break;
            }
            let Some(lane) = self.lanes.iter_mut().filter(|l| !l.queue.is_empty()).max_by_key(|l| l.queue[0].created_at) else { break };
            out.extend(lane.queue.pop_front());
        }
        out
    }

    fn exhausted(&self) -> bool {
        self.lanes.iter().all(|l| l.done && l.queue.is_empty())
    }
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
    /// Posts newer than anything shown, found by [`FeedView::check_for_new`].
    new_posts: usize,
    /// Paging state of a custom feed (taken while a page loads).
    merge: Option<Merge>,
    _load: Option<Task<()>>,
    _check: Option<Task<()>>,
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
        // The feed's own sort when "remember the sort per feed" is on, else the shared one.
        let stored = storage::load();
        let own = source.sort_key().filter(|_| stored.sort_per_feed).and_then(|k| stored.feed_sorts.get(&k).cloned());
        let sort = own.or(stored.feed_sort).and_then(|s| FeedSort::ALL.into_iter().find(|f| f.as_query() == s)).unwrap_or_default();

        let mut subs = vec![cx.subscribe(&bus::entity(cx), |this, _, ev: &ContentEvent, cx| this.on_content(ev, cx))];
        let mut last_user = Session::get(cx).user().map(|u| u.id.clone());
        subs.push(cx.subscribe(&Session::entity(cx), move |this, session, _: &SessionEvent, cx| {
            let user = session.read(cx).user().map(|u| u.id.clone());
            if user != last_user {
                last_user = user;
                this.refresh(cx);
            }
        }));
        // A page that failed to load (or to load more) tries again once back online.
        subs.push(crate::connection::on_reconnect(cx, |this: &mut Self, cx| {
            if this.error.is_some() {
                this.retry(cx);
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
            new_posts: 0,
            merge: None,
            _load: None,
            _check: None,
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
            Source::Custom(_) => "Custom feed".into(),
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
        let key = self.source.sort_key();
        storage::update(|s| match key.filter(|_| s.sort_per_feed) {
            Some(key) => {
                s.feed_sorts.insert(key, sort.as_query().to_string());
            }
            None => s.feed_sort = Some(sort.as_query().to_string()),
        });
        self.refresh(cx);
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.new_posts = 0;
        self._check = None;
        self.merge = None;
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
        // A custom feed carries its paging state through the load.
        let merge = match &self.source {
            Source::Custom(id) => Some(self.merge.take().unwrap_or_else(|| {
                Merge::new(crate::personal::custom_feed(*id, cx).map(|f| f.communities.into_iter().map(|c| c.id).collect::<Vec<_>>()).unwrap_or_default())
            })),
            _ => None,
        };
        self._load = Some(api_view(
            cx,
            move |c| -> discuit_api::Result<Page> {
                if let Some(mut merge) = merge {
                    merge.fill(c)?;
                    let posts = merge.take(PAGE_SIZE as usize);
                    let more = (!merge.exhausted()).then(|| "more".to_string());
                    return Ok((posts.into_iter().map(|p| FeedItem::Post(Box::new(p))).collect(), Vec::new(), more, Some(merge)));
                }
                let (items, ids, next) = match source {
                    Source::Site(feed) => {
                        let page = c.feed(&FeedQuery { feed, sort, community_id: None, limit: Some(PAGE_SIZE), next })?;
                        (page.posts.unwrap_or_default().into_iter().map(|p| FeedItem::Post(Box::new(p))).collect(), Vec::new(), page.next)
                    }
                    Source::Community { id, .. } => {
                        let page = c.feed(&FeedQuery { feed: FeedType::Community, sort, community_id: Some(id), limit: Some(PAGE_SIZE), next })?;
                        (page.posts.unwrap_or_default().into_iter().map(|p| FeedItem::Post(Box::new(p))).collect(), Vec::new(), page.next)
                    }
                    Source::User(name) => {
                        let page = c.user_feed(&name, None, Some(PAGE_SIZE), next.as_deref())?;
                        (page.items.unwrap_or_default(), Vec::new(), page.next)
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
                        (items, ids, page.next)
                    }
                    // Custom feeds load through `Merge` above.
                    Source::Custom(_) => (Vec::new(), Vec::new(), None),
                };
                Ok((items, ids, next, None))
            },
            move |this, result, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                let footer = this.header_rows() + this.items.len();
                match result {
                    Ok((new, ids, next, merge)) => {
                        this.merge = merge;
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

    /// Look at the feed's first page for posts newer than anything shown, and
    /// offer them with a "new posts" pill (the feed isn't changed under the
    /// reader). Only site and community feeds; called while the feed is on screen.
    pub fn check_for_new(&mut self, cx: &mut Context<Self>) {
        let community_id = match &self.source {
            Source::Site(_) => None,
            Source::Community { id, .. } => Some(id.clone()),
            _ => return,
        };
        if self.loading || self.error.is_some() || self.items.is_empty() || self._check.is_some() {
            return;
        }
        let feed = match &self.source {
            Source::Site(feed) => *feed,
            _ => FeedType::Community,
        };
        let sort = self.effective_sort();
        let generation = self.generation;
        self._check = Some(api_view(
            cx,
            move |c| c.feed(&FeedQuery { feed, sort, community_id, limit: Some(PAGE_SIZE), next: None }),
            move |this, result, cx| {
                this._check = None;
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(page) => {
                        let fresh = page.posts.unwrap_or_default().into_iter().map(|p| FeedItem::Post(Box::new(p))).collect();
                        let fresh: Vec<Post> =
                            this.filter_muted(fresh, cx).into_iter().filter_map(|i| if let FeedItem::Post(p) = i { Some(*p) } else { None }).collect();
                        this.new_posts = count_new(&this.items, &fresh);
                    }
                    Err(e) => log::debug!("checking for new posts: {e}"),
                }
            },
        ));
    }

    /// Show the new posts: reload from the top.
    fn show_new_posts(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        self.list.scroll_to(gpui::ListOffset::default());
    }

    fn filter_muted(&self, items: Vec<FeedItem>, cx: &Context<Self>) -> Vec<FeedItem> {
        // Profiles and lists show everything; feeds hide muted authors/communities
        // and posts matching the keyword filters (Settings → Reading).
        if matches!(self.source, Source::User(_) | Source::List(_)) {
            return items;
        }
        let mutes = Session::get(cx).mutes().cloned();
        let keywords = crate::personal::keyword_filters();
        items
            .into_iter()
            .filter(|i| match i {
                FeedItem::Post(p) => {
                    !(p.is_author_muted
                        || p.is_community_muted
                        || mutes.as_ref().is_some_and(|m| m.is_user_muted(&p.user_id) || m.is_community_muted(&p.community_id))
                        || crate::personal::post_filtered(p, &keywords))
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
                (Source::Custom(_), true) => "No posts yet. Add communities to this feed above.",
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
        let t = *cx.theme();
        let new_posts = self.new_posts;
        let main = div().relative().size_full().child(list(self.list.clone(), cx.processor(Self::render_row)).size_full()).when(new_posts > 0, |d| {
            let label = match new_posts {
                1 => "1 new post".to_string(),
                n if n >= PAGE_SIZE as usize => format!("{n}+ new posts"),
                n => format!("{n} new posts"),
            };
            // Floats over the top of the feed, centred.
            d.child(
                div().absolute().top(px(14.)).left_0().right_0().flex().justify_center().child(
                    div()
                        .id("feed-new-posts")
                        // Keep clicks from reaching the post underneath.
                        .occlude()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(14.))
                        .py(px(7.))
                        .rounded_full()
                        .bg(t.colors.button_main)
                        .hover(|s| s.bg(t.colors.button_main_hover))
                        .text_color(t.colors.button_main_text)
                        .text_size(t.fs.s)
                        .font_weight(FontWeight::SEMIBOLD)
                        .shadow_lg()
                        .cursor_pointer()
                        .child(icons::icon(Icon::ChevronUp, t.colors.button_main_text))
                        .child(label)
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.show_new_posts(cx);
                        })),
                ),
            )
        });
        layout::with_aside(main, self.aside.clone().map(|a| a.into_any_element()), self.aside_on_top, window)
    }
}

/// How many of `fresh` (the feed's first page now) are newer than every post in
/// `shown` and not already there. Pinned posts don't count as the newest shown.
fn count_new(shown: &[FeedItem], fresh: &[Post]) -> usize {
    let posts = || shown.iter().filter_map(|i| if let FeedItem::Post(p) = i { Some(p) } else { None });
    let Some(newest) = posts().filter(|p| !p.is_pinned && !p.is_pinned_site).map(|p| p.created_at).max() else { return 0 };
    let ids: std::collections::HashSet<&str> = posts().map(|p| p.id.as_str()).collect();
    fresh.iter().filter(|p| p.created_at > newest && !ids.contains(p.id.as_str())).count()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, minutes_ago: i64, pinned: bool) -> Post {
        let mut p: Post = serde_json::from_value(serde_json::json!({ "id": id, "publicId": id, "title": id, "createdAt": "2026-01-01T00:00:00Z" })).unwrap();
        p.created_at = chrono::Utc::now() - chrono::Duration::minutes(minutes_ago);
        p.is_pinned_site = pinned;
        p
    }

    fn lane(id: &str, posts: &[(&str, i64)], done: bool) -> Lane {
        Lane { community_id: id.into(), queue: posts.iter().map(|(p, m)| post(p, *m, false)).collect(), next: None, done }
    }

    #[test]
    fn custom_feeds_merge_newest_first() {
        let mut m = Merge { lanes: vec![lane("a", &[("a1", 1), ("a2", 30)], true), lane("b", &[("b1", 5), ("b2", 10), ("b3", 60)], true)] };
        let ids = |posts: Vec<Post>| posts.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(m.take(3)), ["a1", "b1", "b2"]);
        assert_eq!(ids(m.take(10)), ["a2", "b3"]);
        assert!(m.exhausted());
        // A community that has run dry but has more to fetch stops the page:
        // its next post might be newer than anything left in the others.
        let mut m = Merge { lanes: vec![lane("a", &[("a1", 1)], false), lane("b", &[("b1", 5), ("b2", 10)], true)] };
        assert_eq!(ids(m.take(10)), ["a1"]);
        assert!(!m.exhausted());
    }

    #[test]
    fn only_posts_newer_than_the_feed_count() {
        let shown: Vec<FeedItem> = [post("a", 30, false), post("b", 60, false)].into_iter().map(|p| FeedItem::Post(Box::new(p))).collect();
        // Two newer posts, one already shown, one older that only now ranks high.
        let fresh = [post("n1", 5, false), post("n2", 10, false), post("a", 30, false), post("old", 90, false)];
        assert_eq!(count_new(&shown, &fresh), 2);
        // An old pinned post at the top doesn't hide what came after it.
        let mut with_pin = vec![FeedItem::Post(Box::new(post("pin", 1, true)))];
        with_pin.extend(shown.iter().cloned());
        assert_eq!(count_new(&with_pin, &fresh), 2);
        assert_eq!(count_new(&[], &fresh), 0);
    }
}
