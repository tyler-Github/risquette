//! Account settings: profile, preferences, password, mutes, and account deletion.

use crate::{
    icons::Icon,
    input::TextInput,
    modals::{Field, FormSpec},
    nav,
    session::Session,
    state::{api, api_task, api_view},
    ui,
};
use discuit_api::{MuteKind, Mutes, ProfileUpdate};
use gpui::{Context, Entity, PathPromptOptions, SharedString, Task, Window, div, prelude::*, px};
use risquette_theme::ActiveTheme;

pub struct SettingsView {
    profile: ProfileUpdate,
    about: Entity<TextInput>,
    current_pw: Entity<TextInput>,
    new_pw: Entity<TextInput>,
    repeat_pw: Entity<TextInput>,
    mutes: Option<Mutes>,
    saving: bool,
    status: Option<(bool, SharedString)>,
    pw_status: Option<(bool, SharedString)>,
    instance: Entity<TextInput>,
    instance_status: Option<(bool, SharedString)>,
    probing: bool,
    update_server: Entity<TextInput>,
    /// Settings → Reading: a word or phrase to add to the keyword filters.
    keyword: Entity<TextInput>,
    _tasks: Vec<Task<()>>,
    _subs: Vec<gpui::Subscription>,
}

/// Well-known instances offered as one-click choices.
const PRESET_INSTANCES: &[&str] = &[discuit_api::DEFAULT_BASE_URL];

impl SettingsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let logged_in = Session::get(cx).user().is_some();
        let stored = crate::storage::load();
        let current = current_instance(cx);
        let instance = cx.new(|cx| TextInput::new(cx).placeholder("discuit.example.com").with_text(current.trim_start_matches("https://").to_string()));
        let update_server =
            cx.new(|cx| TextInput::new(cx).placeholder(crate::updater::DEFAULT_SERVER).with_text(stored.update_server.clone().unwrap_or_default()));
        let updater = crate::updater::Updater::entity(cx);
        let keyword = cx.new(|cx| TextInput::new(cx).icon(Icon::Funnel).placeholder("Add a word or phrase").max_len(100));
        let subs = vec![
            cx.observe(&updater, |_, _, cx| cx.notify()),
            cx.observe(&Session::entity(cx), |_, _, cx| cx.notify()),
            // Enter adds the keyword.
            cx.subscribe(&keyword, |this: &mut Self, _, ev: &crate::input::InputEvent, cx| {
                if *ev == crate::input::InputEvent::Submit {
                    this.add_keyword(cx);
                }
            }),
        ];
        let user = Session::get(cx).user().cloned().unwrap_or_default();
        let profile = ProfileUpdate::from_user(&user);
        let about = cx.new(|cx| {
            TextInput::new(cx).multiline(4).placeholder("Tell people about yourself (markdown)").with_text(user.about_me.clone().unwrap_or_default())
        });
        let mut this = Self {
            profile,
            about,
            current_pw: cx.new(|cx| TextInput::new(cx).masked().placeholder("Current password")),
            new_pw: cx.new(|cx| TextInput::new(cx).masked().placeholder("New password")),
            repeat_pw: cx.new(|cx| TextInput::new(cx).masked().placeholder("Repeat new password")),
            mutes: None,
            saving: false,
            status: None,
            pw_status: None,
            instance,
            instance_status: None,
            probing: false,
            update_server,
            keyword,
            _tasks: Vec::new(),
            _subs: subs,
        };
        if logged_in {
            this._tasks.push(api_view(
                cx,
                |c| c.mutes(),
                |this, r, _| {
                    if let Ok(m) = r {
                        this.mutes = Some(m);
                    }
                },
            ));
        }
        this
    }

    /// Validate the typed instance (GET `/api/_initial`), then switch to it.
    fn connect_instance(&mut self, input: String, cx: &mut Context<Self>) {
        if self.probing {
            return;
        }
        let Some(base) = crate::state::normalize_instance(&input) else {
            self.instance_status = Some((false, "Enter an address like discuit.example.com".into()));
            cx.notify();
            return;
        };
        if base == current_instance(cx) {
            self.instance_status = Some((true, "Already connected.".into()));
            cx.notify();
            return;
        }
        self.probing = true;
        self.instance_status = Some((true, format!("Checking {}…", base.trim_start_matches("https://")).into()));
        cx.notify();
        let probe_base = base.clone();
        let task = cx.background_executor().spawn(async move { crate::state::probe_instance(&probe_base) });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.probing = false;
                match result {
                    Ok(_) => {
                        this.instance_status = None;
                        cx.defer(move |cx| crate::state::switch_instance(base, cx));
                    }
                    Err(e) => this.instance_status = Some((false, e.into())),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn save_update_server(&mut self, cx: &mut Context<Self>) {
        let raw = self.update_server.read(cx).text().trim().trim_end_matches('/').to_string();
        if !raw.is_empty() && !(raw.starts_with("https://") || raw.starts_with("http://")) {
            nav::toast("The update server must start with https://", cx);
            return;
        }
        crate::storage::update(|s| s.update_server = (!raw.is_empty()).then_some(raw));
        crate::updater::Updater::entity(cx).update(cx, |u, cx| u.check(true, cx));
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let mut profile = self.profile.clone();
        let about = self.about.read(cx).text().to_string();
        profile.about_me = (!about.trim().is_empty()).then_some(about);
        self.saving = true;
        self.status = None;
        self._tasks.push(api_view(
            cx,
            move |c| c.update_profile(&profile),
            |this, r, cx| {
                this.saving = false;
                match r {
                    Ok(user) => {
                        this.profile = ProfileUpdate::from_user(&user);
                        Session::entity(cx).update(cx, |s, cx| s.set_user(user, cx));
                        this.status = Some((true, "Saved.".into()));
                    }
                    Err(e) => this.status = Some((false, e.user_message().into())),
                }
            },
        ));
        cx.notify();
    }

    fn change_password(&mut self, cx: &mut Context<Self>) {
        let (cur, new, rep) =
            (self.current_pw.read(cx).text().to_string(), self.new_pw.read(cx).text().to_string(), self.repeat_pw.read(cx).text().to_string());
        if new != rep {
            self.pw_status = Some((false, "New passwords don't match.".into()));
            cx.notify();
            return;
        }
        if cur.is_empty() || new.is_empty() {
            self.pw_status = Some((false, "Fill in all three fields.".into()));
            cx.notify();
            return;
        }
        self._tasks.push(api_view(
            cx,
            move |c| c.change_password(&cur, &new, &rep),
            |this, r, cx| match r {
                Ok(_) => {
                    this.pw_status = Some((true, "Password changed.".into()));
                    for i in [&this.current_pw, &this.new_pw, &this.repeat_pw] {
                        i.update(cx, |i, cx| i.clear(cx));
                    }
                }
                Err(e) => this.pw_status = Some((false, e.user_message().into())),
            },
        ));
    }

    fn upload_pro_pic(&mut self, cx: &mut Context<Self>) {
        let Some(username) = Session::get(cx).username().map(str::to_string) else { return };
        let paths =
            cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Choose a profile picture".into()) });
        self._tasks.push(cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let Ok(client) = cx.update(|cx| crate::state::AppState::client(cx)) else { return };
            let name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "avatar".into());
            let result = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(&path).map_err(|e| format!("Couldn't read file: {e}"))?;
                    client.upload_user_pro_pic(&username, &name, &bytes).map_err(|e| e.user_message())
                })
                .await;
            cx.update(|cx| match result {
                Ok(user) => {
                    Session::entity(cx).update(cx, |s, cx| s.set_user(user, cx));
                    nav::toast("Profile picture updated", cx);
                }
                Err(e) => nav::toast(e, cx),
            })
            .ok();
        }));
    }

    fn remove_pro_pic(&mut self, cx: &mut Context<Self>) {
        let Some(username) = Session::get(cx).username().map(str::to_string) else { return };
        api(
            cx,
            move |c| c.delete_user_pro_pic(&username),
            |r, cx| match r {
                Ok(user) => Session::entity(cx).update(cx, |s, cx| s.set_user(user, cx)),
                Err(e) => nav::toast(e.user_message(), cx),
            },
        );
    }

    fn unmute(&mut self, mute_id: String, cx: &mut Context<Self>) {
        self._tasks.push(api_view(
            cx,
            move |c| c.unmute(&mute_id).and_then(|_| c.mutes()),
            |this, r, cx| match r {
                Ok(m) => {
                    this.mutes = Some(m.clone());
                    Session::entity(cx).update(cx, |s, cx| s.set_mutes(m, cx));
                }
                Err(e) => nav::toast(e.user_message(), cx),
            },
        ));
    }

    fn clear_mutes(&mut self, kind: MuteKind, cx: &mut Context<Self>) {
        self._tasks.push(api_view(
            cx,
            move |c| c.clear_mutes(kind).and_then(|_| c.mutes()),
            |this, r, cx| match r {
                Ok(m) => {
                    this.mutes = Some(m.clone());
                    Session::entity(cx).update(cx, |s, cx| s.set_mutes(m, cx));
                }
                Err(e) => nav::toast(e.user_message(), cx),
            },
        ));
    }

    /// Client settings that apply whatever account or instance: (instance, app, updates).
    fn app_sections(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> (gpui::Div, gpui::Div, gpui::Div) {
        use crate::updater::{self, Status, Updater};
        use risquette_theme::{Mode, Theme};
        let t = *cx.theme();
        let stored = crate::storage::load();
        let status_line = |s: &Option<(bool, SharedString)>| {
            s.clone().map(|(ok, msg)| div().text_size(t.fs.s).text_color(if ok { t.colors.text_light } else { t.colors.red }).child(msg))
        };

        // --- Instance -------------------------------------------------------
        let current = current_instance(cx);
        let session = Session::get(cx);
        let mut presets = div().flex().flex_wrap().gap(px(6.));
        for (ix, preset) in PRESET_INSTANCES.iter().enumerate() {
            let active = current == *preset;
            let p = preset.to_string();
            presets = presets.child(
                ui::button_with_icon(("s-preset", ix), Icon::Globe, preset.trim_start_matches("https://").to_string(), cx)
                    .when(active, |d| d.border_1().border_color(t.colors.brand))
                    .on_click(cx.listener(move |this, _, _, cx| this.connect_instance(p.clone(), cx))),
            );
        }
        let instance_card = section("Instance", cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded(t.metrics.radius)
                    .bg(t.colors.bg)
                    .child(crate::icons::sized(Icon::Server, px(20.), t.colors.brand))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(t.colors.text)
                                    .child(current.trim_start_matches("https://").to_string()),
                            )
                            .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(match (session.initial(), session.user()) {
                                (Some(i), Some(u)) => format!("{} members · logged in as @{}", ui::compact(i.no_users), u.username),
                                (Some(i), None) => format!("{} members · not logged in", ui::compact(i.no_users)),
                                _ => "Connecting…".to_string(),
                            })),
                    ),
            )
            .child(setting(
                "Connect to another instance",
                "Risquette works with any Discuit server. Logins are kept separately for each instance.",
                div().flex().gap(px(8.)).child(div().flex_1().min_w_0().flex().flex_col().child(self.instance.clone())).child(
                    ui::disabled(ui::button_main("s-connect", if self.probing { "Checking…" } else { "Connect" }, cx), self.probing).on_click(cx.listener(
                        |this, _, _, cx| {
                            let text = this.instance.read(cx).text().to_string();
                            this.connect_instance(text, cx);
                        },
                    )),
                ),
                cx,
            ))
            .children(status_line(&self.instance_status))
            .child(presets);

        // --- App ------------------------------------------------------------
        let theme_choice = stored.theme.clone();
        let set_theme = |choice: Option<&'static str>| {
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                crate::storage::update(|s| s.theme = choice.map(str::to_string));
                let system_dark = matches!(window.appearance(), gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark);
                let mode = Mode::from_setting(choice, system_dark);
                Theme::set_mode(mode, cx);
                cx.refresh_windows();
            }
        };
        let theme = segmented(cx)
            .child(segment("s-th-sys", Some(Icon::Monitor), "System", theme_choice.is_none(), cx).on_click(set_theme(None)))
            .child(segment("s-th-light", Some(Icon::Sun), "Light", theme_choice.as_deref() == Some("light"), cx).on_click(set_theme(Some("light"))))
            .child(segment("s-th-dark", Some(Icon::Moon), "Dark", theme_choice.as_deref() == Some("dark"), cx).on_click(set_theme(Some("dark"))));
        let startup_all = stored.startup_page.as_deref() == Some("all");
        let startup = segmented(cx)
            .child(segment("s-start-home", Some(Icon::House), "Home", !startup_all, cx).on_click(cx.listener(|_, _, _, cx| {
                crate::storage::update(|s| s.startup_page = None);
                cx.notify();
            })))
            .child(segment("s-start-all", Some(Icon::Globe), "All", startup_all, cx).on_click(cx.listener(|_, _, _, cx| {
                crate::storage::update(|s| s.startup_page = Some("all".into()));
                cx.notify();
            })));
        let app_card = section("App", cx)
            .child(setting("Theme", "Follow your system, or always use light or dark (Ctrl+Shift+L switches between them).", theme, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(ui::checkbox("s-high-contrast", stored.high_contrast, "High contrast", cx).on_click(cx.listener(|_, _, _, cx| {
                        let on = !crate::storage::load().high_contrast;
                        crate::storage::update(|s| s.high_contrast = on);
                        Theme::set_high_contrast(on, cx);
                    })))
                    .child(div().pl(px(24.)).text_size(t.fs.s).text_color(t.colors.text_light).child(
                        "Black and white with strong borders and bold accents, in light or dark. Text meets the strictest accessibility guideline (WCAG AAA).",
                    )),
            )
            .child(setting("Open on launch", "Which feed Risquette shows when it starts.", startup, cx))
            // Only Windows has the built-in player; elsewhere videos always open in the browser.
            .when(cfg!(windows), |d| {
                d.child(ui::checkbox("s-videos", stored.videos_in_browser, "Open videos in my web browser instead of the built-in player", cx).on_click(
                    cx.listener(|_, _, _, cx| {
                        crate::storage::update(|s| s.videos_in_browser = !s.videos_in_browser);
                        cx.notify();
                    }),
                ))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        ui::checkbox("s-desktop-notify", stored.desktop_notifications, "Show desktop notifications", cx)
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::storage::update(|s| s.desktop_notifications = !s.desktop_notifications);
                            crate::desktop::sync_tray(cx);
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .pl(px(24.))
                            .text_size(t.fs.s)
                            .text_color(t.colors.text_light)
                            .child(
                                if cfg!(windows) {
                                    "For new replies, mentions and other notifications, while Risquette isn't the window you're using. While this is on, Risquette keeps an icon in the system tray."
                                } else {
                                    "For new replies, mentions and other notifications, while Risquette isn't the window you're using."
                                },
                            ),
                    ),
            );

        // --- Updates --------------------------------------------------------
        let status = Updater::entity(cx).read(cx).status.clone();
        let (line, color): (String, gpui::Hsla) = match &status {
            Status::Idle => ("Not checked yet.".into(), t.colors.text_light),
            Status::Checking => ("Checking…".into(), t.colors.text_light),
            Status::UpToDate => ("You're on the newest version.".into(), t.colors.green),
            Status::Available(r) => (format!("Version {} is available.", r.version), t.colors.brand),
            Status::Downloading { done, total, .. } => {
                (format!("Downloading… {} of {}", updater::human_size(*done), updater::human_size(*total)), t.colors.brand)
            }
            Status::Ready { release, .. } => (format!("Version {} is ready to install.", release.version), t.colors.green),
            Status::Failed(e) => (e.to_string(), t.colors.red),
            Status::NotConfigured => ("No update server is configured for this build.".into(), t.colors.text_light),
        };
        let beta = updater::channel() == "beta";
        let channel = segmented(cx)
            .child(segment("s-ch-stable", None, "Stable", !beta, cx).on_click(cx.listener(|_, _, _, cx| {
                crate::storage::update(|s| s.update_channel = None);
                cx.notify();
            })))
            .child(segment("s-ch-beta", None, "Beta", beta, cx).on_click(cx.listener(|_, _, _, cx| {
                crate::storage::update(|s| s.update_channel = Some("beta".into()));
                cx.notify();
            })));
        let has_offer = Updater::entity(cx).read(cx).offered().is_some();
        let updates_card = section("Updates", cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(t.colors.text)
                                    .child(format!("Risquette {}", updater::CURRENT_VERSION)),
                            )
                            .child(div().text_size(t.fs.s).text_color(color).child(line)),
                    )
                    .child(if has_offer {
                        ui::button_main_with_icon("s-upd-open", Icon::Download, "View update", cx).on_click(|_, _, cx| updater::open_dialog(cx))
                    } else {
                        ui::button_with_icon("s-upd-check", Icon::RefreshCw, "Check now", cx).on_click(|_, _, cx| updater::open_dialog(cx))
                    }),
            )
            .child(ui::checkbox("s-upd-auto", !stored.auto_update_disabled, "Check for updates automatically", cx).on_click(cx.listener(|_, _, _, cx| {
                crate::storage::update(|s| s.auto_update_disabled = !s.auto_update_disabled);
                cx.notify();
            })))
            .child(setting("Channel", "Beta builds get new features first and may be rough around the edges.", channel, cx))
            .child(setting(
                "Update server",
                "Leave empty to use the official server.",
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().flex().flex_col().child(self.update_server.clone()))
                    .child(ui::button("s-upd-save", "Save", cx).on_click(cx.listener(|this, _, _, cx| this.save_update_server(cx)))),
                cx,
            ))
            .when_some(stored.skipped_version.clone(), |d, v| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(t.fs.s)
                        .text_color(t.colors.text_light)
                        .child(format!("Skipping version {v}."))
                        .child(ui::button("s-upd-unskip", "Stop skipping", cx).on_click(cx.listener(|_, _, _, cx| {
                            crate::storage::update(|s| s.skipped_version = None);
                            cx.notify();
                        }))),
                )
            });

        (instance_card, app_card, updates_card)
    }

    fn toggle(&mut self, f: impl FnOnce(&mut ProfileUpdate), cx: &mut Context<Self>) {
        f(&mut self.profile);
        cx.notify();
    }
}

/// The connected instance's base URL, without a trailing slash.
fn current_instance(cx: &gpui::App) -> String {
    crate::state::AppState::client(cx).base_url().as_str().trim_end_matches('/').to_string()
}

/// One option of a segmented control.
fn segment(id: impl Into<gpui::ElementId>, icon: Option<Icon>, label: &'static str, active: bool, cx: &gpui::App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    let fg = if active { t.colors.text } else { t.colors.text_light };
    let hover = t.colors.text;
    div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .py(px(6.))
        .rounded(px(5.))
        .cursor_pointer()
        .text_size(t.fs.s)
        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
        .text_color(fg)
        .when(active, |d| d.bg(t.colors.card).shadow_sm())
        .when(!active, |d| d.hover(move |s| s.text_color(hover)))
        .when_some(icon, |d, i| d.child(crate::icons::sized(i, px(14.), if active { t.colors.brand } else { fg })))
        .child(label)
}

fn segmented(cx: &gpui::App) -> gpui::Div {
    let t = cx.theme();
    div().flex().gap(px(3.)).p(px(3.)).rounded(px(7.)).bg(t.colors.button)
}

/// A labelled setting row: title + hint on the left, control below.
/// A card's explanatory text, under its heading.
fn card_text(text: impl Into<SharedString>, cx: &gpui::App) -> gpui::Div {
    let t = cx.theme();
    div().text_size(t.fs.s).text_color(t.colors.text_light).child(text.into())
}

impl SettingsView {
    fn add_keyword(&mut self, cx: &mut Context<Self>) {
        let text = self.keyword.read(cx).text().to_string();
        if crate::personal::add_keyword(&text, cx) {
            self.keyword.update(cx, |i, cx| i.clear(cx));
        }
        cx.notify();
    }

    /// Reading: how feeds and threads look, and what they hide.
    fn reading_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let stored = crate::storage::load();
        let toggle = |id: &'static str, on: bool, label: &'static str, hint: &'static str, flip: fn(&mut crate::storage::Stored), cx: &mut Context<Self>| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(ui::checkbox(id, on, label, cx).on_click(cx.listener(move |_, _, _, cx| {
                    crate::storage::update(flip);
                    cx.refresh_windows();
                })))
                .child(div().pl(px(24.)).text_size(t.fs.s).text_color(t.colors.text_light).child(hint))
        };
        let keywords = stored.keyword_filters.clone();
        let chips = keywords
            .iter()
            .enumerate()
            .map(|(ix, k)| {
                let keyword = k.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .py(px(3.))
                    .rounded_full()
                    .bg(t.colors.input)
                    .text_size(t.fs.s)
                    .child(k.clone())
                    .child(
                        div()
                            .id(SharedString::from(format!("s-kw-rm-{ix}")))
                            .p(px(3.))
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|s| s.bg(t.colors.button_hover))
                            .tooltip(ui::tooltip("Stop filtering this"))
                            .child(crate::icons::sized(Icon::X, px(12.), t.colors.text_light))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                crate::personal::remove_keyword(&keyword, cx);
                                cx.notify();
                            })),
                    )
            })
            .collect::<Vec<_>>();
        section("Reading", cx)
            .child(toggle(
                "s-carousel-below",
                stored.carousel_controls_below,
                "Image gallery controls under the picture",
                "Puts the arrows, dots and counter of posts with several images below the image, so nothing covers it.",
                |s| s.carousel_controls_below = !s.carousel_controls_below,
                cx,
            ))
            .child(toggle(
                "s-thread-colours",
                stored.colored_thread_lines,
                "Colour-code reply lines by depth",
                "Each level of replies gets its own colour, so long threads are easier to follow.",
                |s| s.colored_thread_lines = !s.colored_thread_lines,
                cx,
            ))
            .child(toggle(
                "s-sort-per-feed",
                stored.sort_per_feed,
                "Remember the sort for each feed",
                "Home, All and each community keep their own sort, instead of one sort for every feed.",
                |s| s.sort_per_feed = !s.sort_per_feed,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Keyword filters"))
                    .child(card_text(
                        "Posts whose title or text contains one of these words or phrases are hidden from feeds (not from profiles or saved lists). Whole words only, any case: \"art\" hides \"Modern Art\" but not \"start\".",
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().flex().flex_col().child(self.keyword.clone()))
                            .child(ui::button("s-kw-add", "Add", cx).on_click(cx.listener(|this, _, _, cx| this.add_keyword(cx)))),
                    )
                    .when(!chips.is_empty(), |d| d.child(div().flex().flex_wrap().gap(px(6.)).children(chips))),
            )
    }
}

fn setting(title: &'static str, hint: impl Into<SharedString>, control: impl IntoElement, cx: &gpui::App) -> gpui::Div {
    let t = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_size(t.fs.normal).font_weight(gpui::FontWeight::SEMIBOLD).text_color(t.colors.text).child(title))
                .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(hint.into())),
        )
        .child(control)
}

/// Help & troubleshooting: where to report problems, and the logs and crash reports to attach.
fn help_section(cx: &gpui::App) -> gpui::Div {
    let crashes = crate::crash::crashes_dir();
    let crash_count = std::fs::read_dir(&crashes).map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("crash-")).count()).unwrap_or(0);
    let logs = crate::crash::logs_dir();
    section("Help & troubleshooting", cx)
        .child(card_text(
            "Found a bug or something that doesn't look right? Report it on GitHub. Attaching the log file helps us find the cause. It never includes your password or login.",
            cx,
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(
                    ui::button_main_with_icon("s-report", Icon::Flag, "Report a problem", cx)
                        .on_click(|_, _, cx| cx.open_url("https://github.com/tyler-Github/risquette/issues/new/choose")),
                )
                .child(ui::button_with_icon("s-logs", Icon::FileText, "Open logs folder", cx).on_click(move |_, _, cx| {
                    let _ = std::fs::create_dir_all(&logs);
                    cx.open_with_system(&logs);
                }))
                .when(crash_count > 0, |d| {
                    d.child(
                        ui::button_with_icon("s-crashes", Icon::FileText, format!("Crash reports ({crash_count})"), cx)
                            .on_click(move |_, _, cx| cx.open_with_system(&crashes)),
                    )
                }),
        )
}

fn section(title: &'static str, cx: &gpui::App) -> gpui::Div {
    ui::card(cx).p(px(20.)).flex().flex_col().gap(px(12.)).child(ui::heading(title, cx))
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let user = Session::get(cx).user().cloned().unwrap_or_default();
        let p = self.profile.clone();
        let pic = user.pro_pic.as_ref().map(|i| i.best_url(120).to_string());

        let status = |s: &Option<(bool, SharedString)>| {
            s.clone().map(|(ok, msg)| div().text_size(t.fs.s).text_color(if ok { t.colors.green } else { t.colors.red }).child(msg))
        };

        let profile = section("Profile", cx)
            .child(
                div().flex().items_center().gap(px(14.)).child(ui::avatar(pic.as_deref(), &user.username, px(64.), cx)).child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            ui::button_with_icon("s-upload", Icon::Upload, "Upload picture", cx)
                                .on_click(cx.listener(|this, _, _, cx| this.upload_pro_pic(cx))),
                        )
                        .when(user.pro_pic.is_some(), |d| {
                            d.child(ui::button("s-rmpic", "Remove", cx).on_click(cx.listener(|this, _, _, cx| this.remove_pro_pic(cx))))
                        }),
                ),
            )
            .child(div().flex().flex_col().gap(px(6.)).child(ui::field_label("About me", cx)).child(self.about.clone()))
            .when_some(user.email.clone(), |d, e| d.child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(format!("Email: {e}"))));

        let prefs = section("Preferences", cx)
            .child(ui::checkbox("s-home", p.home_feed != "all", "Home feed shows only joined communities", cx).on_click(
                cx.listener(|this, _, _, cx| this.toggle(|p| p.home_feed = if p.home_feed == "all" { "subscriptions".into() } else { "all".into() }, cx)),
            ))
            .child(
                ui::checkbox("s-upn", !p.upvote_notifications_off, "Notify me about upvotes", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(|p| p.upvote_notifications_off = !p.upvote_notifications_off, cx))),
            )
            .child(
                ui::checkbox("s-rpn", !p.reply_notifications_off, "Notify me about replies", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(|p| p.reply_notifications_off = !p.reply_notifications_off, cx))),
            )
            .child(
                ui::checkbox("s-sort", p.remember_feed_sort, "Remember feed sort", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(|p| p.remember_feed_sort = !p.remember_feed_sort, cx))),
            )
            .child(
                ui::checkbox("s-embeds", p.embeds_off, "Disable embeds", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(|p| p.embeds_off = !p.embeds_off, cx))),
            )
            .child(
                ui::checkbox("s-pics", p.hide_user_profile_pictures, "Hide user profile pictures", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(|p| p.hide_user_profile_pictures = !p.hide_user_profile_pictures, cx))),
            )
            .child(
                ui::checkbox("s-alt", p.require_alt_text, "Require alt text on my images", cx)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(|p| p.require_alt_text = !p.require_alt_text, cx))),
            )
            .child(
                div().flex().items_center().justify_between().child(div().children(status(&self.status))).child(
                    ui::disabled(ui::button_main("s-save", if self.saving { "Saving…" } else { "Save profile & preferences" }, cx), self.saving)
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                ),
            );

        let password = section("Change password", cx).child(self.current_pw.clone()).child(self.new_pw.clone()).child(self.repeat_pw.clone()).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().children(status(&self.pw_status)))
                .child(ui::button("s-pw", "Change password", cx).on_click(cx.listener(|this, _, _, cx| this.change_password(cx)))),
        );

        let mut mutes = section("Muted", cx);
        match &self.mutes {
            None => mutes = mutes.child(ui::notice("Loading…", cx)),
            Some(m) => {
                let users = m.user_mutes.clone().unwrap_or_default();
                let comms = m.community_mutes.clone().unwrap_or_default();
                if users.is_empty() && comms.is_empty() {
                    mutes = mutes.child(div().text_color(t.colors.text_light).text_size(t.fs.s).child("You haven't muted anyone."));
                }
                for (ix, mute) in users.into_iter().chain(comms).enumerate() {
                    let label = match (&mute.muted_user, &mute.muted_community) {
                        (Some(u), _) => format!("@{}", u.username),
                        (_, Some(c)) => c.name.clone(),
                        _ => mute.muted_user_id.clone().or(mute.muted_community_id.clone()).unwrap_or_default(),
                    };
                    let id = mute.id.clone();
                    mutes = mutes.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().child(format!("{} {}", if mute.kind == "community" { "Community" } else { "User" }, label)))
                            .child(ui::button(("s-unmute", ix), "Unmute", cx).on_click(cx.listener(move |this, _, _, cx| this.unmute(id.clone(), cx)))),
                    );
                }
                mutes = mutes.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(ui::button("s-clear-u", "Unmute all users", cx).on_click(cx.listener(|this, _, _, cx| this.clear_mutes(MuteKind::User, cx))))
                        .child(
                            ui::button("s-clear-c", "Unmute all communities", cx)
                                .on_click(cx.listener(|this, _, _, cx| this.clear_mutes(MuteKind::Community, cx))),
                        ),
                );
            }
        }

        let username = user.username.clone();
        // Every saved login on this instance, with the active one first.
        let mut saved = Session::get(cx).saved_accounts(cx);
        saved.sort_by_key(|a| !a.username.eq_ignore_ascii_case(&username));
        let mut account_rows = div().flex().flex_col().gap(px(8.));
        for (ix, account) in saved.into_iter().enumerate() {
            let current = account.username.eq_ignore_ascii_case(&username);
            let name = account.username.clone();
            let (switch_name, remove_name) = (name.clone(), name.clone());
            account_rows = account_rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(8.))
                    .rounded(t.metrics.radius)
                    .when(current, |d| d.bg(t.colors.brand.opacity(0.08)))
                    .child(ui::avatar(account.avatar.as_deref(), &name, px(32.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).text_color(t.colors.text).child(format!("@{name}")))
                            .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(if current { "Using now" } else { "Saved login" })),
                    )
                    .when(!current, |d| {
                        d.child(
                            ui::button(("s-acct-switch", ix), "Switch", cx)
                                .on_click(move |_, _, cx| Session::entity(cx).update(cx, |s, cx| s.switch_account(&switch_name, cx))),
                        )
                    })
                    .child(ui::button(("s-acct-remove", ix), if current { "Log out" } else { "Remove" }, cx).on_click(cx.listener(move |_, _, _, cx| {
                        let name = remove_name.clone();
                        Session::entity(cx).update(cx, |s, cx| s.remove_account(&name, cx));
                        cx.notify();
                    }))),
            );
        }
        let danger = section("Accounts", cx)
            .child(card_text(
                "Stay logged in to more than one account and switch between them here or from the account menu. Removing an account logs it out.",
                cx,
            ))
            .child(account_rows)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(ui::button_with_icon("s-add-account", Icon::Plus, "Add account", cx).on_click(|_, _, cx| crate::modals::open_add_account(cx)))
                    .child(div().flex_1())
                    .child(ui::button_danger("s-delete", format!("Delete @{username}'s account"), cx).on_click(move |_, _, cx| {
                        let username = username.clone();
                        FormSpec::new("Delete account", "Delete my account", move |v, cx| {
                            let (username, password) = (username.clone(), v.texts[0].clone());
                            let task = api_task(cx, move |c| c.delete_account(&username, &password));
                            cx.spawn(async move |cx| {
                                task.await?;
                                cx.update(|cx| {
                                    Session::entity(cx).update(cx, |s, cx| s.logout(cx));
                                    nav::toast("Your account was deleted.", cx);
                                })
                                .ok();
                                Ok(())
                            })
                        })
                        .description("This permanently deletes your account. Enter your password to confirm.")
                        .field(Field::password("Password"))
                        .danger()
                        .open(cx);
                    })),
            );

        let (instance_card, app_card, updates_card) = self.app_sections(window, cx);
        let reading_card = self.reading_section(cx);
        let help_card = help_section(cx);
        let logged_in = Session::get(cx).user().is_some();
        let two_cols = f32::from(window.viewport_size().width) > 1250.;
        let (left, right) = if logged_in {
            (
                div().flex_1().min_w_0().flex().flex_col().gap(px(12.)).child(danger).child(profile).child(prefs).child(password),
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(app_card)
                    .child(reading_card)
                    .child(instance_card)
                    .child(updates_card)
                    .child(help_card)
                    .child(mutes),
            )
        } else {
            let signin = section("Account", cx).child(card_text("Log in to edit your profile, notification preferences, password and mutes.", cx)).child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(ui::button_main_with_icon("s-login", Icon::LogIn, "Log in", cx).on_click(|_, _, cx| crate::modals::open_login(cx)))
                    .child(ui::button("s-signup", "Create account", cx).on_click(|_, _, cx| crate::modals::open_signup(cx))),
            );
            (
                div().flex_1().min_w_0().flex().flex_col().gap(px(12.)).child(app_card).child(reading_card).child(signin).child(help_card),
                div().flex_1().min_w_0().flex().flex_col().gap(px(12.)).child(instance_card).child(updates_card),
            )
        };
        // Centred with a row whose child grows up to PAGE_MAX, rather than a centred
        // column with `w_full().max_w()`: in that layout GPUI also measures text at
        // the uncapped width, then draws it on one line past the card's edge.
        div().id("settings").size_full().overflow_y_scroll().child(
            div().w_full().flex().justify_center().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(crate::layout::PAGE_MAX))
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .when(two_cols, |d| d.flex_row().items_start())
                    .when(!two_cols, |d| d.flex_col())
                    .gap(px(12.))
                    .child(left)
                    .child(right),
            ),
        )
    }
}
