//! The window's root view: navbar, sidebar, navigation stack, and overlays
//! (modals, context menus, toasts).

use crate::{
    Back, Refresh, ToggleTheme,
    admin::AdminView,
    communities::CommunitiesView,
    community::CommunityView,
    feed::{FeedView, Source},
    icons::{self, Icon},
    lists::ListsView,
    modtools::ModToolsView,
    nav::{self, MenuItem, RootHandle, Target},
    new_post::NewPostView,
    notifications::NotificationsView,
    post::PostView,
    quick_jump::QuickJump,
    session::{Session, SessionEvent},
    settings::SettingsView,
    sidebar::Sidebar,
    site_aside::SiteAside,
    state::AppState,
    storage, ui,
    user::UserView,
};
use discuit_api::FeedType;
use gpui::{
    AnimationExt, AnyView, App, Context, Entity, FocusHandle, Focusable, FontWeight, Pixels, Point, SharedString, Subscription, Task, Window, anchored,
    deferred, div, prelude::*, px,
};
use risquette_theme::{ActiveTheme, Theme};
use std::{rc::Rc, time::Duration};

type TitleFn = Rc<dyn Fn(&App) -> SharedString>;
type RefreshFn = Rc<dyn Fn(&mut App)>;
type ControlsFn = Rc<dyn Fn(&App) -> Option<gpui::AnyElement>>;

struct Route {
    target: Target,
    view: AnyView,
    title: TitleFn,
    refresh: Option<RefreshFn>,
    /// Page controls shown in the toolbar next to the title (e.g. the feed sort).
    controls: Option<ControlsFn>,
}

pub struct Root {
    stack: Vec<Route>,
    sidebar: Entity<Sidebar>,
    quick_jump: Entity<QuickJump>,
    focus: FocusHandle,
    modal: Option<AnyView>,
    menu: Option<(Point<Pixels>, Vec<MenuItem>)>,
    toasts: Vec<(u64, SharedString)>,
    next_toast: u64,
    _subs: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Root {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.set_global(RootHandle(cx.entity().downgrade()));
        let mut subs = Vec::new();
        // When the session first loads (or the user logs in/out), reset to the right home feed.
        let mut last_user = None::<String>;
        let mut first = true;
        subs.push(cx.subscribe(&Session::entity(cx), move |this, session, _: &SessionEvent, cx| {
            crate::desktop::sync_tray(cx);
            let s = session.read(cx);
            let user = s.user().map(|u| u.id.clone());
            if first && s.loaded || user != last_user {
                let was_first = first;
                first = false;
                last_user = user;
                let start = match crate::storage::load().startup_page.as_deref() {
                    Some("all") => Target::All,
                    _ => Target::Home,
                };
                this.navigate(if was_first { start } else { Target::Home }, cx);
                if was_first && let Some(t) = cx.try_global::<nav::StartupTarget>().and_then(|s| s.0.clone()) {
                    this.navigate(t, cx);
                }
            }
            cx.notify();
        }));
        // Re-render for the update pill; offer the dialog when an automatic check finds one.
        let updater = crate::updater::Updater::entity(cx);
        subs.push(cx.observe(&updater, |_, _, cx| cx.notify()));
        subs.push(cx.subscribe(&updater, |_, _, _: &crate::updater::UpdaterEvent, cx| crate::updater::open_dialog(cx)));
        // Show or hide the offline banner.
        subs.push(cx.observe(&crate::connection::entity(cx), |_, _, cx| cx.notify()));
        let sidebar = cx.new(Sidebar::new);
        let quick_jump = cx.new(QuickJump::new);
        let mut this = Self {
            stack: Vec::new(),
            sidebar,
            quick_jump,
            focus: cx.focus_handle(),
            modal: None,
            menu: None,
            toasts: Vec::new(),
            next_toast: 0,
            _subs: subs,
            _tasks: Vec::new(),
        };
        this.navigate(Target::All, cx);
        this.start_polling(cx);
        this.watch_for_new_posts(cx);
        this
    }

    /// Refresh the notification count every minute while logged in (GET /api/_user).
    /// Keep the notification badge current while logged in: shortly after
    /// startup, then every minute. Reads the first page of notifications
    /// (GET), which carries the new count and lets us count unread ones.
    fn start_polling(&mut self, cx: &mut Context<Self>) {
        self._tasks.push(cx.spawn(async move |_, cx| {
            let mut delay = Duration::from_secs(4);
            loop {
                cx.background_executor().timer(delay).await;
                delay = Duration::from_secs(60);
                let Ok(logged_in) = cx.update(|cx| Session::get(cx).is_logged_in()) else { break };
                if !logged_in {
                    continue;
                }
                let Ok(client) = cx.update(|cx| AppState::client(cx)) else { break };
                let result = cx.background_executor().spawn(async move { client.notifications(None) }).await;
                if let Ok(page) = result {
                    let unread = page.items.as_ref().map_or(0, |items| items.iter().filter(|n| !n.seen).count() as i64);
                    cx.update(|cx| {
                        Session::entity(cx).update(cx, |s, cx| {
                            s.set_notification_count(page.new_count, cx);
                            s.set_unread_notifications(unread, cx);
                        });
                        crate::desktop::on_notifications(page.items.as_deref().unwrap_or_default(), cx);
                    })
                    .ok();
                }
            }
        }));
    }

    /// Once a minute, have the feed on screen (if any) look for new posts. Only
    /// that feed asks, not the pages behind it, and it does so even while another
    /// window has focus (Risquette is often left open beside other things).
    fn watch_for_new_posts(&mut self, cx: &mut Context<Self>) {
        self._tasks.push(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(60)).await;
                let result = this.update(cx, |this, cx| {
                    if let Some(feed) = this.current_feed(cx) {
                        feed.update(cx, |f, cx| f.check_for_new(cx));
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        }));
    }

    /// The feed the current page shows: the page itself, or a community's feed.
    fn current_feed(&self, cx: &App) -> Option<Entity<FeedView>> {
        let view = self.stack.last()?.view.clone();
        match view.downcast::<FeedView>() {
            Ok(feed) => Some(feed),
            Err(view) => view.downcast::<CommunityView>().ok()?.read(cx).feed(),
        }
    }

    pub fn navigate(&mut self, target: Target, cx: &mut Context<Self>) {
        self.menu = None;
        let session = Session::get(cx);
        // Logged out, "Home" is the All feed; record it as such so the sidebar highlights it.
        let target = if target == Target::Home && !session.is_logged_in() { Target::All } else { target };
        let route = match &target {
            Target::Home | Target::All => {
                let feed_type = if target == Target::Home && session.is_logged_in() {
                    match session.user().and_then(|u| u.home_feed.as_deref()) {
                        Some("all") => FeedType::All,
                        _ => FeedType::Home,
                    }
                } else {
                    FeedType::All
                };
                let aside = cx.new(SiteAside::new);
                let title = if target == Target::Home { "Home" } else { "All" };
                let feed = cx.new(|cx| FeedView::new(Source::Site(feed_type), cx).with_aside(aside).with_title(title));
                self.stack.clear();
                feed_route(target.clone(), feed)
            }
            Target::Post { public_id } => {
                let view = cx.new(|cx| PostView::new(public_id.clone(), cx));
                let v = view.clone();
                Route { target: target.clone(), view: view.into(), title: Rc::new(move |cx| v.read(cx).title()), refresh: None, controls: None }
            }
            Target::Community(name) => {
                let view = cx.new(|cx| CommunityView::new(name.clone(), cx));
                let v = view.clone();
                let r = view.clone();
                let c = view.clone();
                Route {
                    target: target.clone(),
                    view: view.into(),
                    title: Rc::new(move |cx| v.read(cx).title()),
                    refresh: Some(Rc::new(move |cx| r.update(cx, |v, cx| v.refresh(cx)))),
                    controls: Some(Rc::new(move |cx| c.read(cx).feed().and_then(|f| crate::feed::sort_control(&f, cx)))),
                }
            }
            Target::User(name) => {
                let view = cx.new(|cx| UserView::new(name.clone(), cx));
                let r = view.clone();
                let title: SharedString = format!("@{name}").into();
                Route {
                    target: target.clone(),
                    view: view.into(),
                    title: Rc::new(move |_| title.clone()),
                    refresh: Some(Rc::new(move |cx| r.update(cx, |v, cx| v.refresh(cx)))),
                    controls: None,
                }
            }
            Target::NewPost { community } => {
                if !crate::actions::require_login(cx) {
                    return;
                }
                let view = cx.new(|cx| NewPostView::new(community.clone(), cx));
                simple(target.clone(), view.into(), "New post")
            }
            Target::Notifications => {
                if !crate::actions::require_login(cx) {
                    return;
                }
                let view = cx.new(NotificationsView::new);
                let r = view.clone();
                Route {
                    target: target.clone(),
                    view: view.into(),
                    title: Rc::new(|_| "Notifications".into()),
                    refresh: Some(Rc::new(move |cx| r.update(cx, |v, cx| v.refresh(cx)))),
                    controls: None,
                }
            }
            Target::Settings => {
                let view = cx.new(SettingsView::new);
                simple(target.clone(), view.into(), "Settings")
            }
            Target::Lists(username) => {
                let view = cx.new(|cx| ListsView::new(username.clone(), cx));
                let own = Session::get(cx).username() == Some(username.as_str());
                let title: SharedString = if own { "Saved lists".into() } else { format!("@{username}'s lists").into() };
                Route { target: target.clone(), view: view.into(), title: Rc::new(move |_| title.clone()), refresh: None, controls: None }
            }
            Target::List { id, title } => {
                let feed = cx.new(|cx| FeedView::new(Source::List(*id), cx).with_title(title.clone()));
                feed_route(target.clone(), feed)
            }
            Target::Communities => {
                let view = cx.new(CommunitiesView::new);
                simple(target.clone(), view.into(), "Communities")
            }
            Target::ModTools(name) => {
                let view = cx.new(|cx| ModToolsView::new(name.clone(), cx));
                let title: SharedString = format!("Mod tools · {name}").into();
                Route { target: target.clone(), view: view.into(), title: Rc::new(move |_| title.clone()), refresh: None, controls: None }
            }
            Target::Admin => {
                let view = cx.new(AdminView::new);
                simple(target.clone(), view.into(), "Admin")
            }
            Target::CustomFeed(id) => {
                let id = *id;
                let header = cx.new(|cx| crate::custom_feed::CustomFeedHeader::new(id, cx));
                let feed = cx.new(|cx| FeedView::new(Source::Custom(id), cx).with_header(header.clone()));
                header.update(cx, |h, _| h.set_feed(feed.downgrade()));
                let mut route = feed_route(target.clone(), feed);
                // The title follows renames.
                route.title = Rc::new(move |cx| crate::personal::custom_feed(id, cx).map(|f| f.name.into()).unwrap_or_else(|| "Custom feed".into()));
                route
            }
        };
        self.stack.push(route);
        if self.stack.len() > 40 {
            self.stack.remove(1);
        }
        self.sync_sidebar(cx);
        cx.notify();
    }

    fn sync_sidebar(&mut self, cx: &mut Context<Self>) {
        let current = self.stack.last().map(|r| r.target.clone());
        self.sidebar.update(cx, |s, cx| s.set_current(current, cx));
    }

    pub fn set_modal(&mut self, modal: Option<AnyView>, cx: &mut Context<Self>) {
        self.modal = modal;
        self.menu = None;
        cx.notify();
    }

    pub fn set_menu(&mut self, menu: Option<(Point<Pixels>, Vec<MenuItem>)>, cx: &mut Context<Self>) {
        self.menu = menu;
        cx.notify();
    }

    pub fn show_toast(&mut self, message: SharedString, cx: &mut Context<Self>) {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push((id, message));
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|(i, _)| *i != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn back(&mut self, _: &Back, _window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.menu = None;
        } else if self.modal.is_some() {
            self.modal = None;
        } else if self.stack.len() > 1 {
            self.stack.pop();
            self.sync_sidebar(cx);
        }
        cx.notify();
    }

    fn refresh(&mut self, _: &Refresh, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.stack.last().and_then(|r| r.refresh.clone()) {
            f(cx);
        } else if let Some(target) = self.stack.last().map(|r| r.target.clone()) {
            // Re-open the current page.
            self.stack.pop();
            self.navigate(target, cx);
        }
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, _window: &mut Window, cx: &mut Context<Self>) {
        let mode = cx.theme().mode.toggled();
        storage::update(|s| s.theme = Some(mode.setting().into()));
        Theme::set_mode(mode, cx);
    }

    /// The account menu, opened from the sidebar's account card.
    pub fn user_menu(cx: &App) -> Vec<MenuItem> {
        let session = Session::get(cx);
        let Some(user) = session.user() else { return Vec::new() };
        let username = user.username.clone();
        let mods_something = user.modding_list.as_ref().is_some_and(|l| !l.is_empty());
        let mut items = vec![
            MenuItem::new(format!("Profile (@{username})"), {
                let u = username.clone();
                move |_, cx| nav::go(Target::User(u.clone()), cx)
            })
            .icon(Icon::User),
            MenuItem::new("Moderating feed", |_, cx| {
                let feed = cx.new(|cx| FeedView::new(Source::Site(FeedType::Moderating), cx));
                if let Some(root) = cx.try_global::<RootHandle>().and_then(|h| h.0.upgrade()) {
                    cx.defer(move |cx| {
                        root.update(cx, |r, cx| {
                            r.stack.push(feed_route(Target::All, feed));
                            cx.notify();
                        })
                    });
                }
            })
            .icon(Icon::Shield),
        ];
        if !mods_something {
            items.retain(|i| i.label.as_ref() != "Moderating feed");
        }
        // Other saved accounts on this instance, then adding one.
        let others: Vec<_> = session.saved_accounts(cx).into_iter().filter(|a| !a.username.eq_ignore_ascii_case(&username)).collect();
        for (ix, account) in others.into_iter().enumerate() {
            let name = account.username.clone();
            let item = MenuItem::new(format!("Switch to @{name}"), move |_, cx| {
                let name = name.clone();
                Session::entity(cx).update(cx, |s, cx| s.switch_account(&name, cx));
            })
            .icon(Icon::User);
            items.push(if ix == 0 { item.separated() } else { item });
        }
        let add = MenuItem::new("Add another account", |_, cx| crate::modals::open_add_account(cx)).icon(Icon::Plus);
        items.push(if items.iter().any(|i| i.separated) { add } else { add.separated() });
        items.push(
            MenuItem::new(format!("Log out @{username}"), |_, cx| Session::entity(cx).update(cx, |s, cx| s.logout(cx))).icon(Icon::LogOut).danger().separated(),
        );
        items
    }

    /// Notifications bell with an unread counter; opens the notifications page.
    fn render_bell(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let unread = Session::get(cx).notification_badge();
        let active = self.stack.last().is_some_and(|r| r.target == Target::Notifications);
        let tip = match unread {
            0 => "Notifications".to_string(),
            1 => "Notifications · 1 unread".to_string(),
            n => format!("Notifications · {n} unread"),
        };
        div()
            .relative()
            .child(
                ui::icon_button("nav-bell", Icon::Bell, tip, cx)
                    .when(active, |d| d.bg(t.colors.button))
                    .on_click(|_, _, cx| nav::go(Target::Notifications, cx)),
            )
            .when(unread > 0, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(1.))
                        .right(px(-2.))
                        .min_w(px(17.))
                        .h(px(17.))
                        .px(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(9.))
                        .border_2()
                        .border_color(t.colors.bg)
                        .bg(t.colors.red)
                        .text_color(gpui::white())
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .child(if unread > 99 { "99+".to_string() } else { unread.to_string() }),
                )
            })
    }

    /// The toolbar above the content: back + page title, quick-jump search, and actions.
    fn render_navbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let can_go_back = self.stack.len() > 1;
        let title = self.stack.last().map(|r| (r.title)(cx)).unwrap_or_default();
        let controls = self.stack.last().and_then(|r| r.controls.clone()).and_then(|f| f(cx));
        let logged_in = Session::get(cx).is_logged_in();

        div()
            .h(t.metrics.navbar_height)
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(12.))
            .bg(t.colors.bg)
            .border_b_1()
            .border_color(t.colors.separator)
            .child(
                div()
                    .flex_1()
                    .flex_basis(px(0.))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        ui::disabled(ui::icon_button("nav-back", Icon::ChevronLeft, "Back (Esc)", cx), !can_go_back)
                            .on_click(cx.listener(|this, _, window, cx| this.back(&Back, window, cx))),
                    )
                    .child(div().min_w_0().truncate().text_size(t.fs.l).font_weight(FontWeight::SEMIBOLD).child(title))
                    .children(controls.map(|c| div().flex_none().ml(px(8.)).child(c))),
            )
            .child(div().w(px(460.)).flex_shrink().min_w(px(200.)).child(self.quick_jump.clone()))
            .child(
                div()
                    .flex_1()
                    .flex_basis(px(0.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(4.))
                    .children(crate::updater::toolbar_pill(cx))
                    .when(logged_in, |d| d.child(self.render_bell(cx)))
                    .child(
                        ui::icon_button("nav-refresh", Icon::RefreshCw, "Refresh (F5)", cx)
                            .on_click(cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx))),
                    )
                    .child(
                        ui::icon_button(
                            "nav-theme",
                            if t.mode.is_dark() { Icon::Sun } else { Icon::Moon },
                            if t.mode.is_dark() { "Light theme" } else { "Dark theme" },
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(&ToggleTheme, window, cx))),
                    )
                    .when(logged_in, |d| {
                        d.child(
                            ui::button_main_with_icon("nav-new", Icon::Plus, "New post", cx)
                                .ml(px(6.))
                                .on_click(|_, _, cx| nav::go(Target::NewPost { community: None }, cx)),
                        )
                    }),
            )
    }

    /// While the server can't be reached: a card over the dimmed window, with a
    /// countdown to the next attempt, "Try now", and a way into Settings in case
    /// the server address itself is the problem.
    fn render_reconnecting(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let connection = crate::connection::entity(cx);
        let (show, checking, next) = {
            let c = connection.read(cx);
            (c.show_screen(), c.is_checking(), c.seconds_to_next_check())
        };
        if !show {
            return None;
        }
        let t = *cx.theme();
        let host = AppState::client(cx).base_url().host_str().unwrap_or("the server").to_string();
        let status: SharedString = match (checking, next) {
            (true, _) | (false, None) => "Reconnecting…".into(),
            (false, Some(1)) => "Trying again in 1 second".into(),
            (false, Some(n)) => format!("Trying again in {n} seconds").into(),
        };
        let pulse = || gpui::Animation::new(Duration::from_millis(1800)).repeat().with_easing(gpui::pulsating_between(0.15, 0.9));
        let retry = connection.clone();

        // The logo inside a softly pulsing ring.
        let mark = div()
            .relative()
            .size(px(96.))
            .flex()
            .items_center()
            .justify_center()
            .child(div().absolute().inset_0().rounded_full().border_2().border_color(t.colors.brand).with_animation(
                "reconnect-ring",
                pulse(),
                |ring, delta| ring.opacity(delta),
            ))
            .child(gpui::img(logo()).size(px(64.)));

        // "Trying again in 3 seconds", after a pulsing dot.
        let status_pill = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(5.))
            .rounded_full()
            .bg(t.colors.input)
            .text_size(t.fs.s)
            .text_color(t.colors.text_light)
            .child(div().size(px(8.)).rounded_full().bg(t.colors.brand).with_animation("reconnect-dot", pulse(), |dot, delta| dot.opacity(delta)))
            .child(status);

        let card = div()
            .w(px(360.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.))
            .px(px(28.))
            .pt(px(30.))
            .pb(px(24.))
            .rounded(px(16.))
            .bg(t.colors.card)
            .border_1()
            .border_color(t.colors.card_ring)
            .shadow_lg()
            .child(mark)
            .child(div().mt(px(4.)).text_size(t.fs.l).font_weight(FontWeight::BOLD).text_center().child(format!("Can't reach {host}")))
            .child(
                div()
                    .text_size(t.fs.s)
                    .text_color(t.colors.text_light)
                    .text_center()
                    .child("Check your internet connection. Risquette will pick up where you left off once it's back."),
            )
            .child(status_pill)
            .child(
                div()
                    .mt(px(6.))
                    .w(px(304.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        ui::button_main("reconnect-now", if checking { "Trying…" } else { "Try now" }, cx)
                            .justify_center()
                            .on_click(move |_, _, cx| retry.update(cx, |c, cx| c.check_now(cx))),
                    )
                    .child(
                        ui::button_clear("reconnect-settings", cx)
                            .justify_center()
                            .text_size(t.fs.s)
                            .text_color(t.colors.text_light)
                            .child("Use a different server")
                            .on_click(move |_, _, cx| {
                                connection.update(cx, |c, cx| c.dismiss(cx));
                                nav::go(Target::Settings, cx);
                            }),
                    ),
            );

        Some(
            div()
                .id("reconnecting")
                .absolute()
                .inset_0()
                // Dim the app, and keep clicks and scrolling from reaching it.
                .bg(t.colors.body.opacity(0.94))
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .child(card),
        )
    }

    /// Offline with the reconnecting screen set aside: a strip under the toolbar, with "Try again".
    fn render_offline_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let connection = crate::connection::entity(cx);
        let (show, checking) = {
            let c = connection.read(cx);
            (c.show_banner(), c.is_checking())
        };
        if !show {
            return None;
        }
        let t = cx.theme();
        let base = AppState::client(cx).base_url().host_str().unwrap_or("the server").to_string();
        Some(
            div()
                .id("offline-banner")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(8.))
                .bg(t.colors.card)
                .border_b_1()
                .border_color(t.colors.separator)
                .text_size(t.fs.s)
                .child(icons::icon(Icon::WifiOff, t.colors.red))
                .child(div().flex_1().min_w_0().child(format!("Can't reach {base}. Check your internet connection; Risquette will reconnect by itself.")))
                .child(
                    ui::button("offline-retry", if checking { "Checking…" } else { "Try again" }, cx)
                        .flex_none()
                        .on_click(move |_, _, cx| connection.update(cx, |c, cx| c.check_now(cx))),
                ),
        )
    }

    fn render_overlays(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let t = *cx.theme();
        let mut out = Vec::new();
        if let Some(modal) = self.modal.clone() {
            out.push(
                div()
                    .id("modal-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(t.colors.modal_overlay)
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.modal = None;
                            cx.notify();
                        }),
                    )
                    .child(modal)
                    .into_any_element(),
            );
        }
        if let Some((pos, items)) = self.menu.clone() {
            let menu = div()
                .id("ctx-menu")
                .min_w(px(200.))
                .py(px(4.))
                .bg(t.colors.card)
                .rounded(t.metrics.radius)
                .border_1()
                .border_color(t.colors.card_border)
                .shadow_lg()
                .flex()
                .flex_col()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(items.into_iter().enumerate().map(|(ix, item)| {
                    let action = item.action.clone();
                    let hover = t.colors.button;
                    let separator = t.colors.separator;
                    div()
                        .id(("ctx-item", ix))
                        .when(item.separated && ix > 0, |d| d.mt(px(4.)).border_t_1().border_color(separator).pt(px(11.)))
                        .px(px(14.))
                        .py(px(7.))
                        .text_size(t.fs.normal)
                        .text_color(if item.danger { t.colors.red } else { t.colors.text })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .when_some(item.icon, |d, i| d.child(icons::icon(i, if item.danger { t.colors.red } else { t.colors.text_light })))
                        .child(item.label.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.menu = None;
                            cx.notify();
                            action(window, cx);
                        }))
                }));
            out.push(
                div()
                    .id("menu-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }),
                    )
                    .child(deferred(anchored().position(pos).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2))
                    .into_any_element(),
            );
        }
        if !self.toasts.is_empty() {
            let _ = window;
            out.push(
                div()
                    .absolute()
                    .bottom(px(24.))
                    .left_0()
                    .right_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .children(self.toasts.iter().map(|(_, msg)| {
                        div()
                            .px(px(16.))
                            .py(px(10.))
                            .rounded(px(8.))
                            .bg(if t.mode.is_dark() { gpui::white() } else { gpui::black() }.opacity(0.88))
                            .text_color(if t.mode.is_dark() { gpui::black() } else { gpui::white() })
                            .text_size(t.fs.normal)
                            .shadow_lg()
                            .child(msg.clone())
                    }))
                    .into_any_element(),
            );
        }
        out
    }
}

fn simple(target: Target, view: AnyView, title: &'static str) -> Route {
    Route { target, view, title: Rc::new(move |_| title.into()), refresh: None, controls: None }
}

fn feed_route(target: Target, feed: Entity<FeedView>) -> Route {
    let v = feed.clone();
    let r = feed.clone();
    let c = feed.clone();
    Route {
        target,
        view: feed.into(),
        title: Rc::new(move |cx| v.read(cx).title()),
        refresh: Some(Rc::new(move |cx| r.update(cx, |f, cx| f.refresh(cx)))),
        controls: Some(Rc::new(move |cx| crate::feed::sort_control(&c, cx))),
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let body = self.stack.last().map(|r| r.view.clone());
        let overlays = self.render_overlays(window, cx);
        div()
            .key_context("Root")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::toggle_theme))
            .relative()
            .size_full()
            .flex()
            .bg(t.colors.body)
            .text_color(t.colors.text)
            .font_family(t.font_family)
            .text_size(t.fs.normal)
            .child(self.sidebar.clone())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_navbar(cx))
                    .children(self.render_offline_banner(cx))
                    .child(div().flex_1().min_h_0().children(body)),
            )
            .children(overlays)
            .children(self.render_reconnecting(cx))
    }
}

/// The app's logo, for the reconnecting screen.
fn logo() -> std::sync::Arc<gpui::Image> {
    static LOGO: std::sync::OnceLock<std::sync::Arc<gpui::Image>> = std::sync::OnceLock::new();
    LOGO.get_or_init(|| std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, include_bytes!("../../../branding/logo-128.png").to_vec()))).clone()
}
