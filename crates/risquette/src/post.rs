//! A single post with its threaded comments, a comment composer and inline replies.

use crate::{
    actions,
    icons::{self, Icon},
    bus::{self, ContentEvent},
    input::{InputEvent, TextInput},
    markdown,
    nav::{self, Target},
    community::CommunityAside,
    layout,
    post_card::{vote_pill, COLUMN_WIDTH},
    session::Session,
    state::{api_view, AppState},
    ui,
};
use discuit_api::{
    tree::{self, CommentRow},
    Comment, Post, PostType, UserGroup,
};
use gpui::{
    div, list, prelude::*, px, AnyElement, ClipboardItem, Context, Entity, FontWeight, ListAlignment,
    ListState, ObjectFit, SharedString, Subscription, Task, Window,
};
use risquette_theme::ActiveTheme;
use std::collections::HashSet;

/// Width of one thread-guide column (also the avatar column).
const GUIDE: f32 = 22.;
/// Deeper threads stop indenting so text never gets squeezed.
const MAX_VISUAL_LEVEL: usize = 8;

/// An open reply or the top-level composer.
struct Composer {
    input: Entity<TextInput>,
    busy: bool,
    error: Option<SharedString>,
    as_mod: bool,
    _sub: Subscription,
}

pub struct PostView {
    public_id: String,
    post: Option<Post>,
    comments: Vec<Comment>,
    rows: Vec<CommentRow>,
    /// For each row, the ids of its visible ancestors (outermost first).
    ancestors: Vec<Vec<String>>,
    collapsed: HashSet<String>,
    comments_next: Option<String>,
    loading_replies: HashSet<String>,
    loading: bool,
    error: Option<SharedString>,
    list: ListState,
    composer: Composer,
    /// The comment id being replied to, and its composer.
    reply: Option<(String, Composer)>,
    aside: Option<Entity<CommunityAside>>,
    _tasks: Vec<Task<()>>,
    _subs: Vec<Subscription>,
}

impl PostView {
    pub fn new(public_id: String, cx: &mut Context<Self>) -> Self {
        let composer = Self::make_composer(None, cx);
        let subs = vec![cx.subscribe(&bus::entity(cx), |this, _, ev: &ContentEvent, cx| this.on_content(ev, cx))];
        let mut this = Self {
            public_id,
            post: None,
            comments: Vec::new(),
            rows: Vec::new(),
            ancestors: Vec::new(),
            collapsed: HashSet::new(),
            comments_next: None,
            loading_replies: HashSet::new(),
            loading: false,
            error: None,
            list: ListState::new(1, ListAlignment::Top, px(1500.)),
            composer,
            reply: None,
            aside: None,
            _tasks: Vec::new(),
            _subs: subs,
        };
        this.load(cx);
        this
    }

    fn make_composer(parent: Option<String>, cx: &mut Context<Self>) -> Composer {
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .multiline(if parent.is_some() { 3 } else { 4 })
                .placeholder(if parent.is_some() { "Write a reply… (Ctrl+Enter to send)" } else { "Add a comment… (markdown, Ctrl+Enter to send)" })
        });
        let sub = cx.subscribe(&input, move |this: &mut Self, _, ev: &InputEvent, cx| {
            if *ev == InputEvent::Submit {
                this.submit_comment(parent.clone(), cx);
            }
        });
        Composer { input, busy: false, error: None, as_mod: false, _sub: sub }
    }

    pub fn title(&self) -> SharedString {
        self.post.as_ref().map(|p| p.title.clone().into()).unwrap_or_else(|| "Post".into())
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        let id = self.public_id.clone();
        self._tasks.push(api_view(cx, move |c| c.post(&id), |this, result, cx| {
            this.loading = false;
            match result {
                Ok(mut post) => {
                    if let Some(c) = post.community.clone() {
                        this.aside = Some(cx.new(|cx| CommunityAside::new(c, true, cx)));
                    }
                    this.comments = post.comments.take().unwrap_or_default();
                    this.comments_next = post.comments_next.take();
                    this.post = Some(post);
                }
                Err(err) => {
                    log::error!("post {}: {err}", this.public_id);
                    this.error = Some(err.user_message().into());
                }
            }
            this.rebuild_rows();
        }));
    }

    fn on_content(&mut self, ev: &ContentEvent, cx: &mut Context<Self>) {
        match ev {
            ContentEvent::PostUpdated(p) => {
                if let Some(old) = &self.post {
                    if old.id == p.id {
                        self.post = Some(actions::merge_post((**p).clone(), old));
                        cx.notify();
                    }
                }
            }
            ContentEvent::CommentUpdated(c) => {
                if let Some(old) = self.comments.iter_mut().find(|o| o.id == c.id) {
                    let author = old.author.clone();
                    *old = (**c).clone();
                    if old.author.is_none() {
                        old.author = author;
                    }
                    self.rebuild_rows();
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    fn load_more_comments(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.comments_next.clone() else { return };
        if self.loading {
            return;
        }
        self.loading = true;
        let id = self.public_id.clone();
        self._tasks.push(api_view(cx, move |c| c.comments(&id, Some(&next)), |this, result, _| {
            this.loading = false;
            match result {
                Ok(page) => {
                    this.merge(page.comments.unwrap_or_default());
                    this.comments_next = page.next;
                }
                Err(err) => this.error = Some(err.user_message().into()),
            }
            this.rebuild_rows();
        }));
        cx.notify();
    }

    fn load_replies(&mut self, comment_id: String, cx: &mut Context<Self>) {
        if !self.loading_replies.insert(comment_id.clone()) {
            return;
        }
        let id = self.public_id.clone();
        let parent = comment_id.clone();
        self._tasks.push(api_view(cx, move |c| c.comment_replies(&id, &parent), move |this, result, _| {
            this.loading_replies.remove(&comment_id);
            match result {
                Ok(replies) => this.merge(replies),
                Err(err) => log::error!("replies for {comment_id}: {err}"),
            }
            this.rebuild_rows();
        }));
        cx.notify();
    }

    fn merge(&mut self, new: Vec<Comment>) {
        let known: HashSet<String> = self.comments.iter().map(|c| c.id.clone()).collect();
        self.comments.extend(new.into_iter().filter(|c| !known.contains(&c.id)));
    }

    fn toggle_collapse(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        let forest = tree::build_tree(self.comments.clone());
        self.rows = tree::flatten(&forest, &self.collapsed);
        // Rows are depth-first, so a stack of ids by level gives each row's ancestors.
        let mut stack: Vec<String> = Vec::new();
        self.ancestors = self
            .rows
            .iter()
            .map(|r| {
                stack.truncate(r.level);
                let ancestors = stack.clone();
                stack.push(r.comment.id.clone());
                ancestors
            })
            .collect();
        // Items: post, composer/header, each row, footer. Keep the scroll position.
        let top = self.list.logical_scroll_top();
        self.list.reset(self.rows.len() + 3);
        self.list.scroll_to(top);
    }

    fn open_reply(&mut self, comment_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !actions::require_login(cx) {
            return;
        }
        if self.reply.as_ref().is_some_and(|(id, _)| id == &comment_id) {
            self.reply = None;
        } else {
            let composer = Self::make_composer(Some(comment_id.clone()), cx);
            composer.input.read(cx).focus(window);
            self.reply = Some((comment_id, composer));
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn submit_comment(&mut self, parent: Option<String>, cx: &mut Context<Self>) {
        if !actions::require_login(cx) {
            return;
        }
        let mod_group = if Session::get(cx).is_admin() && !self.is_strict_mod(cx) { UserGroup::Admins } else { UserGroup::Mods };
        let composer = if parent.is_some() {
            match self.reply.as_mut() {
                Some((_, c)) => c,
                None => return,
            }
        } else {
            &mut self.composer
        };
        let body = composer.input.read(cx).text().to_string();
        if body.trim().is_empty() || composer.busy {
            return;
        }
        composer.busy = true;
        composer.error = None;
        let group = composer.as_mod.then_some(mod_group);
        let public_id = self.public_id.clone();
        let parent_for_call = parent.clone();
        self._tasks.push(api_view(
            cx,
            move |c| c.add_comment(&public_id, parent_for_call.as_deref(), &body, group),
            move |this, result, cx| {
                let composer = match parent {
                    Some(_) => this.reply.as_mut().map(|(_, c)| c),
                    None => Some(&mut this.composer),
                };
                match result {
                    Ok(comment) => {
                        if let Some(c) = composer {
                            c.busy = false;
                            c.input.update(cx, |i, cx| i.clear(cx));
                        }
                        if parent.is_some() {
                            this.reply = None;
                        }
                        if let Some(p) = this.post.as_mut() {
                            p.no_comments += 1;
                        }
                        this.comments.push(comment.clone());
                        this.rebuild_rows();
                        bus::emit(ContentEvent::CommentAdded(Box::new(comment)), cx);
                    }
                    Err(e) => {
                        if let Some(c) = composer {
                            c.busy = false;
                            c.error = Some(e.user_message().into());
                        }
                    }
                }
            },
        ));
        cx.notify();
    }

    fn is_strict_mod(&self, cx: &Context<Self>) -> bool {
        self.post.as_ref().is_some_and(|p| Session::get(cx).is_mod_of_strict(&p.community_id))
    }

    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let inner = match ix {
            0 => self.render_post(cx),
            1 => self.render_composer_section(cx),
            i if i - 2 < self.rows.len() => self.render_comment(i - 2, window, cx),
            _ => self.render_footer(cx),
        };
        div().w_full().child(inner).into_any_element()
    }

    fn render_post(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let Some(post) = self.post.clone() else {
            let msg: SharedString = match &self.error {
                Some(e) => format!("Couldn't load this post: {e}").into(),
                None => "Loading…".into(),
            };
            return ui::card(cx)
                .mt(t.metrics.gap)
                .p(t.metrics.gap)
                .text_color(t.colors.text_light)
                .child(msg)
                .when(self.error.is_some(), |d| d.child(ui::button("post-retry", "Retry", cx).mt(px(10.)).on_click(cx.listener(|this, _, _, cx| this.load(cx)))))
                .into_any_element();
        };

        let community = post.community_name.clone();
        let pro_pic = post.community_pro_pic.as_ref().map(|i| i.best_url(50).to_string());
        let author = post.username.clone();
        let header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .text_size(t.fs.s)
            .text_color(t.colors.text_light)
            .child(ui::avatar(pro_pic.as_deref(), &post.community_name, px(28.), cx))
            .child(
                div()
                    .id("post-community")
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.colors.text)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(community.clone())
                    .on_click(move |_, _, cx| nav::go(Target::Community(community.clone()), cx)),
            )
            .child("·")
            .child("By")
            .child(
                div()
                    .id("post-author")
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(format!("@{}", post.username))
                    .on_click(move |_, _, cx| nav::go(Target::User(author.clone()), cx)),
            )
            .when(post.user_group.as_deref() == Some("mods"), |d| d.child(div().text_color(t.colors.green).font_weight(FontWeight::BOLD).child("MOD")))
            .when(post.user_group.as_deref() == Some("admins"), |d| d.child(div().text_color(t.colors.red).font_weight(FontWeight::BOLD).child("ADMIN")))
            .child("·")
            .child(ui::time_ago(post.created_at))
            .when(post.edited_at.is_some(), |d| d.child("· edited"))
            .when(post.locked, |d| d.child(div().text_color(t.colors.red).child("· Locked")))
            .when(post.deleted, |d| d.child(div().text_color(t.colors.red).child(format!("· Deleted{}", post.deleted_as.as_deref().map(|a| format!(" by {a}")).unwrap_or_default()))));

        let mut body = div().flex().flex_col().gap(px(12.)).child(header).child(
            div()
                .text_size(t.fs.xxl - px(3.))
                .font_weight(FontWeight::BOLD)
                .text_color(t.colors.text)
                .line_height(px(30.))
                .child(post.title.clone()),
        );

        match post.kind {
            PostType::Link => {
                if let Some(link) = &post.link {
                    let url = link.url.clone();
                    let host = link.hostname.clone().unwrap_or_else(|| url.clone());
                    body = body.child(
                        div()
                            .id("post-link")
                            .text_size(t.fs.normal)
                            .text_color(t.colors.link)
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(host)
                            .child(icons::sized(Icon::ExternalLink, px(13.), t.colors.link))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    );
                    match crate::post_card::embed_view("post", &link.url, &post.title, link.image.as_ref(), cx) {
                        Some(embed) => body = body.child(embed),
                        None => {
                            if let Some(image) = &link.image {
                                body = body.child(self.render_image(image, 360., None, cx));
                            }
                        }
                    }
                }
            }
            PostType::Image => {
                let images = crate::post_card::post_images(&post);
                if !images.is_empty() {
                    body = body.child(crate::post_card::carousel("post-car", &post.id, &images, 700., true, cx));
                }
            }
            _ => {}
        }
        if let Some(text) = post.body.as_deref().filter(|b| !b.trim().is_empty()) {
            body = body.child(div().text_size(t.fs.l).line_height(px(22.)).child(markdown::render("post-body", text, cx)));
            // Embed the first video/image link found in a text post's body.
            if let Some((link, _)) = crate::embed::first_in_text(text) {
                body = body.children(crate::post_card::embed_view("post-body-embed", &link, &post.title, None, cx));
            }
        }

        let vote_post = post.clone();
        let menu_post = post.clone();
        let web = AppState::client(cx).post_web_url(&post.community_name, &post.public_id);
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(t.fs.normal)
                .text_color(t.colors.text_light)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(vote_pill("post-vote", post.points(), post.vote(), move |up, cx| actions::vote_post(&vote_post, up, cx), cx))
                        .child(div().px(px(10.)).py(px(4.)).child(format!("{} upvotes · {} downvotes", post.upvotes, post.downvotes))),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(ui::button_clear("post-share", cx).child(icons::icon(Icon::Link, t.colors.text_light)).child("Copy link").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(web.clone()));
                            nav::toast("Link copied", cx);
                        }))
                        .child(ui::menu_button("post-menu", move |cx| actions::post_menu(&menu_post, cx), cx)),
                ),
        );

        ui::card(cx).mt(t.metrics.gap).p(t.metrics.gap + px(5.)).child(body).into_any_element()
    }

    fn render_image(&self, image: &discuit_api::Image, max_h: f32, caption: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let url = image.best_url(COLUMN_WIDTH as u32).to_string();
        let full = AppState::client(cx).absolute_url(&image.url).map(|u| u.to_string()).unwrap_or_default();
        let ph = ui::placeholder_color(image, cx);
        let h = if image.width > 0 { (COLUMN_WIDTH * image.height as f32 / image.width as f32).clamp(100., max_h) } else { 300. };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .id(SharedString::from(format!("img-{}", image.id)))
                    .w_full()
                    .h(px(h))
                    .rounded(t.metrics.radius)
                    .bg(t.colors.skeleton)
                    .cursor_pointer()
                    .child(ui::remote_image(&url, ObjectFit::Contain, t.metrics.radius, ph, cx))
                    .on_click(move |_, _, cx| cx.open_url(&full)),
            )
            .when_some(caption.or(image.alt_text.clone()).filter(|c| !c.is_empty()), |d, c| {
                d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(c))
            })
            .into_any_element()
    }

    fn render_composer(&self, composer: &Composer, id: &str, parent: Option<String>, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let can_mod = self.post.as_ref().is_some_and(|p| Session::get(cx).is_mod_of(&p.community_id));
        let as_mod = composer.as_mod;
        let parent_toggle = parent.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(composer.input.clone())
            .when_some(composer.error.clone(), |d, e| d.child(div().text_size(t.fs.s).text_color(t.colors.red).child(e)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().when(can_mod, |d| {
                        d.child(ui::checkbox(SharedString::from(format!("{id}-asmod")), as_mod, "Comment as moderator", cx).on_click(cx.listener(
                            move |this, _, _, cx| {
                                let c = match &parent_toggle {
                                    Some(_) => this.reply.as_mut().map(|(_, c)| c),
                                    None => Some(&mut this.composer),
                                };
                                if let Some(c) = c {
                                    c.as_mod = !c.as_mod;
                                }
                                cx.notify();
                            },
                        )))
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .when(parent.is_some(), |d| {
                                d.child(ui::button(SharedString::from(format!("{id}-cancel")), "Cancel", cx).on_click(cx.listener(|this, _, _, cx| {
                                    this.reply = None;
                                    this.rebuild_rows();
                                    cx.notify();
                                })))
                            })
                            .child(
                                ui::disabled(ui::button_main(SharedString::from(format!("{id}-send")), if composer.busy { "Sending…" } else if parent.is_some() { "Reply" } else { "Comment" }, cx), composer.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| this.submit_comment(parent.clone(), cx))),
                            ),
                    ),
            )
    }

    fn render_composer_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let Some(post) = &self.post else { return div().into_any_element() };
        let n = post.no_comments;
        let locked = post.locked;
        let logged_in = Session::get(cx).is_logged_in();
        let is_mod = Session::get(cx).is_mod_of(&post.community_id);
        let mut section = div().pt(t.metrics.gap).flex().flex_col().gap(px(10.));
        if locked && !is_mod {
            section = section.child(ui::card(cx).p(t.metrics.gap).text_color(t.colors.text_light).child("This post is locked. New comments are disabled."));
        } else if logged_in {
            let composer = self.render_composer(&self.composer, "top", None, cx);
            section = section.child(ui::card(cx).p(t.metrics.gap).child(composer));
        } else {
            section = section.child(
                ui::card(cx)
                    .p(t.metrics.gap)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_color(t.colors.text_light).child("Log in to join the conversation."))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(ui::button_main("c-login", "Log in", cx).on_click(|_, _, cx| crate::modals::open_login(cx))),
                    ),
            );
        }
        section
            .child(div().pt(px(4.)).text_size(t.fs.l).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text).child(ui::plural(n, "comment")))
            .into_any_element()
    }

    fn render_comment(&mut self, row_ix: usize, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let row = self.rows[row_ix].clone();
        let ancestors = self.ancestors.get(row_ix).cloned().unwrap_or_default();
        let c = &row.comment;
        let id = c.id.clone();
        let post = self.post.clone().unwrap_or_default();
        let is_op = c.user_id.as_deref() == Some(post.user_id.as_str());
        let is_me = Session::get(cx).user().is_some_and(|u| Some(&u.id) == c.user_id.as_ref());
        let next_level = self.rows.get(row_ix + 1).map(|r| r.level);
        let has_visible_children = !row.collapsed && next_level.is_some_and(|l| l > row.level);
        let first_in_thread = row.level == 0;
        let last_in_thread = next_level.is_none_or(|l| l == 0);
        let group: SharedString = format!("c-{id}").into();

        let line = t.colors.comment_line;
        let brand = t.colors.brand;

        // One guide per ancestor; clicking a guide collapses that ancestor's thread.
        let guides = ancestors.iter().take(MAX_VISUAL_LEVEL).enumerate().map(|(depth, ancestor)| {
            let target = ancestor.clone();
            div()
                .id(SharedString::from(format!("g-{id}-{depth}")))
                .w(px(GUIDE))
                .flex_none()
                .flex()
                .justify_center()
                .cursor_pointer()
                .child(div().w(px(2.)).h_full().bg(line))
                .hover(move |s| s.bg(brand.opacity(0.10)))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapse(target.clone(), cx)))
                .into_any_element()
        }).collect::<Vec<_>>();

        // Avatar column, with this comment's own line running down to its replies.
        let pro_pic = c.author.as_ref().and_then(|a| a.pro_pic.as_ref()).map(|i| i.best_url(40).to_string());
        let own_id = id.clone();
        let avatar_col = div()
            .w(px(GUIDE))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .child(ui::avatar(pro_pic.as_deref(), &c.username, px(22.), cx))
            .when(has_visible_children, |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("rail-{id}")))
                        .mt(px(4.))
                        .flex_1()
                        .w_full()
                        .flex()
                        .justify_center()
                        .cursor_pointer()
                        .child(div().w(px(2.)).h_full().bg(line))
                        .hover(move |s| s.bg(brand.opacity(0.10)))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapse(own_id.clone(), cx))),
                )
            });

        let username = c.username.clone();
        let deleted_author = c.user_deleted || (c.deleted && c.username.is_empty());
        let badge = |label: &'static str, color: gpui::Hsla| {
            div().px(px(5.)).rounded(px(3.)).bg(color.opacity(0.15)).text_color(color).text_size(t.fs.xs).font_weight(FontWeight::BOLD).child(label)
        };
        let collapse_id = id.clone();
        let header = div()
            .id(SharedString::from(format!("ch-{id}")))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .min_h(px(22.))
            .text_size(t.fs.s)
            .text_color(t.colors.text_light)
            .cursor_pointer()
            .child(
                div()
                    .id(SharedString::from(format!("cu-{id}")))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if is_op { t.colors.brand } else { t.colors.text })
                    .hover(|s| s.underline())
                    .child(if deleted_author { "[deleted]".to_string() } else { c.username.clone() })
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        nav::go(Target::User(username.clone()), cx);
                    }),
            )
            .when(is_op, |d| d.child(badge("OP", t.colors.brand)))
            .when(is_me && !is_op, |d| d.child(badge("YOU", t.colors.brand)))
            .when(c.user_group.as_deref() == Some("mods"), |d| d.child(badge("MOD", t.colors.green)))
            .when(c.user_group.as_deref() == Some("admins"), |d| d.child(badge("ADMIN", t.colors.red)))
            .child("·")
            .child(ui::time_ago(c.created_at))
            .when(c.edited_at.is_some(), |d| d.child("· edited"))
            .when(c.locked, |d| d.child(icons::sized(Icon::Lock, px(12.), t.colors.red)))
            .when(row.collapsed, |d| {
                d.child("·").child(ui::plural(c.points(), "point")).when(c.no_replies > 0, |d| {
                    d.child(div().px(px(6.)).rounded(px(8.)).bg(t.colors.input).text_size(t.fs.xs).font_weight(FontWeight::SEMIBOLD).child(format!("+{}", c.no_replies)))
                })
            })
            // Clicking the header (not the name) collapses or expands, like the web client.
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapse(collapse_id.clone(), cx)));

        let mut content = div().flex().flex_col().gap(px(4.)).flex_1().min_w_0().child(header);

        if !row.collapsed {
            let body = if c.deleted { "[This comment was deleted]".to_string() } else { c.body.clone() };
            content = content.child(
                div()
                    .text_size(t.fs.normal)
                    .line_height(px(21.))
                    .text_color(if c.deleted { t.colors.text_light } else { t.colors.text })
                    .child(markdown::render(format!("c-{id}"), &body, cx)),
            );

            let vote_c = c.clone();
            let menu_c = c.clone();
            let community_id = post.community_id.clone();
            let reply_id = id.clone();
            let can_reply = !c.deleted && !c.locked && (!post.locked || Session::get(cx).is_mod_of(&post.community_id));
            let replying_here = self.reply.as_ref().is_some_and(|(r, _)| r == &id);
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .ml(px(-6.))
                    .text_size(t.fs.s)
                    .text_color(t.colors.text_light)
                    // Quiet until the comment is hovered (or a vote is set / a reply is open).
                    .when(c.vote().is_none() && !replying_here, |d| d.opacity(0.65).group_hover(group.clone(), |s| s.opacity(1.)))
                    .child(vote_pill(format!("cv-{id}"), c.points(), c.vote(), move |up, cx| actions::vote_comment(&vote_c, up, cx), cx).bg(gpui::transparent_black()).text_size(t.fs.s))
                    .when(can_reply, |d| {
                        d.child(
                            ui::button_clear(SharedString::from(format!("reply-{id}")), cx)
                                .px(px(8.))
                                .py(px(3.))
                                .text_size(t.fs.s)
                                .text_color(t.colors.text_light)
                                .child(icons::sized(Icon::Reply, px(14.), t.colors.text_light))
                                .child("Reply")
                                .on_click(cx.listener(move |this, _, window, cx| this.open_reply(reply_id.clone(), window, cx))),
                        )
                    })
                    .child(ui::menu_button(SharedString::from(format!("cm-{id}")), move |cx| actions::comment_menu(&menu_c, &community_id, cx), cx).py(px(3.))),
            );

            if let Some((reply_to, composer)) = &self.reply {
                if reply_to == &id {
                    let composer = self.render_composer(composer, &format!("r-{id}"), Some(id.clone()), cx);
                    content = content.child(div().pt(px(4.)).pb(px(6.)).child(composer));
                }
            }

            if row.unloaded_replies > 0 {
                let loading = self.loading_replies.contains(&id);
                let rid = id.clone();
                content = content.child(
                    div()
                        .id(SharedString::from(format!("more-{id}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .py(px(2.))
                        .text_size(t.fs.s)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.colors.link)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .child(icons::sized(Icon::MessageSquare, px(14.), t.colors.link))
                        .child(if loading { "Loading…".to_string() } else { format!("Show {}", ui::plural(row.unloaded_replies, "more reply")) })
                        .on_click(cx.listener(move |this, _, _, cx| this.load_replies(rid.clone(), cx))),
                );
            }
        }

        div()
            .group(group)
            .bg(t.colors.card)
            .border_color(t.colors.card_ring)
            .border_l_1()
            .border_r_1()
            .when(first_in_thread, |d| d.mt(px(10.)).rounded_t(t.metrics.radius).border_t_1().pt(px(12.)))
            .when(!first_in_thread, |d| d.pt(px(6.)))
            .when(last_in_thread, |d| d.rounded_b(t.metrics.radius).border_b_1().pb(px(10.)))
            .px(px(14.))
            .flex()
            .children(guides)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(8.))
                    .child(avatar_col)
                    .child(content),
            )
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.theme();
        let base = div().py(px(20.)).flex().justify_center().text_color(t.colors.text_light).text_size(t.fs.normal);
        if self.post.is_none() {
            return base.into_any_element();
        }
        if self.loading {
            return base.child("Loading comments…").into_any_element();
        }
        if self.comments_next.is_some() {
            return base
                .child(ui::button("more-comments", "Load more comments", cx).on_click(cx.listener(|this, _, _, cx| this.load_more_comments(cx))))
                .into_any_element();
        }
        if self.rows.is_empty() {
            return base.child("No comments yet. Be the first!").into_any_element();
        }
        base.into_any_element()
    }
}

impl Render for PostView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = list(self.list.clone(), cx.processor(Self::render_row)).size_full();
        layout::with_aside(main, self.aside.clone().map(|a| a.into_any_element()), false, window)
    }
}
