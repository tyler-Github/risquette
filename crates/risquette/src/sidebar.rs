//! The left sidebar: primary navigation, favourite communities (dragged into
//! order), custom feeds, moderated and joined communities (with a filter), and
//! the account card. Collapses to an icon rail. Right-clicking a community
//! offers favourites and custom feeds.

use crate::{
    communities, custom_feed,
    icons::{self, Icon},
    input::{InputEvent, TextInput},
    modals,
    nav::{self, MenuItem, Target},
    personal,
    session::{Session, SessionEvent},
    storage::{self, FeedCommunity},
    ui,
};
use discuit_api::Community;
use gpui::{AnyElement, App, Context, ElementId, Entity, FontWeight, Hsla, MouseButton, SharedString, Subscription, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

pub const EXPANDED_WIDTH: f32 = 252.;
pub const COLLAPSED_WIDTH: f32 = 60.;

pub struct Sidebar {
    current: Option<Target>,
    collapsed: bool,
    /// Collapsed by the user, or forced by a narrow window (set each render).
    rail: bool,
    filter: Entity<TextInput>,
    show_filter: bool,
    show_favourites: bool,
    show_custom_feeds: bool,
    show_moderating: bool,
    show_communities: bool,
    _subs: Vec<Subscription>,
}

/// A favourite being dragged to a new place in the list.
#[derive(Clone)]
struct DraggedFavourite {
    ix: usize,
    name: SharedString,
}

impl Render for DraggedFavourite {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded(px(7.))
            .bg(t.colors.card)
            .border_1()
            .border_color(t.colors.brand)
            .shadow_lg()
            .text_size(t.fs.normal)
            .text_color(t.colors.text)
            .child(self.name.clone())
    }
}

/// The right-click menu for a community: favourites, their order, and custom feeds.
fn community_menu(name: &str, id: Option<&str>, cx: &App) -> Vec<MenuItem> {
    let favourites = personal::favourites(cx);
    let fav_ix = favourites.iter().position(|f| f.eq_ignore_ascii_case(name));
    let toggle_name = name.to_string();
    let mut items = vec![
        MenuItem::new(if fav_ix.is_some() { "Remove from favourites" } else { "Add to favourites" }, move |_, cx| personal::toggle_favourite(&toggle_name, cx))
            .icon(Icon::Star),
    ];
    if let Some(ix) = fav_ix {
        if ix > 0 {
            items.push(MenuItem::new("Move up", move |_, cx| personal::move_favourite(ix, ix - 1, cx)).icon(Icon::ChevronUp));
        }
        if ix + 1 < favourites.len() {
            items.push(MenuItem::new("Move down", move |_, cx| personal::move_favourite(ix, ix + 1, cx)).icon(Icon::ChevronDown));
        }
    }
    if let Some(id) = id {
        items.extend(custom_feed::menu_items(FeedCommunity { id: id.to_string(), name: name.to_string() }, cx));
    }
    items
}

impl Sidebar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextInput::new(cx).icon(Icon::Search).placeholder("Filter communities"));
        let subs = vec![
            cx.subscribe(&Session::entity(cx), |_, _, _: &SessionEvent, cx| cx.notify()),
            cx.observe(&Session::entity(cx), |_, _, cx| cx.notify()),
            cx.subscribe(&filter, |_, _, ev: &InputEvent, cx| {
                if *ev == InputEvent::Changed {
                    cx.notify();
                }
            }),
        ];
        let collapsed = storage::load().sidebar_collapsed;
        Self {
            current: None,
            collapsed,
            rail: collapsed,
            filter,
            show_filter: false,
            show_favourites: true,
            show_custom_feeds: true,
            show_moderating: true,
            show_communities: true,
            _subs: subs,
        }
    }

    pub fn set_current(&mut self, target: Option<Target>, cx: &mut Context<Self>) {
        self.current = target;
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, cx: &mut Context<Self>) {
        self.collapsed = !self.collapsed;
        let collapsed = self.collapsed;
        storage::update(|s| s.sidebar_collapsed = collapsed);
        cx.notify();
    }

    fn is_active(&self, target: &Target) -> bool {
        self.current.as_ref() == Some(target)
    }

    /// One navigation row (icon + label + optional trailing badge).
    fn nav_item(&self, id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>, target: Target, badge: Option<i64>, cx: &App) -> AnyElement {
        let active = self.is_active(&target);
        let label: SharedString = label.into();
        let leading = icons::sized(icon, px(18.), if active { cx.theme().colors.brand } else { cx.theme().colors.text_light });
        self.row(id, leading, label, active, badge, cx).on_click(move |_, _, cx| nav::go(target.clone(), cx)).into_any_element()
    }

    fn row(
        &self,
        id: impl Into<ElementId>,
        leading: impl IntoElement,
        label: SharedString,
        active: bool,
        badge: Option<i64>,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let t = cx.theme();
        let hover = t.colors.button;
        let collapsed = self.rail;
        let mut el = div()
            .id(id)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.))
            .h(px(36.))
            .px(px(if collapsed { 0. } else { 12. }))
            .when(collapsed, |d| d.justify_center())
            .rounded(px(7.))
            .cursor_pointer()
            .text_size(t.fs.normal)
            .text_color(if active { t.colors.brand } else { t.colors.text })
            .when(active, |d| d.bg(t.colors.brand.opacity(0.12)).font_weight(FontWeight::SEMIBOLD))
            .when(!active, move |d| d.hover(move |s| s.bg(hover)))
            .child(leading);
        if active {
            el = el.child(div().absolute().left(px(-10.)).top(px(9.)).w(px(3.)).h(px(20.)).rounded(px(2.)).bg(t.colors.brand));
        }
        if collapsed {
            el = el.tooltip(ui::tooltip(label.clone()));
            if let Some(n) = badge.filter(|n| *n > 0) {
                el = el.child(div().absolute().top(px(4.)).right(px(8.)).size(px(8.)).rounded_full().bg(t.colors.red));
                let _ = n;
            }
        } else {
            el = el.child(div().flex_1().min_w_0().truncate().child(label));
            if let Some(n) = badge.filter(|n| *n > 0) {
                el = el.child(
                    div()
                        .px(px(7.))
                        .py(px(1.))
                        .rounded(px(10.))
                        .bg(t.colors.red)
                        .text_color(gpui::white())
                        .text_size(t.fs.xs)
                        .font_weight(FontWeight::BOLD)
                        .child(if n > 99 { "99+".to_string() } else { n.to_string() }),
                );
            }
        }
        el
    }

    fn community_row(&self, prefix: &str, ix: usize, c: &Community, mod_link: bool, cx: &mut Context<Self>) -> AnyElement {
        let target = Target::Community(c.name.clone());
        let active = self.is_active(&target);
        let pic = c.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
        let avatar = ui::avatar(pic.as_deref(), &c.name, px(22.), cx);
        let mut row = self.row(SharedString::from(format!("{prefix}-{ix}")), avatar, c.name.clone().into(), active, None, cx);
        if mod_link && !self.rail {
            let name = c.name.clone();
            let color = cx.theme().colors.text_light;
            let hover = cx.theme().colors.button_hover;
            row = row.child(
                div()
                    .id(SharedString::from(format!("{prefix}-mt-{ix}")))
                    .p(px(4.))
                    .rounded(px(4.))
                    .hover(move |s| s.bg(hover))
                    .child(icons::sized(Icon::Shield, px(14.), color))
                    .tooltip(ui::tooltip("Mod tools"))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        nav::go(Target::ModTools(name.clone()), cx);
                    }),
            );
        }
        let (name, id) = (c.name.clone(), c.id.clone());
        row.on_click(move |_, _, cx| nav::go(target.clone(), cx))
            .on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                cx.stop_propagation();
                let items = community_menu(&name, Some(&id), cx);
                nav::open_menu(ev.position, items, cx);
            })
            .into_any_element()
    }

    /// A favourite: like a community row, and dragged to reorder.
    fn favourite_row(&self, ix: usize, name: &str, known: Option<&Community>, cx: &mut Context<Self>) -> AnyElement {
        let target = Target::Community(name.to_string());
        let active = self.is_active(&target);
        let pic = known.and_then(|c| c.pro_pic.as_ref()).map(|i| i.best_url(40).to_string());
        let avatar = ui::avatar(pic.as_deref(), name, px(22.), cx);
        let brand = cx.theme().colors.brand;
        let (menu_name, id) = (name.to_string(), known.map(|c| c.id.clone()));
        let group: SharedString = format!("sb-fav-{ix}").into();
        let grip = cx.theme().colors.text_light;
        self.row(group.clone(), avatar, name.to_string().into(), active, None, cx)
            .group(group.clone())
            // A grip that shows on hover, as a hint that favourites can be dragged.
            .when(!self.rail, |d| d.child(div().opacity(0.).group_hover(group, |s| s.opacity(1.)).child(icons::sized(Icon::GripVertical, px(14.), grip))))
            .on_click(move |_, _, cx| nav::go(target.clone(), cx))
            .on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                cx.stop_propagation();
                let items = community_menu(&menu_name, id.as_deref(), cx);
                nav::open_menu(ev.position, items, cx);
            })
            .on_drag(DraggedFavourite { ix, name: name.to_string().into() }, |d, _, _, cx| cx.new(|_| d.clone()))
            .drag_over::<DraggedFavourite>(move |s, _, _, _| s.bg(brand.opacity(0.15)))
            .on_drop(move |d: &DraggedFavourite, _, cx| personal::move_favourite(d.ix, ix, cx))
            .into_any_element()
    }

    /// A custom feed: opens it; right-click to rename or delete.
    fn custom_feed_row(&self, feed: &storage::CustomFeed, cx: &mut Context<Self>) -> AnyElement {
        let target = Target::CustomFeed(feed.id);
        let active = self.is_active(&target);
        let id = feed.id;
        let icon = icons::sized(Icon::Layers, px(18.), if active { cx.theme().colors.brand } else { cx.theme().colors.text_light });
        self.row(SharedString::from(format!("sb-cf-{id}")), icon, feed.name.clone().into(), active, None, cx)
            .on_click(move |_, _, cx| nav::go(target.clone(), cx))
            .on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                cx.stop_propagation();
                let items = vec![
                    MenuItem::new("Rename…", move |_, cx| custom_feed::open_rename(id, cx)).icon(Icon::Pencil),
                    MenuItem::new("Delete…", move |_, cx| custom_feed::confirm_delete(id, cx)).icon(Icon::Trash).danger(),
                ];
                nav::open_menu(ev.position, items, cx);
            })
            .into_any_element()
    }

    fn section_header(&self, id: &'static str, label: &'static str, open: bool, count: usize, toggle: fn(&mut Self), cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        if self.rail {
            return div().h(px(1.)).mx(px(10.)).my(px(8.)).bg(t.colors.separator).into_any_element();
        }
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .pt(px(14.))
            .pb(px(6.))
            .cursor_pointer()
            .text_size(t.fs.xs)
            .font_weight(FontWeight::BOLD)
            .text_color(t.colors.text_light)
            .child(icons::sized(if open { Icon::ChevronDown } else { Icon::ChevronRight }, px(14.), t.colors.text_light))
            .child(div().flex_1().child(label))
            .when(id == "sb-h-comms" && count > 0, |d| {
                let active = self.show_filter;
                let hover = t.colors.button;
                d.child(
                    div()
                        .id("sb-filter-toggle")
                        .p(px(3.))
                        .rounded(px(4.))
                        .when(active, |d| d.bg(hover))
                        .hover(move |s| s.bg(hover))
                        .child(icons::sized(Icon::Search, px(13.), if active { t.colors.text } else { t.colors.text_light }))
                        .tooltip(ui::tooltip("Filter communities"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.show_filter = !this.show_filter;
                            if this.show_filter {
                                this.show_communities = true;
                                this.filter.read(cx).focus(window);
                            } else {
                                this.filter.update(cx, |f, cx| f.clear(cx));
                            }
                            cx.notify();
                        })),
                )
            })
            .child(div().font_weight(FontWeight::NORMAL).child(count.to_string()))
            .on_click(cx.listener(move |this, _, _, cx| {
                toggle(this);
                cx.notify();
            }))
            .into_any_element()
    }

    fn account_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let session = Session::get(cx);
        let Some(user) = session.user().cloned() else {
            // Settings must stay reachable while logged out (theme, instance, updates…).
            let settings = || ui::icon_button("sb-settings", Icon::Settings, "Settings", cx).on_click(|_, _, cx| nav::go(Target::Settings, cx));
            if self.rail {
                return div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(ui::icon_button("sb-login", Icon::LogIn, "Log in", cx).on_click(|_, _, cx| modals::open_login(cx)))
                    .child(settings())
                    .into_any_element();
            }
            return div()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().flex_1().min_w_0().text_size(t.fs.s).text_color(t.colors.text_light).child("Join the conversation on Discuit."))
                        .child(settings()),
                )
                .child(ui::button_main_with_icon("sb-login", Icon::LogIn, "Log in", cx).justify_center().on_click(|_, _, cx| modals::open_login(cx)))
                .child(ui::button("sb-signup", "Create account", cx).justify_center().on_click(|_, _, cx| modals::open_signup(cx)))
                .into_any_element();
        };
        let pic = user.pro_pic.as_ref().map(|i| i.best_url(60).to_string());
        let hover = t.colors.button;
        if self.rail {
            return div()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("sb-me")
                        .p(px(4.))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(ui::avatar(pic.as_deref(), &user.username, px(30.), cx))
                        .tooltip(ui::tooltip(format!("@{}", user.username)))
                        .on_click(open_account_menu),
                )
                .into_any_element();
        }
        div()
            .p(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .id("sb-me")
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(6.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(ui::avatar(pic.as_deref(), &user.username, px(32.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_size(t.fs.normal).child(user.username.clone()))
                            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(ui::plural(user.points, "point"))),
                    )
                    .child(icons::sized(Icon::ChevronUp, px(14.), t.colors.text_light))
                    .tooltip(ui::tooltip("Account"))
                    .on_click(open_account_menu),
            )
            .child(ui::icon_button("sb-settings", Icon::Settings, "Settings", cx).on_click(|_, _, cx| nav::go(Target::Settings, cx)))
            .into_any_element()
    }
}

fn open_account_menu(ev: &gpui::ClickEvent, window: &mut Window, cx: &mut App) {
    let pos = if ev.position() == gpui::Point::default() { window.mouse_position() } else { ev.position() };
    nav::open_menu(pos, crate::root::Root::user_menu(cx), cx);
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rail = self.collapsed || window.viewport_size().width < px(1000.);
        let t = *cx.theme();
        let session = Session::get(cx);
        let logged_in = session.is_logged_in();
        let is_admin = session.is_admin();
        let username = session.username().map(str::to_string);
        let moderating: Vec<Community> = session.user().and_then(|u| u.modding_list.clone()).unwrap_or_default();
        let filter = self.filter.read(cx).text().trim().to_lowercase();
        // Every community we have details for, to show favourites with their pictures.
        let known: Vec<Community> = session.communities().iter().chain(moderating.iter()).cloned().collect();
        let mut communities: Vec<Community> = session.communities().to_vec();
        communities.sort_by_key(|c| c.name.to_lowercase());
        let total = communities.len();
        // Not loaded yet, or it couldn't load (offline): don't claim the list is empty.
        let communities_loaded = session.initial().is_some();
        let communities_failed = session.error.is_some();
        if !filter.is_empty() {
            communities.retain(|c| c.name.to_lowercase().contains(&filter));
        }

        let width = if self.rail { COLLAPSED_WIDTH } else { EXPANDED_WIDTH };
        let mut primary = div().flex().flex_none().flex_col().gap(px(2.));
        if logged_in {
            primary = primary.child(self.nav_item("sb-home", Icon::House, "Home", Target::Home, None, cx));
        }
        primary = primary.child(self.nav_item("sb-all", Icon::Globe, "All", Target::All, None, cx)).child(self.nav_item(
            "sb-comms",
            Icon::Compass,
            "Explore communities",
            Target::Communities,
            None,
            cx,
        ));
        if let Some(u) = &username {
            primary = primary.child(self.nav_item("sb-saved", Icon::Bookmark, "Saved", Target::Lists(u.clone()), None, cx));
        }
        if is_admin {
            primary = primary.child(self.nav_item("sb-admin", Icon::ShieldCheck, "Admin", Target::Admin, None, cx));
        }

        let mut scroll = div().id("sb-scroll").flex_1().min_h_0().overflow_y_scroll().px(px(10.)).py(px(10.)).flex().flex_col().child(primary);

        // Favourites, in the user's order (dragged, or "Move up/down" on right-click).
        let favourites = personal::favourites(cx);
        if !favourites.is_empty() {
            let open = self.show_favourites;
            scroll = scroll.child(self.section_header("sb-h-fav", "FAVOURITES", open, favourites.len(), |s| s.show_favourites = !s.show_favourites, cx));
            if open || self.rail {
                for (ix, name) in favourites.iter().enumerate() {
                    let c = known.iter().find(|c| c.name.eq_ignore_ascii_case(name)).cloned();
                    scroll = scroll.child(self.favourite_row(ix, name, c.as_ref(), cx));
                }
            }
        }

        // Custom feeds, with a way to make one.
        let feeds = personal::custom_feeds(cx);
        let open = self.show_custom_feeds;
        if !feeds.is_empty() || !self.rail {
            scroll = scroll.child(self.section_header("sb-h-cf", "CUSTOM FEEDS", open, feeds.len(), |s| s.show_custom_feeds = !s.show_custom_feeds, cx));
        }
        if open || self.rail {
            for feed in &feeds {
                scroll = scroll.child(self.custom_feed_row(feed, cx));
            }
            if !self.rail {
                let muted: Hsla = t.colors.text_light;
                scroll = scroll.child(
                    self.row("sb-cf-new", icons::sized(Icon::Plus, px(18.), muted), "New custom feed".into(), false, None, cx)
                        .text_color(muted)
                        .on_click(|_, _, cx| custom_feed::open_new_feed(None, cx)),
                );
            }
        }

        if !moderating.is_empty() {
            let open = self.show_moderating;
            scroll = scroll.child(self.section_header("sb-h-mod", "MODERATING", open, moderating.len(), |s| s.show_moderating = !s.show_moderating, cx));
            if open || self.rail {
                for (ix, c) in moderating.iter().enumerate() {
                    scroll = scroll.child(self.community_row("sb-mod", ix, c, true, cx));
                }
            }
        }

        let open = self.show_communities;
        scroll = scroll.child(self.section_header(
            "sb-h-comms",
            if logged_in { "JOINED" } else { "POPULAR" },
            open,
            total,
            |s| s.show_communities = !s.show_communities,
            cx,
        ));
        if open && !self.rail && self.show_filter {
            scroll = scroll.child(div().flex_none().px(px(2.)).pb(px(6.)).child(self.filter.clone()));
        }
        if open || self.rail {
            if communities.is_empty() && !self.rail {
                let msg = match (total, communities_loaded, communities_failed) {
                    (0, false, true) => Some("Communities appear here once Risquette can reach the server."),
                    (0, false, false) => None,
                    (0, true, _) => Some("You haven't joined any communities yet."),
                    _ => Some("No matches."),
                };
                if let Some(msg) = msg {
                    scroll = scroll.child(div().px(px(12.)).py(px(6.)).text_size(t.fs.s).text_color(t.colors.text_light).child(msg));
                }
            }
            for (ix, c) in communities.iter().enumerate() {
                scroll = scroll.child(self.community_row("sb-c", ix, c, false, cx));
            }
            if logged_in && !self.rail {
                let muted: Hsla = t.colors.text_light;
                scroll = scroll.child(
                    self.row("sb-create", icons::sized(Icon::Plus, px(18.), muted), "Create a community".into(), false, None, cx)
                        .text_color(muted)
                        .on_click(|_, _, cx| communities::open_create_community(cx)),
                );
            }
        }

        let collapsed = self.collapsed;
        let header = div()
            .h(t.metrics.navbar_height)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(if self.rail { 0. } else { 12. }))
            .when(self.rail, |d| d.justify_center())
            .border_b_1()
            .border_color(t.colors.separator)
            .child(
                ui::icon_button(
                    "sb-toggle",
                    if collapsed { Icon::Menu } else { Icon::PanelLeft },
                    if collapsed { "Expand sidebar" } else { "Collapse sidebar" },
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_collapsed(cx))),
            )
            .when(!self.rail, |d| {
                d.child(
                    div()
                        .id("logo")
                        .text_size(px(21.))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(t.colors.brand)
                        .cursor_pointer()
                        .child("risquette")
                        .on_click(|_, _, cx| nav::go(Target::Home, cx)),
                )
            });

        div()
            .w(px(width))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.colors.bg)
            .border_r_1()
            .border_color(t.colors.separator)
            .child(header)
            .child(scroll)
            .child(div().border_t_1().border_color(t.colors.separator).py(px(6.)).child(self.account_card(cx)))
    }
}
