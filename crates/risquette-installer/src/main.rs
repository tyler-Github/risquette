//! Risquette Setup: a branded installer, updater and uninstaller.
//!
//! - `risquette-setup`                               install wizard
//! - `risquette-setup --silent [--dir PATH]`         unattended install
//! - `risquette-setup --update --wait-pid N --launch` started by the app's updater
//! - `uninstall.exe --uninstall [--silent]`          remove (registered with Windows)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod install;

use gpui::{
    App, Application, Bounds, Context, FontWeight, Hsla, Image, ImageFormat, PathPromptOptions, SharedString, Task, TitlebarOptions, Window, WindowAppearance,
    WindowBounds, WindowOptions, div, img, prelude::*, px, relative, size,
};
use install::Options;
use risquette_theme::{ActiveTheme, Mode as ThemeMode, Theme};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Install,
    Update { pid: Option<u32>, launch: bool },
    Uninstall,
}

#[derive(Debug, Clone)]
struct Args {
    mode: Mode,
    silent: bool,
    dir: Option<PathBuf>,
    /// Debug builds: show a page without doing anything (for screenshots).
    preview: Option<String>,
}

fn parse_args(args: &[String]) -> Args {
    let mut out = Args { mode: Mode::Install, silent: false, dir: None, preview: None };
    let (mut pid, mut launch, mut update) = (None, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--update" => update = true,
            "--uninstall" => out.mode = Mode::Uninstall,
            "--silent" | "/S" => out.silent = true,
            "--launch" => launch = true,
            "--wait-pid" => pid = it.next().and_then(|p| p.parse().ok()),
            "--dir" => out.dir = it.next().map(PathBuf::from),
            "--preview" if cfg!(debug_assertions) => out.preview = it.next().cloned(),
            _ => {}
        }
    }
    if update && out.mode != Mode::Uninstall {
        out.mode = Mode::Update { pid, launch };
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Page {
    Welcome,
    Options,
    Working,
    Done,
    Failed(SharedString),
    ConfirmUninstall,
    Uninstalled,
}

enum Msg {
    Progress(f32, String),
    Finished(Result<(), String>),
}

struct Setup {
    mode: Mode,
    page: Page,
    opts: Options,
    launch_after: bool,
    remove_data: bool,
    progress: f32,
    message: String,
    existing: Option<String>,
    logo: Arc<Image>,
    _task: Option<Task<()>>,
}

static LOGO: &[u8] = include_bytes!("../../../assets/risquette.png");

impl Setup {
    fn new(args: &Args, cx: &mut Context<Self>) -> Self {
        let dir = args.dir.clone().or_else(install::installed_dir).unwrap_or_else(install::default_dir);
        let mut this = Self {
            mode: args.mode.clone(),
            page: match args.mode {
                Mode::Install => Page::Welcome,
                Mode::Update { .. } => Page::Working,
                Mode::Uninstall => Page::ConfirmUninstall,
            },
            opts: Options::new(dir),
            launch_after: true,
            remove_data: false,
            progress: 0.,
            message: "Starting…".into(),
            existing: install::installed_version(),
            logo: Arc::new(Image::from_bytes(ImageFormat::Png, LOGO.to_vec())),
            _task: None,
        };
        if let Some(page) = args.preview.as_deref() {
            this.page = match page {
                "options" => Page::Options,
                "working" => {
                    this.progress = 0.55;
                    this.message = "Replacing the old version…".into();
                    Page::Working
                }
                "done" => Page::Done,
                "failed" => Page::Failed("Risquette seems to be running. Close it and try again.".into()),
                _ => this.page,
            };
            return this;
        }
        if let Mode::Update { pid, .. } = this.mode {
            this.start_install(pid, cx);
        }
        this
    }

    /// Run `job` on a worker thread, streaming its progress into the UI.
    fn run(&mut self, job: impl FnOnce(&dyn Fn(f32, &str)) -> anyhow::Result<()> + Send + 'static, cx: &mut Context<Self>) {
        let (tx, rx) = smol::channel::unbounded::<Msg>();
        std::thread::spawn(move || {
            let report = |f: f32, m: &str| {
                let _ = tx.send_blocking(Msg::Progress(f, m.to_string()));
            };
            let result = job(&report).map_err(|e| format!("{e:#}"));
            let _ = tx.send_blocking(Msg::Finished(result));
        });
        self.progress = 0.;
        self.page = Page::Working;
        self._task = Some(cx.spawn(async move |this, cx| {
            while let Ok(msg) = rx.recv().await {
                let finished = matches!(msg, Msg::Finished(_));
                if this.update(cx, |this, cx| this.on_msg(msg, cx)).is_err() || finished {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn on_msg(&mut self, msg: Msg, cx: &mut Context<Self>) {
        match msg {
            Msg::Progress(f, m) => {
                self.progress = f;
                self.message = m;
            }
            Msg::Finished(Err(e)) => self.page = Page::Failed(e.into()),
            Msg::Finished(Ok(())) => match self.mode.clone() {
                Mode::Uninstall => self.page = Page::Uninstalled,
                Mode::Install => self.page = Page::Done,
                Mode::Update { launch, .. } => {
                    self.page = Page::Done;
                    self.message = "Risquette is up to date.".into();
                    if launch {
                        let _ = install::launch(&self.opts.dir);
                    }
                    // Close on our own shortly after; nothing to ask.
                    cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(Duration::from_millis(1200)).await;
                        cx.update(|cx| cx.quit()).ok();
                    })
                    .detach();
                }
            },
        }
        cx.notify();
    }

    fn start_install(&mut self, wait_pid: Option<u32>, cx: &mut Context<Self>) {
        let opts = self.opts.clone();
        self.run(
            move |p| {
                if let Some(pid) = wait_pid {
                    p(0.02, "Waiting for Risquette to close…");
                    install::wait_for_pid(pid);
                }
                install::install(&opts, p)
            },
            cx,
        );
    }

    fn start_uninstall(&mut self, cx: &mut Context<Self>) {
        let (dir, remove_data) = (self.opts.dir.clone(), self.remove_data);
        self.run(move |p| install::uninstall(&dir, remove_data, p), cx);
    }

    fn choose_dir(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Install here".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(mut dir) = paths.into_iter().next() else { return };
            if dir.file_name().is_none_or(|n| !n.eq_ignore_ascii_case("Risquette")) {
                dir.push("Risquette");
            }
            this.update(cx, |this, cx| {
                this.opts.dir = dir;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Done && self.launch_after && self.mode == Mode::Install {
            let _ = install::launch(&self.opts.dir);
        }
        cx.quit();
    }
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

fn button(id: &'static str, label: impl Into<SharedString>, primary: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    let (bg, fg, hover) = if primary {
        (t.colors.button_main, t.colors.button_main_text, t.colors.button_main_hover)
    } else {
        (t.colors.button, t.colors.button_text, t.colors.button_hover)
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(96.))
        .h(px(34.))
        .px(px(16.))
        .rounded(px(6.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(label.into())
}

fn danger_button(id: &'static str, label: &'static str, cx: &App) -> gpui::Stateful<gpui::Div> {
    let red = cx.theme().colors.red;
    button(id, label, true, cx).bg(red).text_color(gpui::white())
}

fn checkbox(id: &'static str, checked: bool, label: &'static str, hint: Option<&'static str>, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.theme();
    div()
        .id(id)
        .flex()
        .items_start()
        .gap(px(10.))
        .cursor_pointer()
        .child(
            div()
                .mt(px(2.))
                .size(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(if checked { t.colors.brand } else { t.colors.text_light })
                .bg(if checked { t.colors.brand } else { gpui::transparent_black() })
                .text_color(gpui::white())
                .text_size(px(12.))
                .font_weight(FontWeight::BOLD)
                .when(checked, |d| d.child("✓")),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_size(px(14.)).text_color(t.colors.text).child(label))
                .when_some(hint, |d, h| d.child(div().text_size(px(12.)).text_color(t.colors.text_light).child(h))),
        )
}

fn progress_bar(fraction: f32, cx: &App) -> gpui::Div {
    let t = cx.theme();
    div()
        .h(px(8.))
        .w_full()
        .rounded(px(4.))
        .bg(t.colors.button)
        .overflow_hidden()
        .child(div().h_full().w(relative(fraction.clamp(0.02, 1.))).rounded(px(4.)).bg(t.colors.brand))
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", (bytes as f64 / 1_000_000.).max(1.))
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl Setup {
    /// The brand panel on the left: logo, name, steps.
    fn side_panel(&self, cx: &App) -> gpui::Div {
        let t = cx.theme();
        let brand = t.colors.brand;
        let steps: &[&str] = match self.mode {
            Mode::Uninstall => &["Confirm", "Remove", "Finished"],
            _ => &["Welcome", "Options", "Install", "Finished"],
        };
        let current = match (&self.mode, &self.page) {
            (Mode::Uninstall, Page::ConfirmUninstall) => 0,
            (Mode::Uninstall, Page::Working | Page::Failed(_)) => 1,
            (Mode::Uninstall, _) => 2,
            (_, Page::Welcome) => 0,
            (_, Page::Options) => 1,
            (_, Page::Working | Page::Failed(_)) => 2,
            _ => 3,
        };
        let mut list = div().flex().flex_col().gap(px(14.)).mt(px(36.));
        for (ix, step) in steps.iter().enumerate() {
            let (done, active) = (ix < current, ix == current);
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(14.))
                    .text_color(gpui::white().opacity(if active {
                        1.
                    } else if done {
                        0.85
                    } else {
                        0.55
                    }))
                    .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                    .child(
                        div()
                            .size(px(22.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(gpui::white().opacity(if active || done { 0.9 } else { 0.4 }))
                            .when(active || done, |d| d.bg(gpui::white()))
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(if active || done { brand } else { gpui::white().opacity(0.6) })
                            .child(if done { "✓".to_string() } else { (ix + 1).to_string() }),
                    )
                    .child(*step),
            );
        }
        div()
            .w(px(232.))
            .flex_none()
            .h_full()
            .p(px(28.))
            .flex()
            .flex_col()
            .bg(gpui::linear_gradient(
                160.,
                gpui::linear_color_stop(Hsla { l: brand.l + 0.08, ..brand }, 0.),
                gpui::linear_color_stop(Hsla { l: (brand.l - 0.12).max(0.15), ..brand }, 1.),
            ))
            .child(img(self.logo.clone()).size(px(56.)).rounded(px(12.)))
            .child(div().mt(px(14.)).text_size(px(24.)).font_weight(FontWeight::EXTRA_BOLD).text_color(gpui::white()).child("Risquette"))
            .child(div().text_size(px(13.)).text_color(gpui::white().opacity(0.8)).child("A desktop client for Discuit"))
            .child(list)
            .child(div().flex_1())
            .child(div().text_size(px(12.)).text_color(gpui::white().opacity(0.6)).child(format!("Version {}", install::VERSION)))
    }

    fn page_body(&mut self, cx: &mut Context<Self>) -> (String, gpui::AnyElement, gpui::Div) {
        let t = *cx.theme();
        let text = |s: String| div().text_size(px(14.)).line_height(px(21.)).text_color(t.colors.text_light).child(s);
        let footer = div().flex().items_center().justify_end().gap(px(8.));
        match self.page.clone() {
            Page::Welcome => {
                let intro = match &self.existing {
                    Some(v) if v == install::VERSION => format!("Risquette {v} is already installed. Continue to repair or reinstall it."),
                    Some(v) => format!("Risquette {v} is installed. Continuing will update it to {}. Your settings and logins are kept.", install::VERSION),
                    None => "A fast, native desktop client for Discuit. Browse your communities, post and comment, and get notified without keeping a browser tab open.".into(),
                };
                let feature = |title: &'static str, body: &'static str| {
                    div().flex().gap(px(12.)).child(div().mt(px(6.)).size(px(8.)).flex_none().rounded_full().bg(t.colors.brand)).child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text).child(title))
                            .child(div().text_size(px(13.)).text_color(t.colors.text_light).child(body)),
                    )
                };
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(text(intro))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(feature("Native and light", "Built in Rust with GPUI. Starts fast and stays responsive."))
                            .child(feature("Any Discuit instance", "Use discuit.org or any other server running Discuit."))
                            .child(feature("Stays up to date", "New versions install themselves when you choose to.")),
                    )
                    .child(text("Risquette installs for your Windows account only and doesn't need administrator rights.".into()));
                let footer = footer.child(button("cancel", "Cancel", false, cx).on_click(cx.listener(|_, _, _, cx| cx.quit()))).child(
                    button("next", "Next", true, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.page = Page::Options;
                        cx.notify();
                    })),
                );
                (if self.existing.is_some() { "Update Risquette".into() } else { "Welcome to Risquette".into() }, body.into_any_element(), footer)
            }
            Page::Options => {
                let o = &self.opts;
                let location = div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text_light).child("INSTALL LOCATION"))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(34.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(t.colors.input_border_hover)
                                    .bg(t.colors.input)
                                    .text_size(px(13.))
                                    .text_color(t.colors.text)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(o.dir.display().to_string()),
                            )
                            .child(button("browse", "Change…", false, cx).on_click(cx.listener(|this, _, _, cx| this.choose_dir(cx)))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.colors.text_light)
                            .child(format!("Needs about {} of disk space.", mb(install::payload_size() * 2))),
                    );
                let body = div().flex().flex_col().gap(px(20.)).child(location).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(checkbox("start", o.start_menu_shortcut, "Add to the Start menu", None, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.opts.start_menu_shortcut = !this.opts.start_menu_shortcut;
                            cx.notify();
                        })))
                        .child(checkbox("desk", o.desktop_shortcut, "Create a desktop shortcut", None, cx).on_click(cx.listener(|this, _, _, cx| {
                            this.opts.desktop_shortcut = !this.opts.desktop_shortcut;
                            cx.notify();
                        })))
                        .child(checkbox("launch", self.launch_after, "Open Risquette when setup finishes", None, cx).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.launch_after = !this.launch_after;
                                cx.notify();
                            },
                        ))),
                );
                let footer = footer
                    .child(button("back", "Back", false, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.page = Page::Welcome;
                        cx.notify();
                    })))
                    .child(
                        button("install", if self.existing.is_some() { "Update" } else { "Install" }, true, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.start_install(None, cx))),
                    );
                ("Choose how to install".into(), body.into_any_element(), footer)
            }
            Page::Working => {
                let title = match self.mode {
                    Mode::Uninstall => "Removing Risquette".to_string(),
                    _ if self.existing.is_some() => "Updating Risquette".to_string(),
                    _ => "Installing Risquette".to_string(),
                };
                let body = div().flex().flex_col().gap(px(10.)).child(progress_bar(self.progress, cx)).child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(px(13.))
                        .text_color(t.colors.text_light)
                        .child(self.message.clone())
                        .child(format!("{:.0}%", self.progress * 100.)),
                );
                (title, body.into_any_element(), footer)
            }
            Page::Done => {
                let body = div().flex().flex_col().gap(px(18.)).child(text(format!(
                    "Risquette {} is installed in {}. You'll find it in the Start menu{}.",
                    install::VERSION,
                    self.opts.dir.display(),
                    if self.opts.desktop_shortcut { " and on your desktop" } else { "" }
                )));
                let body = if self.mode == Mode::Install {
                    body.child(checkbox("launch2", self.launch_after, "Open Risquette now", None, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.launch_after = !this.launch_after;
                        cx.notify();
                    })))
                } else {
                    body
                };
                let footer = footer.child(button("finish", "Finish", true, cx).on_click(cx.listener(|this, _, _, cx| this.finish(cx))));
                ("You're all set".into(), body.into_any_element(), footer)
            }
            Page::Failed(e) => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(div().p(px(12.)).rounded(px(6.)).bg(t.colors.red.opacity(0.1)).text_size(px(14.)).text_color(t.colors.red).child(e))
                    .child(text("Nothing was left half-installed: the previous version (if any) still works.".into()));
                let retry_mode = self.mode.clone();
                let footer = footer.child(button("close", "Close", false, cx).on_click(cx.listener(|_, _, _, cx| cx.quit()))).child(
                    button("retry", "Try again", true, cx).on_click(cx.listener(move |this, _, _, cx| match retry_mode {
                        Mode::Uninstall => this.start_uninstall(cx),
                        _ => this.start_install(None, cx),
                    })),
                );
                ("Something went wrong".into(), body.into_any_element(), footer)
            }
            Page::ConfirmUninstall => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(text(format!("This removes Risquette from {} along with its shortcuts.", self.opts.dir.display())))
                    .child(
                        checkbox(
                            "data",
                            self.remove_data,
                            "Also remove my settings and saved logins",
                            Some("Keep them if you plan to install Risquette again."),
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remove_data = !this.remove_data;
                            cx.notify();
                        })),
                    );
                let footer = footer
                    .child(button("cancel", "Cancel", false, cx).on_click(cx.listener(|_, _, _, cx| cx.quit())))
                    .child(danger_button("uninstall", "Uninstall", cx).on_click(cx.listener(|this, _, _, cx| this.start_uninstall(cx))));
                ("Uninstall Risquette?".into(), body.into_any_element(), footer)
            }
            Page::Uninstalled => {
                let body = text("Risquette has been removed. Thanks for trying it!".into());
                let footer = footer.child(button("close", "Close", true, cx).on_click(cx.listener(|_, _, _, cx| cx.quit())));
                ("Risquette was removed".into(), body.into_any_element(), footer)
            }
        }
    }

    /// The compact window used for in-app updates.
    fn render_update(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let t = *cx.theme();
        let brand = t.colors.brand;
        let (title, failed) = match &self.page {
            Page::Done => ("Risquette is up to date".to_string(), None),
            Page::Failed(e) => ("The update didn't finish".to_string(), Some(e.clone())),
            _ => (format!("Updating Risquette to {}", install::VERSION), None),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.colors.card)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .p(px(20.))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(Hsla { l: brand.l + 0.06, ..brand }, 0.),
                        gpui::linear_color_stop(Hsla { l: (brand.l - 0.1).max(0.15), ..brand }, 1.),
                    ))
                    .child(img(self.logo.clone()).size(px(44.)).rounded(px(10.)))
                    .child(div().flex().flex_col().child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(gpui::white()).child(title)).child(
                        div().text_size(px(12.)).text_color(gpui::white().opacity(0.8)).child(match &self.existing {
                            Some(v) => format!("From version {v}"),
                            None => "A desktop client for Discuit".to_string(),
                        }),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .justify_center()
                    .when(failed.is_none(), |d| {
                        d.child(progress_bar(if self.page == Page::Done { 1. } else { self.progress }, cx))
                            .child(div().text_size(px(13.)).text_color(t.colors.text_light).child(self.message.clone()))
                    })
                    .when_some(failed, |d, e| {
                        d.child(div().text_size(px(13.)).text_color(t.colors.red).child(e)).child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(button("close", "Close", false, cx).on_click(cx.listener(|_, _, _, cx| cx.quit())))
                                .child(button("retry", "Try again", true, cx).on_click(cx.listener(|this, _, _, cx| this.start_install(None, cx)))),
                        )
                    }),
            )
    }
}

impl Render for Setup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        if matches!(self.mode, Mode::Update { .. }) {
            return self.render_update(cx).text_color(t.colors.text);
        }
        let side = self.side_panel(cx);
        let (title, body, footer) = self.page_body(cx);
        div().size_full().flex().bg(t.colors.card).text_color(t.colors.text).child(side).child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_1()
                        .p(px(32.))
                        .flex()
                        .flex_col()
                        .gap(px(18.))
                        .child(div().text_size(px(24.)).font_weight(FontWeight::BOLD).text_color(t.colors.text).child(title))
                        .child(body),
                )
                .child(div().px(px(32.)).py(px(18.)).border_t_1().border_color(t.colors.separator).child(footer)),
        )
    }
}

fn run_silent(args: &Args) -> i32 {
    let report = |f: f32, m: &str| log::info!("{:>3.0}% {m}", f * 100.);
    let result = match args.mode {
        Mode::Uninstall => install::uninstall(&args.dir.clone().or_else(install::installed_dir).unwrap_or_else(install::default_dir), false, &report),
        Mode::Update { pid, launch } => {
            if let Some(pid) = pid {
                install::wait_for_pid(pid);
            }
            let dir = args.dir.clone().or_else(install::installed_dir).unwrap_or_else(install::default_dir);
            install::install(&Options::new(dir.clone()), &report).and_then(|_| if launch { install::launch(&dir) } else { Ok(()) })
        }
        Mode::Install => install::install(&Options::new(args.dir.clone().or_else(install::installed_dir).unwrap_or_else(install::default_dir)), &report),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            log::error!("{e:#}");
            1
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw);
    if args.silent {
        std::process::exit(run_silent(&args));
    }

    Application::new().run(move |cx: &mut App| {
        let mode = match cx.window_appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            _ => ThemeMode::Light,
        };
        cx.set_global(Theme::new(mode));
        let (title, w, h) = match args.mode {
            Mode::Update { .. } => ("Updating Risquette", 480., 210.),
            Mode::Uninstall => ("Uninstall Risquette", 720., 440.),
            Mode::Install => ("Risquette Setup", 760., 480.),
        };
        let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
        let args = args.clone();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
                is_resizable: false,
                is_minimizable: true,
                app_id: Some("risquette-setup".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Setup::new(&args, cx)),
        )
        .expect("open window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Args {
        parse_args(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn parses_modes() {
        assert_eq!(args("").mode, Mode::Install);
        assert_eq!(args("--update --wait-pid 42 --launch").mode, Mode::Update { pid: Some(42), launch: true });
        assert_eq!(args("--update").mode, Mode::Update { pid: None, launch: false });
        let a = args("--uninstall --silent");
        assert_eq!(a.mode, Mode::Uninstall);
        assert!(a.silent);
        assert_eq!(args("--dir C:\\X").dir, Some(PathBuf::from("C:\\X")));
    }
}
