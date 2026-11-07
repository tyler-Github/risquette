//! The window's root view: navbar, sidebar, navigation stack, and overlays
//! (modals, context menus, toasts).

use crate::{
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
    site_aside::SiteAside,
    sidebar::Sidebar,
    state::AppState,
    storage, ui,
    user::UserView,
    Back, Refresh, ToggleTheme,
};
use discuit_api::FeedType;
use gpui::{
    anchored, deferred, div, prelude::*, px, AnyView, App, Context, Entity, FocusHandle, Focusable,
    FontWeight, Pixels, Point, SharedString, Subscription, Task, Window,
};
use risquette_theme::{ActiveTheme, Mode, Theme};
use std::{rc::Rc, time::Duration};

struct Route {
    target: Target,
    view: AnyView,
    title: Rc<dyn Fn(&App) -> SharedString>,
    refresh: Option<Rc<dyn Fn(&mut App)>>,
    /// Page controls shown in the toolbar next to the title (e.g. the feed sort).
    controls: Option<Rc<dyn Fn(&App) -> Option<gpui::AnyElement>>>,
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
                if was_first {
                    if let Some(t) = cx.try_global::<nav::StartupTarget>().and_then(|s| s.0.clone()) {
                        this.navigate(t, cx);
                    }
                }
            }
            cx.notify();
        }));
        // Re-render for the update pill; offer the dialog when an automatic check finds one.
        let updater = crate::updater::Updater::entity(cx);
        subs.push(cx.observe(&updater, |_, _, cx| cx.notify()));
        subs.push(cx.subscribe(&updater, |_, _, _: &crate::updater::UpdaterEvent, cx| crate::updater::open_dialog(cx)));
        let sidebar = cx.new(Sidebar::new);
        let quick_jump = cx.new(QuickJump::new);
        let mut this = Self { stack: Vec::new(), sidebar, quick_jump, focus: cx.focus_handle(), modal: None, menu: None, toasts: Vec::new(), next_toast: 0, _subs: subs, _tasks: Vec::new() };
        this.navigate(Target::All, cx);
        this.start_polling(cx);
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
                        })
                    })
                    .ok();
                }
            }
        }));
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
                Route { target: target.clone(), view: view.into(), title: Rc::new(move |_| title.clone()), refresh: Some(Rc::new(move |cx| r.update(cx, |v, cx| v.refresh(cx)))), controls: None }
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
                Route { target: target.clone(), view: view.into(), title: Rc::new(|_| "Notifications".into()), refresh: Some(Rc::new(move |cx| r.update(cx, |v, cx| v.refresh(cx)))), controls: None }
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
        storage::update(|s| s.theme = Some(if mode == Mode::Dark { "dark" } else { "light" }.into()));
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
            MenuItem::new("Settings", |_, cx| nav::go(Target::Settings, cx)).icon(Icon::Settings),
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
        items.push(MenuItem::new("Log out", |_, cx| Session::entity(cx).update(cx, |s, cx| s.logout(cx))).icon(Icon::LogOut).danger());
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
                    .child(ui::icon_button("nav-refresh", Icon::RefreshCw, "Refresh (F5)", cx).on_click(cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx))))
                    .child(
                        ui::icon_button(
                            "nav-theme",
                            if t.mode == Mode::Dark { Icon::Sun } else { Icon::Moon },
                            if t.mode == Mode::Dark { "Light theme" } else { "Dark theme" },
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(&ToggleTheme, window, cx))),
                    )
                    .when(logged_in, |d| {
                        d.child(ui::button_main_with_icon("nav-new", Icon::Plus, "New post", cx).ml(px(6.)).on_click(|_, _, cx| nav::go(Target::NewPost { community: None }, cx)))
                    }),
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
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.modal = None;
                        cx.notify();
                    }))
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
                    div()
                        .id(("ctx-item", ix))
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
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
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
                            .bg(if t.mode == Mode::Dark { gpui::white() } else { gpui::black() }.opacity(0.88))
                            .text_color(if t.mode == Mode::Dark { gpui::black() } else { gpui::white() })
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
                    .child(div().flex_1().min_h_0().children(body)),
            )
            .children(overlays)
    }
}
