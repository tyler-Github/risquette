//! Create a post: text, image (with uploads) or link.

use crate::{
    icons::{self, Icon},
    markdown,
    input::{InputEvent, TextInput},
    nav::{self, Target},
    session::Session,
    state::{api_view, image_from_bytes},
    ui,
};
use crate::community::CommunityAside;
use discuit_api::{Community, NewPost, NewPostImage, PostType, UserGroup};
use gpui::{
    deferred, div, img, prelude::*, px, Context, Entity, ExternalPaths, FontWeight, ObjectFit,
    PathPromptOptions, SharedString, Subscription, Task, Window,
};
use risquette_theme::ActiveTheme;
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Upload {
    /// Stable id, so finishing uploads update the right tile even after removals.
    key: u64,
    /// Local preview.
    preview: Option<Arc<gpui::Image>>,
    file_name: String,
    /// Server image id once uploaded.
    image_id: Option<String>,
    error: Option<String>,
    caption: Entity<TextInput>,
}

pub struct NewPostView {
    kind: PostType,
    community: Entity<TextInput>,
    suggestions: Vec<Community>,
    show_suggestions: bool,
    title: Entity<TextInput>,
    body: Entity<TextInput>,
    url: Entity<TextInput>,
    uploads: Vec<Upload>,
    as_mod: bool,
    busy: bool,
    error: Option<SharedString>,
    search_gen: u64,
    /// Set while we change the community text ourselves, so it doesn't re-open suggestions.
    suppress_search: bool,
    /// The chosen community's side panel (about + rules).
    aside: Option<Entity<CommunityAside>>,
    aside_for: String,
    /// The resolved community (shown as a chip).
    chosen: Option<Community>,
    /// Show the rendered markdown instead of the editor.
    preview: bool,
    next_key: u64,
    _subs: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl NewPostView {
    pub fn new(community: Option<String>, cx: &mut Context<Self>) -> Self {
        let community_input = cx.new(|cx| TextInput::new(cx).placeholder("Community name").with_text(community.clone().unwrap_or_default()));
        let title = cx.new(|cx| TextInput::new(cx).placeholder("Title").max_len(255));
        let body = cx.new(|cx| TextInput::new(cx).multiline(10).placeholder("Body (markdown supported)"));
        let url = cx.new(|cx| TextInput::new(cx).icon(Icon::Link).placeholder("https://example.com"));
        let mut subs = Vec::new();
        subs.push(cx.subscribe(&community_input, |this: &mut Self, _, ev: &InputEvent, cx| {
            if *ev == InputEvent::Changed {
                if std::mem::take(&mut this.suppress_search) {
                    return;
                }
                this.chosen = None;
                this.search_communities(cx);
            }
        }));
        let mut this = Self {
            kind: PostType::Text,
            community: community_input,
            suggestions: Vec::new(),
            show_suggestions: false,
            title,
            body,
            url,
            uploads: Vec::new(),
            as_mod: false,
            busy: false,
            error: None,
            search_gen: 0,
            suppress_search: false,
            aside: None,
            aside_for: String::new(),
            chosen: None,
            preview: false,
            next_key: 0,
            _subs: subs,
            _tasks: Vec::new(),
        };
        if let Some(name) = community {
            this.load_aside(name, cx);
        }
        this
    }

    fn search_communities(&mut self, cx: &mut Context<Self>) {
        self.search_gen += 1;
        let generation = self.search_gen;
        let q = self.community.read(cx).text().trim().to_string();
        if q.is_empty() {
            self.suggestions.clear();
            self.show_suggestions = false;
            cx.notify();
            return;
        }
        let client = crate::state::AppState::client(cx);
        self._tasks.push(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let still_current = this.update(cx, |this, _| this.search_gen == generation).unwrap_or(false);
            if !still_current {
                return;
            }
            let result = cx.background_executor().spawn(async move { client.communities(Some("all"), Some(&q), None, Some(8)) }).await;
            this.update(cx, |this, cx| {
                if this.search_gen != generation {
                    return;
                }
                if let Ok(list) = result {
                    this.suggestions = list;
                    this.show_suggestions = !this.suggestions.is_empty();
                    let typed = this.community.read(cx).text().trim().to_string();
                    if let Some(exact) = this.suggestions.iter().find(|c| c.name.eq_ignore_ascii_case(&typed)).map(|c| c.name.clone()) {
                        this.load_aside(exact, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn pick_community(&mut self, name: String, cx: &mut Context<Self>) {
        self.search_gen += 1; // cancel pending searches
        self.suppress_search = true;
        self.community.update(cx, |i, cx| i.set_text(name.clone(), cx));
        self.show_suggestions = false;
        self.load_aside(name, cx);
        cx.notify();
    }

    /// Show the chosen community's about/rules beside the form.
    fn load_aside(&mut self, name: String, cx: &mut Context<Self>) {
        if name.eq_ignore_ascii_case(&self.aside_for) {
            return;
        }
        self.aside_for = name.clone();
        self._tasks.push(api_view(cx, move |c| c.community_by_name(&name), |this, r, cx| {
            if let Ok(c) = r {
                if c.name.eq_ignore_ascii_case(&this.aside_for) {
                    this.chosen = Some(c.clone());
                    this.aside = Some(cx.new(|cx| CommunityAside::new(c, true, cx)));
                }
            }
        }));
    }

    fn selected_community_id(&self, cx: &Context<Self>) -> Option<String> {
        let name = self.community.read(cx).text().trim().to_lowercase();
        let session = Session::get(cx);
        session
            .communities()
            .iter()
            .chain(self.suggestions.iter())
            .chain(session.user().and_then(|u| u.modding_list.as_ref()).into_iter().flatten())
            .find(|c| c.name.to_lowercase() == name)
            .map(|c| c.id.clone())
    }

    fn clear_community(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.suppress_search = true;
        self.community.update(cx, |i, cx| i.clear(cx));
        self.community.read(cx).focus(window);
        self.chosen = None;
        self.aside = None;
        self.aside_for.clear();
        cx.notify();
    }

    fn add_images(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Add images".into()) });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update(cx, |this, cx| this.upload_paths(paths, cx)).ok();
        }));
    }

    /// Read and upload each file, adding a tile immediately and filling in its status.
    fn upload_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        const IMAGE_EXT: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "avif", "heic"];
        for path in paths {
            let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !IMAGE_EXT.contains(&ext.as_str()) {
                nav::toast(format!("{} isn't an image", path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()), cx);
                continue;
            }
            let key = self.next_key;
            self.next_key += 1;
            let file_name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
            let caption = cx.new(|cx| TextInput::new(cx).placeholder("Caption (optional)"));
            self.uploads.push(Upload { key, preview: None, file_name: file_name.clone(), image_id: None, error: None, caption });
            let client = crate::state::AppState::client(cx);
            self._tasks.push(cx.spawn(async move |this, cx| {
                let read = cx.background_executor().spawn(async move { std::fs::read(path) }).await;
                let bytes = match read {
                    Ok(b) => b,
                    Err(e) => {
                        this.update(cx, |this, cx| {
                            if let Some(u) = this.uploads.iter_mut().find(|u| u.key == key) {
                                u.error = Some(format!("Couldn't read file: {e}"));
                            }
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                };
                let preview = image_from_bytes(bytes.clone());
                this.update(cx, |this, cx| {
                    if let Some(u) = this.uploads.iter_mut().find(|u| u.key == key) {
                        u.preview = preview;
                    }
                    cx.notify();
                })
                .ok();
                let result = cx.background_executor().spawn(async move { client.upload_image(&file_name, &bytes) }).await;
                this.update(cx, |this, cx| {
                    if let Some(u) = this.uploads.iter_mut().find(|u| u.key == key) {
                        match result {
                            Ok(img) => u.image_id = Some(img.id),
                            Err(e) => u.error = Some(e.user_message()),
                        }
                    }
                    cx.notify();
                })
                .ok();
            }));
        }
        cx.notify();
    }

    /// Why the post can't be submitted yet, if anything.
    fn blocker(&self, cx: &Context<Self>) -> Option<&'static str> {
        if self.community.read(cx).text().trim().is_empty() {
            return Some("Pick a community");
        }
        if self.title.read(cx).text().trim().is_empty() {
            return Some("Add a title");
        }
        match self.kind {
            PostType::Link if self.url.read(cx).text().trim().is_empty() => Some("Add a link"),
            PostType::Image if self.uploads.iter().any(|u| u.image_id.is_none() && u.error.is_none()) => Some("Uploading…"),
            PostType::Image if !self.uploads.iter().any(|u| u.image_id.is_some()) => Some("Add an image"),
            _ => None,
        }
    }

    fn fetch_title(&mut self, cx: &mut Context<Self>) {
        let url = self.url.read(cx).text().trim().to_string();
        if url.is_empty() {
            return;
        }
        self._tasks.push(api_view(cx, move |c| c.link_info(&url), |this, r, cx| match r {
            Ok(info) => {
                if let Some(t) = info.title.filter(|t| !t.is_empty()) {
                    this.title.update(cx, |i, cx| i.set_text(t, cx));
                }
            }
            Err(e) => nav::toast(e.user_message(), cx),
        }));
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let community = self.community.read(cx).text().trim().to_string();
        let title = self.title.read(cx).text().trim().to_string();
        if community.is_empty() {
            self.error = Some("Pick a community.".into());
            cx.notify();
            return;
        }
        if title.is_empty() {
            self.error = Some("Add a title.".into());
            cx.notify();
            return;
        }
        let mut post = NewPost { kind: self.kind, title, community, ..Default::default() };
        match self.kind {
            PostType::Text => {
                let body = self.body.read(cx).text().to_string();
                post.body = (!body.trim().is_empty()).then_some(body);
            }
            PostType::Link => {
                let url = self.url.read(cx).text().trim().to_string();
                if url.is_empty() {
                    self.error = Some("Add a link.".into());
                    cx.notify();
                    return;
                }
                post.url = Some(url);
            }
            PostType::Image => {
                if self.uploads.iter().any(|u| u.image_id.is_none() && u.error.is_none()) {
                    self.error = Some("Wait for uploads to finish.".into());
                    cx.notify();
                    return;
                }
                post.images = self
                    .uploads
                    .iter()
                    .filter_map(|u| {
                        let caption = u.caption.read(cx).text().trim().to_string();
                        u.image_id.clone().map(|image_id| NewPostImage { image_id, caption: (!caption.is_empty()).then_some(caption) })
                    })
                    .collect();
                if post.images.is_empty() {
                    self.error = Some("Add at least one image.".into());
                    cx.notify();
                    return;
                }
            }
            PostType::Unknown => return,
        }
        if self.as_mod {
            let is_admin = Session::get(cx).is_admin();
            let strict = self.selected_community_id(cx).is_some_and(|id| Session::get(cx).is_mod_of_strict(&id));
            post.user_group = Some(if is_admin && !strict { UserGroup::Admins } else { UserGroup::Mods });
        }
        self.busy = true;
        self.error = None;
        self._tasks.push(api_view(cx, move |c| c.create_post(&post), |this, r, cx| {
            this.busy = false;
            match r {
                Ok(p) => nav::go(Target::Post { public_id: p.public_id }, cx),
                Err(e) => this.error = Some(e.user_message().into()),
            }
        }));
        cx.notify();
    }
}

impl NewPostView {
    fn segmented(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let seg = |kind: PostType, icon: Icon, label: &'static str, this: &Self, cx: &mut Context<Self>| {
            let active = this.kind == kind;
            div()
                .id(SharedString::from(format!("np-kind-{label}")))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .py(px(9.))
                .rounded(px(6.))
                .cursor_pointer()
                .text_size(t.fs.normal)
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                .text_color(if active { t.colors.text } else { t.colors.text_light })
                .when(active, |d| d.bg(t.colors.card).shadow_sm())
                .when(!active, |d| d.hover(|s| s.text_color(t.colors.text)))
                .child(icons::icon(icon, if active { t.colors.brand } else { t.colors.text_light }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.kind = kind;
                    this.error = None;
                    cx.notify();
                }))
        };
        div()
            .flex()
            .gap(px(4.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(t.colors.input)
            .child(seg(PostType::Text, Icon::FileText, "Text", self, cx))
            .child(seg(PostType::Image, Icon::Image, "Image", self, cx))
            .child(seg(PostType::Link, Icon::Link, "Link", self, cx))
    }

    fn community_field(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let field = div().flex().flex_col().gap(px(6.)).child(ui::field_label("Community", cx));
        if let Some(c) = &self.chosen {
            let pic = c.pro_pic.as_ref().map(|i| i.best_url(60).to_string());
            return field.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(t.metrics.radius)
                    .bg(t.colors.input)
                    .child(ui::avatar(pic.as_deref(), &c.name, px(28.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(c.name.clone()))
                            .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(format!(
                                "{}{}",
                                ui::plural(c.no_members, "member"),
                                if c.posting_restricted { " · only moderators can post" } else { "" }
                            ))),
                    )
                    .child(ui::button_clear("np-change", cx).text_size(t.fs.s).child("Change").on_click(cx.listener(|this, _, window, cx| this.clear_community(window, cx)))),
            );
        }
        // Text field with suggestions floating below it.
        let mut wrap = div().relative().child(self.community.clone());
        if self.show_suggestions && !self.suggestions.is_empty() {
            let hover = t.colors.button;
            let list = div()
                .id("np-suggestions")
                .absolute()
                .top(px(38.))
                .left_0()
                .right_0()
                .occlude()
                .py(px(4.))
                .bg(t.colors.card)
                .rounded(t.metrics.radius)
                .border_1()
                .border_color(t.colors.card_border)
                .shadow_lg()
                .flex()
                .flex_col()
                .children(self.suggestions.clone().into_iter().enumerate().map(|(ix, c)| {
                    let name = c.name.clone();
                    let pic = c.pro_pic.as_ref().map(|i| i.best_url(40).to_string());
                    div()
                        .id(("np-sugg", ix))
                        .px(px(10.))
                        .py(px(7.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(ui::avatar(pic.as_deref(), &c.name, px(22.), cx))
                        .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(c.name.clone()))
                        .child(div().text_size(t.fs.xs).text_color(t.colors.text_light).child(ui::plural(c.no_members, "member")))
                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _, cx| this.pick_community(name.clone(), cx)))
                }));
            wrap = wrap.child(deferred(list).with_priority(1));
        }
        field.child(wrap)
    }

    fn title_field(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let count = self.title.read(cx).char_count();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(ui::field_label("Title", cx))
                    .child(div().text_size(t.fs.xs).text_color(if count > 240 { t.colors.red } else { t.colors.text_light }).child(format!("{count}/255"))),
            )
            .child(self.title.clone())
    }

    fn body_editor(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let tool = |id: &'static str, icon: Option<Icon>, label: &'static str, tip: &'static str, action: fn(&mut TextInput, &mut Window, &mut Context<TextInput>), this: &Self, _cx: &mut Context<Self>| {
            let body = this.body.clone();
            let el = div()
                .id(id)
                .h(px(28.))
                .min_w(px(28.))
                .px(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .text_color(t.colors.text_light)
                .hover(|s| s.bg(t.colors.button))
                .tooltip(ui::tooltip(tip));
            let el = match icon {
                Some(i) => el.child(icons::icon(i, t.colors.text_light)),
                None => el.font_weight(FontWeight::BOLD).child(label),
            };
            el.on_click(move |_, window, cx| body.update(cx, |b, cx| action(b, window, cx)))
        };
        let tab = |id: &'static str, label: &'static str, preview: bool, this: &Self, cx: &mut Context<Self>| {
            let active = this.preview == preview;
            div()
                .id(id)
                .px(px(10.))
                .py(px(5.))
                .rounded(px(5.))
                .cursor_pointer()
                .text_size(t.fs.s)
                .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                .text_color(if active { t.colors.text } else { t.colors.text_light })
                .when(active, |d| d.bg(t.colors.button))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.preview = preview;
                    cx.notify();
                }))
        };
        let toolbar = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .when(!self.preview, |d| {
                        d.child(tool("md-b", None, "B", "Bold", |b, w, cx| b.wrap_selection("**", "**", "bold text", w, cx), self, cx))
                            .child(tool("md-i", None, "I", "Italic", |b, w, cx| b.wrap_selection("*", "*", "italic text", w, cx), self, cx))
                            .child(tool("md-link", Some(Icon::Link), "", "Link", |b, w, cx| b.wrap_selection("[", "](https://)", "link text", w, cx), self, cx))
                            .child(tool("md-quote", Some(Icon::MessageSquare), "", "Quote", |b, w, cx| b.prefix_lines("> ", w, cx), self, cx))
                            .child(tool("md-list", Some(Icon::List), "", "List", |b, w, cx| b.prefix_lines("- ", w, cx), self, cx))
                            .child(tool("md-code", None, "</>", "Code", |b, w, cx| b.wrap_selection("`", "`", "code", w, cx), self, cx))
                    }),
            )
            .child(div().flex().gap(px(2.)).child(tab("md-write", "Write", false, self, cx)).child(tab("md-preview", "Preview", true, self, cx)));
        let body_text = self.body.read(cx).text().to_string();
        let content = if self.preview {
            div()
                .min_h(px(220.))
                .p(px(12.))
                .rounded(t.metrics.radius)
                .bg(t.colors.input)
                .text_size(t.fs.l)
                .line_height(px(22.))
                .child(if body_text.trim().is_empty() {
                    div().text_color(t.colors.text_light).child("Nothing to preview yet.").into_any_element()
                } else {
                    markdown::render("np-preview", &body_text, cx)
                })
                .into_any_element()
        } else {
            self.body.clone().into_any_element()
        };
        div().flex().flex_col().gap(px(6.)).child(ui::field_label("Body (optional)", cx)).child(toolbar).child(content)
    }

    fn image_grid(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let session = Session::get(cx);
        let points = session.user().map_or(0, |u| u.points);
        let req = session.initial.as_ref().map_or(0, |i| i.image_post_submit_req_points);
        let mut grid = div().grid().grid_cols(3).gap(px(10.));
        for u in &self.uploads {
            let key = u.key;
            let (status, color): (SharedString, gpui::Hsla) = match (&u.image_id, &u.error) {
                (_, Some(e)) => (e.clone().into(), t.colors.red),
                (Some(_), None) => ("Uploaded".into(), t.colors.green),
                (None, None) => ("Uploading…".into(), t.colors.text_light),
            };
            let thumb = match &u.preview {
                // Absolutely positioned so the image keeps to its tile.
                Some(p) => img(p.clone()).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Cover).into_any_element(),
                None => div().absolute().inset_0().flex().items_center().justify_center().child(icons::sized(Icon::Image, px(28.), t.colors.text_light)).into_any_element(),
            };
            grid = grid.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .relative()
                            .h(px(150.))
                            .rounded(t.metrics.radius)
                            .overflow_hidden()
                            .bg(t.colors.input)
                            .child(thumb)
                            .child(
                                div()
                                    .id(SharedString::from(format!("np-rm-{key}")))
                                    .absolute()
                                    .top(px(6.))
                                    .right(px(6.))
                                    .size(px(26.))
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(gpui::black().opacity(0.6))
                                    .hover(|s| s.bg(gpui::black().opacity(0.85)))
                                    .cursor_pointer()
                                    .child(icons::sized(Icon::X, px(14.), gpui::white()))
                                    .tooltip(ui::tooltip("Remove"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.uploads.retain(|u| u.key != key);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(6.))
                                    .bottom(px(6.))
                                    .max_w(px(200.))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(gpui::black().opacity(0.65))
                                    .text_color(if u.error.is_some() { t.colors.red } else { gpui::white() })
                                    .text_size(t.fs.xs)
                                    .truncate()
                                    .child(status.clone()),
                            ),
                    )
                    .child(div().text_size(t.fs.xs).text_color(color).truncate().child(u.file_name.clone()))
                    .child(u.caption.clone()),
            );
        }
        // The "add" tile.
        let border = t.colors.text_light.opacity(0.4);
        let brand = t.colors.brand;
        grid = grid.child(
            div()
                .id("np-add")
                .h(px(150.))
                .rounded(t.metrics.radius)
                .border_2()
                .border_dashed()
                .border_color(border)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .cursor_pointer()
                .text_color(t.colors.text_light)
                .text_size(t.fs.s)
                .hover(move |s| s.border_color(brand).text_color(brand))
                .child(icons::sized(Icon::Upload, px(24.), t.colors.text_light))
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Add images"))
                .child(div().text_size(t.fs.xs).child("or drop files here"))
                .on_click(cx.listener(|this, _, _, cx| this.add_images(cx))),
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(ui::field_label("Images", cx))
            .when(points < req, |d| d.child(div().text_size(t.fs.s).text_color(t.colors.red).child(format!("You need {req} points to post images (you have {points})."))))
            .child(grid)
    }

    fn link_field(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let session = Session::get(cx);
        let points = session.user().map_or(0, |u| u.points);
        let req = session.initial.as_ref().map_or(0, |i| i.link_post_submit_req_points);
        let url = self.url.read(cx).text().trim().to_string();
        let embed = crate::embed::detect(&url);
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(ui::field_label("Link", cx))
            .when(points < req, |d| d.child(div().text_size(t.fs.s).text_color(t.colors.red).child(format!("You need {req} points to post links (you have {points})."))))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().flex().flex_col().child(self.url.clone()))
                    .child(ui::button_with_icon("np-fetch", Icon::RefreshCw, "Use page title", cx).on_click(cx.listener(|this, _, _, cx| this.fetch_title(cx)))),
            )
            .when_some(embed, |d, e| {
                let label = match e {
                    crate::embed::Embed::Video { provider, .. } => format!("{provider} video — will show as an embedded player"),
                    crate::embed::Embed::Image { .. } => "Image link — will show inline".to_string(),
                };
                d.child(div().flex().items_center().gap(px(6.)).text_size(t.fs.xs).text_color(t.colors.green).child(icons::sized(Icon::Check, px(12.), t.colors.green)).child(label))
            })
    }
}

impl Render for NewPostView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let session = Session::get(cx);
        let username = session.username().map(str::to_string).unwrap_or_default();
        let pic = session.user().and_then(|u| u.pro_pic.as_ref()).map(|i| i.best_url(40).to_string());
        let can_mod = self.selected_community_id(cx).is_some_and(|id| session.is_mod_of(&id));
        let blocker = self.blocker(cx);

        let kind_section = match self.kind {
            PostType::Image => self.image_grid(cx),
            PostType::Link => self.link_field(cx),
            _ => self.body_editor(cx),
        };

        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .pt(px(14.))
            .border_t_1()
            .border_color(t.colors.separator)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(t.fs.s)
                            .text_color(t.colors.text_light)
                            .child(ui::avatar(pic.as_deref(), &username, px(22.), cx))
                            .child(format!("Posting as @{username}")),
                    )
                    .when(can_mod, |d| {
                        d.child(ui::checkbox("np-asmod", self.as_mod, "as moderator", cx).on_click(cx.listener(|this, _, _, cx| {
                            this.as_mod = !this.as_mod;
                            cx.notify();
                        })))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .when_some(self.error.clone(), |d, e| d.child(div().max_w(px(320.)).text_size(t.fs.s).text_color(t.colors.red).child(e)))
                    .when(self.error.is_none(), |d| d.when_some(blocker, |d, b| d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(b))))
                    .child(ui::button("np-cancel", "Cancel", cx).on_click(|_, window, cx| window.dispatch_action(Box::new(crate::Back), cx)))
                    .child(
                        ui::disabled(ui::button_main_with_icon("np-submit", Icon::Plus, if self.busy { "Posting…" } else { "Post" }, cx), self.busy || blocker.is_some())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.blocker(cx).is_none() {
                                    this.submit(cx);
                                }
                            })),
                    ),
            );

        let form = ui::card(cx)
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(self.segmented(cx))
            .child(self.community_field(cx))
            .child(self.title_field(cx))
            .child(kind_section)
            .child(footer);

        let brand = t.colors.brand;
        let main = div()
            .id("new-post")
            .size_full()
            .overflow_y_scroll()
            // Drop image files anywhere on the form to add them.
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.kind = PostType::Image;
                this.upload_paths(paths.paths().to_vec(), cx);
            }))
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(brand.opacity(0.06)))
            .child(div().flex_none().pt(px(12.)).pb(px(16.)).child(form));
        let aside = match &self.aside {
            Some(a) => a.clone().into_any_element(),
            None => crate::community::aside_section("POSTING TIPS", cx)
                .child(div().text_size(t.fs.s).child("Pick a community to see its rules here."))
                .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child("Bodies support markdown — use the toolbar or type **bold**, *italic*, [links](https://example.com), > quotes, lists and `code`."))
                .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child("Image posts can have several images; drag files onto the form to add them."))
                .into_any_element(),
        };
        crate::layout::with_aside(main, Some(aside), false, window)
    }
}
