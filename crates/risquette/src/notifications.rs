//! The notifications page.

use crate::{
    icons::Icon,
    markdown, modals,
    nav::{self, MenuItem},
    session::Session,
    state::{api, api_view},
    ui,
};
use discuit_api::Notification;
use gpui::{Context, FontWeight, SharedString, Subscription, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

pub struct NotificationsView {
    items: Vec<Notification>,
    next: Option<String>,
    /// Last unread count pushed to the session (for the bell).
    synced_unread: Option<i64>,
    count: i64,
    loading: bool,
    error: Option<SharedString>,
    _tasks: Vec<Task<()>>,
    _reconnect: Subscription,
}

impl NotificationsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Reload once back online if loading failed.
        let reconnect = crate::connection::on_reconnect(cx, |this: &mut Self, cx| {
            if this.error.is_some() {
                this.refresh(cx);
            }
        });
        let mut this =
            Self { items: Vec::new(), next: None, synced_unread: None, count: 0, loading: false, error: None, _tasks: Vec::new(), _reconnect: reconnect };
        this.refresh(cx);
        // Opening the page clears the badge.
        api(
            cx,
            |c| c.reset_new_notification_count(),
            |r, cx| {
                if r.is_ok() {
                    Session::entity(cx).update(cx, |s, cx| s.set_notification_count(0, cx));
                }
            },
        );
        this
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.items.clear();
        self.next = None;
        self.load(cx);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        let next = self.next.clone();
        self._tasks.push(api_view(
            cx,
            move |c| c.notifications(next.as_deref()),
            |this, r, _| {
                this.loading = false;
                match r {
                    Ok(page) => {
                        this.next = page.next_cursor();
                        this.count = page.count;
                        this.items.extend(page.items.unwrap_or_default());
                    }
                    Err(e) => this.error = Some(e.user_message().into()),
                }
            },
        ));
        cx.notify();
    }

    fn open(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(n) = self.items.get_mut(ix) else { return };
        if !n.seen {
            n.seen = true;
            let id = n.id;
            api(cx, move |c| c.mark_notification_seen(id, true), |_, _| {});
        }
        if let Some(url) = n.to_url.clone() {
            nav::open_link(&url, cx);
        }
        cx.notify();
    }

    fn item_menu(&self, ix: usize, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let Some(n) = self.items.get(ix) else { return Vec::new() };
        let (id, seen) = (n.id, n.seen);
        let entity = cx.entity().downgrade();
        let entity2 = entity.clone();
        vec![
            MenuItem::new(if seen { "Mark as unread" } else { "Mark as read" }, move |_, cx| {
                let entity = entity.clone();
                api(
                    cx,
                    move |c| c.mark_notification_seen(id, !seen),
                    move |r, cx| match r {
                        Ok(_) => {
                            entity
                                .update(cx, |this, cx| {
                                    if let Some(n) = this.items.iter_mut().find(|n| n.id == id) {
                                        n.seen = !seen;
                                    }
                                    cx.notify();
                                })
                                .ok();
                        }
                        Err(e) => nav::toast(e.user_message(), cx),
                    },
                );
            }),
            MenuItem::new("Delete", move |_, cx| {
                let entity = entity2.clone();
                api(
                    cx,
                    move |c| c.delete_notification(id),
                    move |r, cx| match r {
                        Ok(_) => {
                            entity
                                .update(cx, |this, cx| {
                                    this.items.retain(|n| n.id != id);
                                    cx.notify();
                                })
                                .ok();
                        }
                        Err(e) => nav::toast(e.user_message(), cx),
                    },
                );
            })
            .danger(),
        ]
    }
}

impl Render for NotificationsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let unread = self.items.iter().filter(|n| !n.seen).count();
        // Keep the bell in step with what's on this page (marking read/unread, deleting, …).
        if !self.loading && self.error.is_none() && self.synced_unread != Some(unread as i64) {
            self.synced_unread = Some(unread as i64);
            let n = unread as i64;
            cx.defer(move |cx| Session::entity(cx).update(cx, |s, cx| s.set_unread_notifications(n, cx)));
        }
        let header = crate::community::aside_section("NOTIFICATIONS", cx)
            .child(
                div()
                    .flex()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::BOLD).text_size(t.fs.l).child(ui::compact(self.count)))
                            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child("total")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(t.fs.l)
                                    .text_color(if unread > 0 { t.colors.brand } else { t.colors.text })
                                    .child(unread.to_string()),
                            )
                            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child("unread (loaded)")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(ui::button_with_icon("n-seen", Icon::MailCheck, "Mark all as read", cx).justify_center().on_click(cx.listener(|_, _, _, cx| {
                        let entity = cx.entity().downgrade();
                        api(
                            cx,
                            |c| c.mark_all_notifications_seen(None),
                            move |r, cx| match r {
                                Ok(_) => {
                                    entity
                                        .update(cx, |this, cx| {
                                            this.items.iter_mut().for_each(|n| n.seen = true);
                                            cx.notify();
                                        })
                                        .ok();
                                }
                                Err(e) => nav::toast(e.user_message(), cx),
                            },
                        );
                    })))
                    .child(ui::button_with_icon("n-clear", Icon::Trash, "Delete all", cx).justify_center().on_click(cx.listener(|_, _, _, cx| {
                        let entity = cx.entity().downgrade();
                        modals::confirm(
                            "Delete all notifications?",
                            "This can't be undone.",
                            "Delete all",
                            move |cx| {
                                let entity = entity.clone();
                                api(
                                    cx,
                                    |c| c.delete_all_notifications(),
                                    move |r, cx| match r {
                                        Ok(_) => {
                                            entity
                                                .update(cx, |this, cx| {
                                                    this.items.clear();
                                                    this.next = None;
                                                    cx.notify();
                                                })
                                                .ok();
                                        }
                                        Err(e) => nav::toast(e.user_message(), cx),
                                    },
                                );
                            },
                            cx,
                        );
                    }))),
            );

        let mut list = div().flex().flex_col().gap(px(6.));
        for (ix, n) in self.items.clone().into_iter().enumerate() {
            let icon = n.icon_url();
            let title = n.title.clone().unwrap_or_else(|| n.kind.replace('_', " "));
            let menu = self.item_menu(ix, cx);
            list = list.child(
                ui::card(cx)
                    .id(("notif", ix))
                    .p(px(12.))
                    .flex()
                    .gap(px(12.))
                    .cursor_pointer()
                    .when(!n.seen, |d| d.bg(t.colors.brand.opacity(0.07)).border_color(t.colors.brand.opacity(0.4)))
                    .child(ui::avatar(icon.as_deref(), &n.kind, px(36.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().text_size(t.fs.normal).when(!n.seen, |d| d.font_weight(FontWeight::SEMIBOLD)).child(markdown::render(
                                format!("n-t-{ix}"),
                                &title,
                                cx,
                            )))
                            .when_some(n.body.clone().filter(|b| !b.trim().is_empty()), |d, b| {
                                d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).line_clamp(3).child(markdown::plain_preview(&b, 240)))
                            })
                            .when_some(n.created_at, |d, at| d.child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(ui::time_ago(at)))),
                    )
                    .child(ui::menu_button(("n-menu", ix), move |_| menu.clone(), cx))
                    .on_click(cx.listener(move |this, _, _, cx| this.open(ix, cx))),
            );
        }
        if self.items.is_empty() && !self.loading && self.error.is_none() {
            list = list.child(ui::notice("You're all caught up.", cx));
        }
        if let Some(e) = &self.error {
            list = list.child(ui::notice(format!("Couldn't load notifications: {e}"), cx));
        }
        if self.loading {
            list = list.child(ui::notice("Loading…", cx));
        } else if self.next.is_some() {
            list = list.child(
                div().flex().justify_center().py(px(10.)).child(ui::button("n-more", "Load more", cx).on_click(cx.listener(|this, _, _, cx| this.load(cx)))),
            );
        }

        let main = div().id("notifications").size_full().overflow_y_scroll().child(div().flex_none().pt(px(12.)).pb(px(16.)).child(list));
        crate::layout::with_aside(main, Some(header.into_any_element()), true, window)
    }
}
