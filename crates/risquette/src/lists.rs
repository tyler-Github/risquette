//! A user's saved lists: browse, create, edit and delete.

use crate::{
    icons::Icon,
    modals::{self, Field, FormSpec},
    nav::{self, MenuItem, Target},
    session::Session,
    state::{api, api_task, api_view},
    ui,
};
use discuit_api::{List, ListPatch, NewList};
use gpui::{Context, FontWeight, SharedString, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

pub struct ListsView {
    username: String,
    owner: Option<discuit_api::User>,
    lists: Option<Vec<List>>,
    error: Option<SharedString>,
    _task: Option<Task<()>>,
    _owner_task: Option<Task<()>>,
}

impl ListsView {
    pub fn new(username: String, cx: &mut Context<Self>) -> Self {
        let name = username.clone();
        let owner_task = api_view(
            cx,
            move |c| c.user(&name),
            |this: &mut Self, r, _| {
                if let Ok(u) = r {
                    this.owner = Some(u);
                }
            },
        );
        let mut this = Self { username, owner: None, lists: None, error: None, _task: None, _owner_task: Some(owner_task) };
        this.refresh(cx);
        this
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let name = self.username.clone();
        self._task = Some(api_view(
            cx,
            move |c| c.user_lists(&name, None, None),
            |this, r, _| match r {
                Ok(l) => this.lists = Some(l),
                Err(e) => this.error = Some(e.user_message().into()),
            },
        ));
    }

    fn is_own(&self, cx: &Context<Self>) -> bool {
        Session::get(cx).username() == Some(self.username.as_str())
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let username = self.username.clone();
        let entity = cx.entity().downgrade();
        FormSpec::new("New list", "Create", move |v, cx| {
            let display = v.texts[0].trim().to_string();
            if display.is_empty() {
                return Task::ready(Err("Give the list a name.".into()));
            }
            let slug: String = display.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
            let list = NewList {
                name: slug,
                display_name: Some(display),
                description: Some(v.texts[1].trim().to_string()).filter(|d| !d.is_empty()),
                public: v.checks[0],
            };
            let username = username.clone();
            let entity = entity.clone();
            let task = api_task(cx, move |c| c.create_list(&username, &list));
            cx.spawn(async move |cx| {
                let lists = task.await?;
                entity
                    .update(cx, |this, cx| {
                        this.lists = Some(lists);
                        cx.notify();
                    })
                    .ok();
                Ok(())
            })
        })
        .field(Field::text("Name", ""))
        .field(Field::multiline("Description (optional)", "", 3))
        .check("Public (anyone can see it)", false)
        .open(cx);
    }

    fn list_menu(&self, list: &List, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let entity = cx.entity().downgrade();
        let l = list.clone();
        let e1 = entity.clone();
        let l2 = list.clone();
        vec![
            MenuItem::new("Edit", move |_, cx| {
                let (id, entity) = (l.id, e1.clone());
                FormSpec::new("Edit list", "Save", move |v, cx| {
                    let patch = ListPatch {
                        display_name: Some(v.texts[0].trim().to_string()),
                        description: Some(v.texts[1].trim().to_string()),
                        public: Some(v.checks[0]),
                        name: None,
                    };
                    let entity = entity.clone();
                    let task = api_task(cx, move |c| c.update_list(id, &patch));
                    cx.spawn(async move |cx| {
                        let updated = task.await?;
                        entity
                            .update(cx, |this, cx| {
                                if let Some(lists) = this.lists.as_mut()
                                    && let Some(x) = lists.iter_mut().find(|x| x.id == updated.id)
                                {
                                    *x = updated;
                                }
                                cx.notify();
                            })
                            .ok();
                        Ok(())
                    })
                })
                .field(Field::text("Name", l.title().to_string()))
                .field(Field::multiline("Description", l.description.clone().unwrap_or_default(), 3))
                .check("Public", l.public)
                .open(cx);
            }),
            MenuItem::new("Clear all items", {
                let id = l2.id;
                move |_, cx| {
                    modals::confirm(
                        "Clear list?",
                        "Every saved item in this list will be removed.",
                        "Clear",
                        move |cx| {
                            api(
                                cx,
                                move |c| c.clear_list(id),
                                |r, cx| nav::toast(r.map(|_| "List cleared".to_string()).unwrap_or_else(|e| e.user_message()), cx),
                            );
                        },
                        cx,
                    );
                }
            })
            .danger(),
            MenuItem::new("Delete list", move |_, cx| {
                let (id, entity) = (l2.id, entity.clone());
                modals::confirm(
                    "Delete list?",
                    "The list and its items will be deleted.",
                    "Delete",
                    move |cx| {
                        let entity = entity.clone();
                        api(
                            cx,
                            move |c| c.delete_list(id),
                            move |r, cx| match r {
                                Ok(_) => {
                                    entity
                                        .update(cx, |this, cx| {
                                            if let Some(lists) = this.lists.as_mut() {
                                                lists.retain(|x| x.id != id);
                                            }
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
            })
            .danger(),
        ]
    }
}

impl Render for ListsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let own = self.is_own(cx);
        let usable = (f32::from(window.viewport_size().width) - 320.).min(crate::layout::PAGE_MAX);
        let cols = ((usable / 360.).floor() as u16).clamp(1, 4);

        // Owner header: whose lists these are.
        let pic = self.owner.as_ref().and_then(|u| u.pro_pic.as_ref()).map(|i| i.best_url(80).to_string());
        let username = self.username.clone();
        let count = self.lists.as_ref().map_or(0, Vec::len) as i64;
        let heading = if own { "Your lists".to_string() } else { format!("@{}'s lists", self.username) };
        let subtitle = if own { format!("{} · only public lists are visible to others", ui::plural(count, "list")) } else { ui::plural(count, "public list") };
        let header = ui::card(cx)
            .p(px(14.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::avatar(pic.as_deref(), &self.username, px(44.), cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("lists-owner")
                            .text_size(t.fs.xl)
                            .font_weight(FontWeight::BOLD)
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child(heading)
                            .on_click(move |_, _, cx| nav::go(Target::User(username.clone()), cx)),
                    )
                    .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(subtitle)),
            )
            .when(own, |d| d.child(ui::button_main_with_icon("lists-new", Icon::Plus, "New list", cx).on_click(cx.listener(|this, _, _, cx| this.create(cx)))));

        let body: gpui::AnyElement = match &self.lists {
            None if self.error.is_some() => ui::notice(self.error.clone().unwrap(), cx).into_any_element(),
            None => ui::notice("Loading…", cx).into_any_element(),
            Some(l) if l.is_empty() => {
                ui::notice(if own { "No lists yet. Save posts and comments from their menu." } else { "No public lists." }, cx).into_any_element()
            }
            Some(lists) => {
                let mut grid = div().grid().grid_cols(cols).gap(px(10.));
                for (ix, l) in lists.clone().into_iter().enumerate() {
                    let (id, title) = (l.id, l.title().to_string());
                    let menu = if own { self.list_menu(&l, cx) } else { Vec::new() };
                    let description = l.description.clone().filter(|d| !d.trim().is_empty());
                    let updated = l.last_updated_at.or(l.created_at).map(ui::time_ago);
                    let hover = t.colors.card_border;
                    grid = grid.child(
                        ui::card(cx)
                            .id(("list", ix))
                            // Grows with its content (descriptions wrap), never shorter than this.
                            .min_h(px(104.))
                            .p(px(14.))
                            .flex()
                            .items_start()
                            .gap(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.border_color(hover))
                            .child(
                                div()
                                    .size(px(40.))
                                    .flex_none()
                                    .rounded(px(8.))
                                    .bg(t.colors.brand.opacity(0.12))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(crate::icons::sized(Icon::Bookmark, px(20.), t.colors.brand)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(div().truncate().font_weight(FontWeight::SEMIBOLD).text_size(t.fs.l).child(l.title().to_string()))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .items_center()
                                            .gap(px(6.))
                                            .text_size(t.fs.xs)
                                            .text_color(t.colors.text_light)
                                            .child(crate::icons::sized(if l.public { Icon::Globe } else { Icon::Lock }, px(12.), t.colors.text_light))
                                            .child(if l.public { "Public" } else { "Private" })
                                            .child("·")
                                            .child(ui::plural(l.num_items, "item"))
                                            .when_some(updated, |d, u| d.child("·").child(format!("updated {u}"))),
                                    )
                                    .child(
                                        div()
                                            .text_size(t.fs.s)
                                            .line_height(px(19.))
                                            .text_color(if description.is_some() { t.colors.text } else { t.colors.text_light })
                                            .line_clamp(2)
                                            .child(description.unwrap_or_else(|| "No description.".into())),
                                    ),
                            )
                            .when(own, |d| d.child(ui::menu_button(("list-menu", ix), move |_| menu.clone(), cx)))
                            .on_click(move |_, _, cx| nav::go(Target::List { id, title: title.clone() }, cx)),
                    );
                }
                grid.into_any_element()
            }
        };
        // A row whose child grows up to PAGE_MAX, not a centred column with
        // `w_full().max_w()`, which can draw text past the edge (see settings.rs).
        div().id("lists").size_full().overflow_y_scroll().child(div().w_full().flex().justify_center().child(
            div().flex_1().min_w_0().max_w(px(crate::layout::PAGE_MAX)).px(px(16.)).py(px(12.)).flex().flex_col().gap(px(12.)).child(header).child(body),
        ))
    }
}
