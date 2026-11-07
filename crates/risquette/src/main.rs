//! Risquette — a desktop client for Discuit, built with GPUI.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod admin;
mod bus;
mod communities;
mod community;
mod crash;
mod desktop;
mod embed;
mod feed;
mod icons;
mod input;
mod layout;
mod lists;
mod markdown;
mod modals;
mod modtools;
mod nav;
mod new_post;
mod notifications;
mod player;
mod post;
mod post_card;
mod quick_jump;
mod root;
mod session;
mod settings;
mod sidebar;
mod site_aside;
mod state;
mod storage;
mod ui;
mod updater;
mod user;

use discuit_api::{Client, DEFAULT_BASE_URL};
use gpui::{App, AppContext, Application, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowAppearance, WindowBounds, WindowOptions, actions, px, size};
use risquette_theme::{Mode, Theme};
use std::sync::Arc;

actions!(risquette, [Back, Refresh, ToggleTheme, Quit]);

fn startup_target(arg: &str, client: &Client) -> Option<nav::Target> {
    let path = if arg.starts_with('/') {
        arg.to_string()
    } else {
        let url = url::Url::parse(arg).ok()?;
        (url.host_str() == client.base_url().host_str()).then(|| url.path().to_string())?
    };
    nav::target_for_path(&path)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    crash::init_logging();
    crash::install_panic_hook();

    // `risquette --crash-report <file>`: started by the panic hook to show the crash window.
    if let [flag, path, ..] = args.as_slice()
        && flag == "--crash-report"
    {
        crash::run_reporter(path.into());
        return;
    }

    // One window: if Risquette is already running (with these settings), bring it
    // to the front instead. The pop-out video player (`--play`) is exempt.
    let play_mode = args.first().is_some_and(|a| a == "--play");
    if !play_mode && matches!(desktop::claim_instance(), desktop::Instance::AlreadyRunning) {
        log::info!("Risquette is already running; bringing its window to the front");
        desktop::focus_running_instance();
        return;
    }

    // Instance: env override, then the one chosen in Settings, then discuit.org.
    let base_url = std::env::var("RISQUETTE_BASE_URL").ok().or_else(|| storage::load().instance).unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let client = Client::new(&base_url).expect("valid base url");
    log::info!("connecting to {base_url}");
    // `risquette <discuit link or path>` opens that page, e.g. `risquette https://discuit.org/@alice`.
    // `risquette --play <video link>` opens the pop-out player for that link.
    let play = match args.as_slice() {
        [flag, link, ..] if flag == "--play" => match embed::detect(link) {
            Some(embed::Embed::Video { player, .. }) => Some(player),
            _ => None,
        },
        _ => None,
    };
    let startup = if play.is_some() { None } else { args.first().and_then(|arg| startup_target(arg, &client)) };

    Application::new().with_assets(icons::Assets).run(move |cx: &mut App| {
        let mode = match storage::load().theme.as_deref() {
            Some("dark") => Mode::Dark,
            Some("light") => Mode::Light,
            _ => match cx.window_appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Mode::Dark,
                _ => Mode::Light,
            },
        };
        cx.set_global(Theme::new(mode));
        cx.set_global(state::AppState { client: Arc::new(client) });
        cx.set_global(nav::StartupTarget(startup));
        bus::init(cx);
        session::Session::init(cx);
        updater::Updater::init(cx);
        crash::schedule_test_crash(cx);

        input::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("alt-left", Back, None),
            KeyBinding::new("escape", Back, None),
            KeyBinding::new("f5", Refresh, None),
            KeyBinding::new("ctrl-r", Refresh, None),
            KeyBinding::new("ctrl-shift-l", ToggleTheme, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        // Open maximized; 1280×900, centered, is the size it restores down to.
        // `RISQUETTE_WINDOWED=1` opens at that size instead (consistent screenshots).
        let bounds = Bounds::centered(None, size(px(1280.), px(900.)), cx);
        let window_bounds = if std::env::var_os("RISQUETTE_WINDOWED").is_some() { WindowBounds::Windowed(bounds) } else { WindowBounds::Maximized(bounds) };
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: Some(TitlebarOptions { title: Some("Risquette".into()), ..Default::default() }),
                    window_min_size: Some(size(px(640.), px(480.))),
                    app_id: Some("risquette".into()),
                    ..Default::default()
                },
                |_, cx| cx.new(root::Root::new),
            )
            .expect("open window");
        if let Some(player) = play {
            player::open(player, "Video".into(), cx);
        }
        window
            .update(cx, |root, window, cx| {
                window.focus(&root.focus_handle(cx));
                desktop::init(window, cx);
                cx.activate(true);
            })
            .ok();
    });
}
