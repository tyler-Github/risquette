//! Dialogs: a generic form modal (used for login, signup, edits, confirms and
//! most mod/admin actions), plus report and save-to-list dialogs.

use crate::{
    icons::Icon,
    bus::{self, ContentEvent},
    input::{InputEvent, TextInput},
    nav,
    session::Session,
    state::{api_task, api_view},
    ui,
};
use discuit_api::{Comment, ContentType, List, NewList, Post};
use gpui::{
    div, prelude::*, px, AnyElement, App, AppContext, Context, Entity, FontWeight, SharedString,
    Subscription, Task, Window,
};
use risquette_theme::ActiveTheme;
use std::{collections::HashSet, rc::Rc};

/// The card a modal is drawn in.
pub fn frame(title: impl Into<SharedString>, width: f32, body: impl IntoElement, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    div()
        .id("modal-card")
        .w(px(width))
        .max_h(px(720.))
        .overflow_y_scroll()
        .bg(t.colors.card)
        .rounded(px(8.))
        .border_1()
        .border_color(t.colors.card_border)
        .shadow_lg()
        .p(px(22.))
        .flex()
        .flex_col()
        .gap(px(14.))
        // Clicks inside the card must not reach the backdrop.
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(t.fs.xl + px(2.)).font_weight(FontWeight::BOLD).text_color(t.colors.text).child(title.into()))
                .child(ui::icon_button("modal-close", Icon::X, "Close (Esc)", cx).on_click(|_, _, cx| nav::close_modal(cx))),
        )
        .child(body)
}

// ---------------------------------------------------------------------------
// Generic form modal
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum FieldKind {
    Text,
    Password,
    Multiline(usize),
}

#[derive(Clone)]
pub struct Field {
    pub label: SharedString,
    pub kind: FieldKind,
    pub value: String,
    pub placeholder: SharedString,
}

impl Field {
    pub fn text(label: impl Into<SharedString>, value: impl Into<String>) -> Self {
        Self { label: label.into(), kind: FieldKind::Text, value: value.into(), placeholder: SharedString::default() }
    }
    pub fn password(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), kind: FieldKind::Password, value: String::new(), placeholder: SharedString::default() }
    }
    pub fn multiline(label: impl Into<SharedString>, value: impl Into<String>, rows: usize) -> Self {
        Self { label: label.into(), kind: FieldKind::Multiline(rows), value: value.into(), placeholder: SharedString::default() }
    }
    pub fn placeholder(mut self, p: impl Into<SharedString>) -> Self {
        self.placeholder = p.into();
        self
    }
}

pub struct Values {
    pub texts: Vec<String>,
    pub checks: Vec<bool>,
}

type Submit = Rc<dyn Fn(Values, &mut App) -> Task<Result<(), String>>>;

pub struct FormSpec {
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub fields: Vec<Field>,
    pub checks: Vec<(SharedString, bool)>,
    pub submit_label: SharedString,
    pub danger: bool,
    pub width: f32,
    pub links: Vec<(SharedString, Rc<dyn Fn(&mut App)>)>,
    pub on_submit: Submit,
}

impl FormSpec {
    pub fn new(title: impl Into<SharedString>, submit_label: impl Into<SharedString>, on_submit: impl Fn(Values, &mut App) -> Task<Result<(), String>> + 'static) -> Self {
        Self {
            title: title.into(),
            description: None,
            fields: Vec::new(),
            checks: Vec::new(),
            submit_label: submit_label.into(),
            danger: false,
            width: 420.,
            links: Vec::new(),
            on_submit: Rc::new(on_submit),
        }
    }
    pub fn description(mut self, d: impl Into<SharedString>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn field(mut self, f: Field) -> Self {
        self.fields.push(f);
        self
    }
    pub fn check(mut self, label: impl Into<SharedString>, value: bool) -> Self {
        self.checks.push((label.into(), value));
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    pub fn link(mut self, label: impl Into<SharedString>, f: impl Fn(&mut App) + 'static) -> Self {
        self.links.push((label.into(), Rc::new(f)));
        self
    }
    pub fn open(self, cx: &mut App) {
        let view = cx.new(|cx| FormModal::new(self, cx));
        nav::open_modal(view, cx);
    }
}

pub struct FormModal {
    spec: FormSpec,
    inputs: Vec<Entity<TextInput>>,
    checks: Vec<bool>,
    busy: bool,
    error: Option<SharedString>,
    needs_focus: bool,
    _subs: Vec<Subscription>,
    _task: Option<Task<()>>,
}

impl FormModal {
    fn new(spec: FormSpec, cx: &mut Context<Self>) -> Self {
        let mut subs = Vec::new();
        let inputs: Vec<Entity<TextInput>> = spec
            .fields
            .iter()
            .map(|f| {
                let f = f.clone();
                let input = cx.new(|cx| {
                    let i = TextInput::new(cx).placeholder(f.placeholder.clone()).with_text(f.value.clone());
                    match f.kind {
                        FieldKind::Text => i,
                        FieldKind::Password => i.masked(),
                        FieldKind::Multiline(rows) => i.multiline(rows),
                    }
                });
                subs.push(cx.subscribe(&input, |this: &mut Self, _, ev: &InputEvent, cx| {
                    if *ev == InputEvent::Submit {
                        this.submit(cx);
                    }
                }));
                input
            })
            .collect();
        let checks = spec.checks.iter().map(|(_, v)| *v).collect();
        Self { spec, inputs, checks, busy: false, error: None, needs_focus: true, _subs: subs, _task: None }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let values = Values { texts: self.inputs.iter().map(|i| i.read(cx).text().to_string()).collect(), checks: self.checks.clone() };
        self.busy = true;
        self.error = None;
        let task = (self.spec.on_submit)(values, cx);
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(()) => nav::close_modal(cx),
                    Err(e) => this.error = Some(e.into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

impl Render for FormModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.needs_focus {
            self.needs_focus = false;
            if let Some(first) = self.inputs.first() {
                first.read(cx).focus(window);
            }
        }
        let t = *cx.theme();
        let mut body = div().flex().flex_col().gap(px(12.));
        if let Some(d) = &self.spec.description {
            body = body.child(div().text_size(t.fs.normal).text_color(t.colors.text_light).child(d.clone()));
        }
        for (field, input) in self.spec.fields.iter().zip(&self.inputs) {
            body = body.child(div().flex().flex_col().gap(px(6.)).child(ui::field_label(field.label.clone(), cx)).child(input.clone()));
        }
        for (ix, (label, _)) in self.spec.checks.iter().enumerate() {
            let checked = self.checks[ix];
            body = body.child(ui::checkbox(("form-check", ix), checked, label.clone(), cx).on_click(cx.listener(move |this, _, _, cx| {
                this.checks[ix] = !this.checks[ix];
                cx.notify();
            })));
        }
        if let Some(e) = &self.error {
            body = body.child(div().text_size(t.fs.s).text_color(t.colors.red).child(e.clone()));
        }
        let label = if self.busy { SharedString::from("Working…") } else { self.spec.submit_label.clone() };
        let submit = if self.spec.danger { ui::button_danger("form-submit", label, cx) } else { ui::button_main("form-submit", label, cx) };
        let links: Vec<AnyElement> = self
            .spec
            .links
            .iter()
            .enumerate()
            .map(|(ix, (label, f))| {
                let f = f.clone();
                div()
                    .id(("form-link", ix))
                    .text_size(t.fs.s)
                    .text_color(t.colors.link)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(label.clone())
                    .on_click(move |_, _, cx| f(cx))
                    .into_any_element()
            })
            .collect();
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pt(px(4.))
                .child(div().flex().flex_col().gap(px(4.)).children(links))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(ui::button("form-cancel", "Cancel", cx).on_click(|_, _, cx| nav::close_modal(cx)))
                        .child(ui::disabled(submit, self.busy).on_click(cx.listener(|this, _, _, cx| this.submit(cx)))),
                ),
        );
        frame(self.spec.title.clone(), self.spec.width, body, cx)
    }
}

/// A yes/no confirmation; `on_confirm` runs after the user accepts.
pub fn confirm(title: impl Into<SharedString>, message: impl Into<SharedString>, confirm_label: impl Into<SharedString>, on_confirm: impl Fn(&mut App) + 'static, cx: &mut App) {
    FormSpec::new(title, confirm_label, move |_, cx| {
        on_confirm(cx);
        Task::ready(Ok(()))
    })
    .description(message)
    .danger()
    .open(cx);
}

// ---------------------------------------------------------------------------
// Login / signup
// ---------------------------------------------------------------------------

pub fn open_login(cx: &mut App) {
    FormSpec::new("Log in", "Log in", |v, cx| {
        let (user, pass) = (v.texts[0].clone(), v.texts[1].clone());
        if user.trim().is_empty() || pass.is_empty() {
            return Task::ready(Err("Enter your username and password.".into()));
        }
        Session::entity(cx).update(cx, |s, cx| s.login(user, pass, cx))
    })
    .field(Field::text("Username", "").placeholder("Username"))
    .field(Field::password("Password").placeholder("Password"))
    .link("Don't have an account? Sign up", |cx| open_signup(cx))
    .link("Forgot password? (opens website)", |cx| {
        let url = crate::state::AppState::client(cx).web_url("/forgot-password");
        cx.open_url(&url)
    })
    .open(cx);
}

pub fn open_signup(cx: &mut App) {
    if Session::get(cx).signups_disabled() {
        nav::toast("Sign-ups are currently disabled on this instance.", cx);
        return;
    }
    FormSpec::new("Create account", "Sign up", |v, cx| {
        let (user, email, pass, repeat) = (v.texts[0].clone(), v.texts[1].clone(), v.texts[2].clone(), v.texts[3].clone());
        if user.trim().is_empty() || pass.is_empty() {
            return Task::ready(Err("Choose a username and password.".into()));
        }
        if pass != repeat {
            return Task::ready(Err("Passwords don't match.".into()));
        }
        Session::entity(cx).update(cx, |s, cx| s.signup(user, email, pass, None, cx))
    })
    .description("Discuit may require a captcha for new accounts. If sign-up fails, create the account on the website and log in here.")
    .field(Field::text("Username", "").placeholder("3–21 letters, numbers or _"))
    .field(Field::text("Email (optional)", "").placeholder("you@example.com"))
    .field(Field::password("Password"))
    .field(Field::password("Repeat password"))
    .link("Already have an account? Log in", |cx| open_login(cx))
    .link("Sign up on the website instead", |cx| {
        let url = crate::state::AppState::client(cx).web_url("/");
        cx.open_url(&url)
    })
    .open(cx);
}

// ---------------------------------------------------------------------------
// Edits
// ---------------------------------------------------------------------------

pub fn open_edit_post(post: &Post, cx: &mut App) {
    let old = post.clone();
    let is_text = post.kind == discuit_api::PostType::Text;
    let mut spec = FormSpec::new("Edit post", "Save", move |v, cx| {
        let title = v.texts[0].trim().to_string();
        let body = v.texts.get(1).cloned();
        let public_id = old.public_id.clone();
        let old = old.clone();
        let task = api_task(cx, move |c| c.edit_post(&public_id, &title, body.as_deref()));
        cx.spawn(async move |cx| {
            let p = task.await?;
            cx.update(|cx| bus::emit(ContentEvent::PostUpdated(Box::new(crate::actions::merge_post(p, &old))), cx)).ok();
            Ok(())
        })
    })
    .width(620.)
    .field(Field::text("Title", post.title.clone()));
    if is_text {
        spec = spec.field(Field::multiline("Body (markdown)", post.body.clone().unwrap_or_default(), 8));
    }
    spec.open(cx);
}

pub fn open_edit_comment(comment: &Comment, cx: &mut App) {
    let old = comment.clone();
    FormSpec::new("Edit comment", "Save", move |v, cx| {
        let body = v.texts[0].clone();
        if body.trim().is_empty() {
            return Task::ready(Err("Comment can't be empty.".into()));
        }
        let (pid, id) = (old.post_public_id.clone().unwrap_or_default(), old.id.clone());
        let author = old.author.clone();
        let task = api_task(cx, move |c| c.edit_comment(&pid, &id, &body));
        cx.spawn(async move |cx| {
            let mut c = task.await?;
            if c.author.is_none() {
                c.author = author;
            }
            cx.update(|cx| bus::emit(ContentEvent::CommentUpdated(Box::new(c)), cx)).ok();
            Ok(())
        })
    })
    .width(620.)
    .field(Field::multiline("Comment (markdown)", comment.body.clone(), 6))
    .open(cx);
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

pub fn open_report(kind: ContentType, target_id: String, cx: &mut App) {
    if !crate::actions::require_login(cx) {
        return;
    }
    let view = cx.new(|_| ReportModal { kind, target_id, reason: None, busy: false, error: None, _task: None });
    nav::open_modal(view, cx);
}

pub struct ReportModal {
    kind: ContentType,
    target_id: String,
    reason: Option<i64>,
    busy: bool,
    error: Option<SharedString>,
    _task: Option<Task<()>>,
}

impl Render for ReportModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let reasons = Session::get(cx).report_reasons().to_vec();
        let mut body = div().flex().flex_col().gap(px(8.)).child(
            div().text_size(t.fs.normal).text_color(t.colors.text_light).child("Why are you reporting this? Moderators will review it."),
        );
        for r in reasons {
            let selected = self.reason == Some(r.id);
            let id = r.id;
            body = body.child(
                div()
                    .id(("reason", r.id as usize))
                    .p(px(10.))
                    .rounded(t.metrics.radius)
                    .border_1()
                    .border_color(if selected { t.colors.brand } else { t.colors.card_border })
                    .bg(if selected { t.colors.brand.opacity(0.08) } else { gpui::transparent_black() })
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text).child(r.title.clone()))
                    .when_some(r.description.clone(), |d, desc| d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(desc)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reason = Some(id);
                        cx.notify();
                    })),
            );
        }
        if let Some(e) = &self.error {
            body = body.child(div().text_color(t.colors.red).text_size(t.fs.s).child(e.clone()));
        }
        let can_submit = self.reason.is_some() && !self.busy;
        body = body.child(
            div().flex().justify_end().gap(px(8.)).pt(px(6.)).child(ui::button("report-cancel", "Cancel", cx).on_click(|_, _, cx| nav::close_modal(cx))).child(
                ui::disabled(ui::button_danger("report-submit", if self.busy { "Reporting…" } else { "Report" }, cx), !can_submit).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let Some(reason) = this.reason else { return };
                        if this.busy {
                            return;
                        }
                        this.busy = true;
                        let (kind, id) = (this.kind, this.target_id.clone());
                        this._task = Some(api_view(cx, move |c| c.report(kind, &id, reason), |this, r, cx| {
                            this.busy = false;
                            match r {
                                Ok(_) => {
                                    nav::close_modal(cx);
                                    nav::toast("Thanks — your report was sent.", cx);
                                }
                                Err(e) if e.code() == Some("already-voted") => {
                                    nav::close_modal(cx);
                                    nav::toast("You've already reported this.", cx);
                                }
                                Err(e) => this.error = Some(e.user_message().into()),
                            }
                        }));
                        cx.notify();
                    },
                )),
            ),
        );
        frame(if self.kind == ContentType::Post { "Report post" } else { "Report comment" }, 460., body, cx)
    }
}

// ---------------------------------------------------------------------------
// Save to list
// ---------------------------------------------------------------------------

pub fn open_save_to_list(kind: ContentType, target_id: String, cx: &mut App) {
    let view = cx.new(|cx| {
        let mut m = SaveToListModal { kind, target_id, lists: None, saved_in: HashSet::new(), busy: HashSet::new(), error: None, new_list: cx.new(|cx| TextInput::new(cx).placeholder("New list name")), _tasks: Vec::new() };
        m.load(cx);
        m
    });
    nav::open_modal(view, cx);
}

pub struct SaveToListModal {
    kind: ContentType,
    target_id: String,
    lists: Option<Vec<List>>,
    saved_in: HashSet<i64>,
    busy: HashSet<i64>,
    error: Option<SharedString>,
    new_list: Entity<TextInput>,
    _tasks: Vec<Task<()>>,
}

impl SaveToListModal {
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(username) = Session::get(cx).username().map(str::to_string) else { return };
        let (kind, id) = (self.kind, self.target_id.clone());
        self._tasks.push(api_view(
            cx,
            move |c| Ok((c.user_lists(&username, None, None)?, c.saved_to(&id, kind)?)),
            |this, r, _| match r {
                Ok((lists, saved)) => {
                    this.lists = Some(lists);
                    this.saved_in = saved.into_iter().collect();
                }
                Err(e) => this.error = Some(e.user_message().into()),
            },
        ));
    }

    fn toggle(&mut self, list_id: i64, cx: &mut Context<Self>) {
        if !self.busy.insert(list_id) {
            return;
        }
        let saved = self.saved_in.contains(&list_id);
        let (kind, id) = (self.kind, self.target_id.clone());
        self._tasks.push(api_view(
            cx,
            move |c| if saved { c.remove_list_item(list_id, kind, &id) } else { c.add_list_item(list_id, kind, &id) },
            move |this, r, _| {
                this.busy.remove(&list_id);
                match r {
                    Ok(_) => {
                        if saved {
                            this.saved_in.remove(&list_id);
                        } else {
                            this.saved_in.insert(list_id);
                        }
                    }
                    Err(e) => this.error = Some(e.user_message().into()),
                }
            },
        ));
        cx.notify();
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let name = self.new_list.read(cx).text().trim().to_string();
        let Some(username) = Session::get(cx).username().map(str::to_string) else { return };
        if name.is_empty() {
            return;
        }
        let slug: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let list = NewList { name: slug, display_name: Some(name), description: None, public: false };
        self._tasks.push(api_view(cx, move |c| c.create_list(&username, &list), |this, r, cx| match r {
            Ok(lists) => {
                this.lists = Some(lists);
                this.new_list.update(cx, |i, cx| i.clear(cx));
            }
            Err(e) => this.error = Some(e.user_message().into()),
        }));
    }
}

impl Render for SaveToListModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let mut body = div().flex().flex_col().gap(px(10.));
        match &self.lists {
            None => body = body.child(ui::notice("Loading lists…", cx)),
            Some(lists) if lists.is_empty() => body = body.child(ui::notice("You don't have any lists yet.", cx)),
            Some(lists) => {
                for l in lists.clone() {
                    let checked = self.saved_in.contains(&l.id);
                    let id = l.id;
                    let label = format!("{}{}", l.title(), if self.busy.contains(&l.id) { " …" } else { "" });
                    body = body.child(ui::checkbox(("list", l.id as usize), checked, label, cx).on_click(cx.listener(move |this, _, _, cx| this.toggle(id, cx))));
                }
            }
        }
        body = body.child(
            div()
                .flex()
                .gap(px(8.))
                .items_center()
                .child(div().flex_1().child(self.new_list.clone()))
                .child(ui::button("list-create", "Create", cx).on_click(cx.listener(|this, _, _, cx| this.create(cx)))),
        );
        if let Some(e) = &self.error {
            body = body.child(div().text_color(t.colors.red).text_size(t.fs.s).child(e.clone()));
        }
        body = body.child(div().flex().justify_end().child(ui::button_main("lists-done", "Done", cx).on_click(|_, _, cx| nav::close_modal(cx))));
        frame("Save to list", 400., body, cx)
    }
}
