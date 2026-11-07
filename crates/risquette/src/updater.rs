//! In-app updates: ask the Risquette update server (see the
//! `Risquette-Backend` project) for the newest release, download the
//! installer with progress, verify its SHA-256, then hand over to the
//! installer in update mode and quit.

use crate::storage;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, SharedString, Task};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// The version of this build.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The official update server.
pub const OFFICIAL_SERVER: &str = "https://risquette.xnoid.studio";

/// The default update server: `RISQUETTE_UPDATE_URL` at build time, else the official one.
pub const DEFAULT_SERVER: &str = match option_env!("RISQUETTE_UPDATE_URL") {
    Some(url) => url,
    None => OFFICIAL_SERVER,
};

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const PLATFORM: &str = "windows-x86_64";
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub const PLATFORM: &str = "windows-aarch64";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const PLATFORM: &str = "macos-aarch64";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub const PLATFORM: &str = "macos-x86_64";
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const PLATFORM: &str = "linux-x86_64";
#[cfg(not(any(
    all(target_os = "windows", any(target_arch = "x86_64", target_arch = "aarch64")),
    all(target_os = "macos", any(target_arch = "x86_64", target_arch = "aarch64")),
    all(target_os = "linux", target_arch = "x86_64")
)))]
pub const PLATFORM: &str = "unknown";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    pub url: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub release_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Answer {
    available: bool,
    latest: Option<Release>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading { release: Release, done: u64, total: u64 },
    Ready { release: Release, installer: PathBuf },
    Failed(SharedString),
    NotConfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdaterEvent {
    /// An update was found by an automatic check.
    Found,
}

pub struct Updater {
    pub status: Status,
    _task: Option<Task<()>>,
}

impl EventEmitter<UpdaterEvent> for Updater {}

struct UpdaterGlobal(Entity<Updater>);

impl Global for UpdaterGlobal {}

pub fn server(cx: &App) -> Option<String> {
    let _ = cx;
    storage::load()
        .update_server
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| if DEFAULT_SERVER.trim().is_empty() { OFFICIAL_SERVER } else { DEFAULT_SERVER }.to_string())
        .trim()
        .trim_end_matches('/')
        .to_string()
        .into()
}

pub fn channel() -> &'static str {
    match storage::load().update_channel.as_deref() {
        Some("beta") => "beta",
        _ => "stable",
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("Risquette/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Ask the update server whether a newer release exists.
fn fetch(server: &str, channel: &str) -> Result<Option<Release>, String> {
    let url = format!("{server}/v1/updates/{PLATFORM}?channel={channel}&current={CURRENT_VERSION}");
    let mut resp = agent().get(&url).call().map_err(|e| format!("Couldn't reach the update server: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("The update server returned {}", resp.status()));
    }
    let answer: Answer = resp.body_mut().read_json().map_err(|e| format!("Unexpected answer from the update server: {e}"))?;
    Ok(if answer.available { answer.latest } else { None })
}

/// Download `release` to the temp folder, reporting progress, and verify its checksum.
fn download(release: &Release, progress: &AtomicU64) -> Result<PathBuf, String> {
    let expected = release.sha256.clone().ok_or("This release has no checksum, so it can't be verified. Download it from the release page instead.")?;
    let dir = std::env::temp_dir().join("Risquette");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("RisquetteSetup-{}-{PLATFORM}.exe", release.version));
    let resp = agent().get(&release.url).call().map_err(|e| format!("Download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed ({})", resp.status()));
    }
    let mut reader = resp.into_body().into_with_config().limit(2 * 1024 * 1024 * 1024).reader();
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress.store(done, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    let actual = hex(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        let _ = std::fs::remove_file(&path);
        return Err("The download didn't match its checksum and was discarded.".into());
    }
    Ok(path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Updater {
    pub fn init(cx: &mut App) {
        let updater = cx.new(|cx| {
            let mut u = Updater { status: Status::Idle, _task: None };
            u.schedule_auto_checks(cx);
            u
        });
        cx.set_global(UpdaterGlobal(updater));
    }

    pub fn entity(cx: &App) -> Entity<Updater> {
        cx.global::<UpdaterGlobal>().0.clone()
    }

    /// The release on offer (available, downloading or ready), unless skipped.
    pub fn offered(&self) -> Option<&Release> {
        match &self.status {
            Status::Available(r) | Status::Downloading { release: r, .. } | Status::Ready { release: r, .. } => Some(r),
            _ => None,
        }
    }

    fn schedule_auto_checks(&mut self, cx: &mut Context<Self>) {
        self._task = Some(cx.spawn(async move |this, cx| {
            let mut delay = Duration::from_secs(8);
            loop {
                cx.background_executor().timer(delay).await;
                delay = Duration::from_secs(6 * 60 * 60);
                if storage::load().auto_update_disabled {
                    continue;
                }
                let idle = this.update(cx, |this, _| matches!(this.status, Status::Idle | Status::UpToDate | Status::Failed(_) | Status::NotConfigured)).unwrap_or(false);
                if idle {
                    let _ = this.update(cx, |this, cx| this.check(false, cx));
                }
            }
        }));
    }

    /// Check now. `manual` checks ignore a skipped version and always report the result.
    pub fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        let Some(server) = server(cx) else {
            self.status = Status::NotConfigured;
            cx.notify();
            return;
        };
        if PLATFORM == "unknown" {
            self.status = Status::Failed("Updates aren't available for this platform.".into());
            cx.notify();
            return;
        }
        self.status = Status::Checking;
        cx.notify();
        let channel = channel();
        let task = cx.background_executor().spawn(async move { fetch(&server, channel) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(Some(release)) => {
                        let skipped = storage::load().skipped_version.as_deref() == Some(release.version.as_str());
                        if skipped && !manual {
                            Status::UpToDate
                        } else {
                            if !manual {
                                cx.emit(UpdaterEvent::Found);
                            }
                            Status::Available(release)
                        }
                    }
                    Ok(None) => Status::UpToDate,
                    Err(e) => {
                        log::warn!("update check: {e}");
                        Status::Failed(e.into())
                    }
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn skip(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.offered() {
            let v = r.version.clone();
            storage::update(|s| s.skipped_version = Some(v));
        }
        self.status = Status::UpToDate;
        cx.notify();
    }

    /// Download the offered release, updating progress as it goes.
    pub fn download(&mut self, cx: &mut Context<Self>) {
        let Status::Available(release) = self.status.clone() else { return };
        let progress = Arc::new(AtomicU64::new(0));
        self.status = Status::Downloading { release: release.clone(), done: 0, total: release.size };
        cx.notify();
        let (r, p) = (release.clone(), progress.clone());
        let task = cx.background_executor().spawn(async move { download(&r, &p) });
        // Poll progress for the UI while the download runs.
        let poll = progress.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let still = this
                    .update(cx, |this, cx| {
                        if let Status::Downloading { done, .. } = &mut this.status {
                            *done = poll.load(Ordering::Relaxed);
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if !still {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(installer) => Status::Ready { release, installer },
                    Err(e) => Status::Failed(e.into()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Start the installer in update mode and quit, so it can replace this executable.
    pub fn install(&mut self, cx: &mut Context<Self>) {
        let Status::Ready { installer, .. } = self.status.clone() else { return };
        let pid = std::process::id().to_string();
        match std::process::Command::new(&installer).args(["--update", "--wait-pid", &pid, "--launch"]).spawn() {
            Ok(_) => cx.quit(),
            Err(e) => {
                self.status = Status::Failed(format!("Couldn't start the installer: {e}").into());
                cx.notify();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Update interface
// ---------------------------------------------------------------------------

/// Open the update dialog (checking first if nothing is known yet).
pub fn open_dialog(cx: &mut App) {
    let updater = Updater::entity(cx);
    if matches!(updater.read(cx).status, Status::Idle | Status::UpToDate | Status::Failed(_) | Status::NotConfigured) {
        updater.update(cx, |u, cx| u.check(true, cx));
    }
    let view = cx.new(|cx| UpdateDialog { _sub: cx.observe(&updater, |_, _, cx| cx.notify()), updater });
    crate::nav::open_modal(view, cx);
}

pub struct UpdateDialog {
    updater: Entity<Updater>,
    _sub: gpui::Subscription,
}

fn short_date(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso).map(|d| d.format("%B %-d, %Y").to_string()).unwrap_or_default()
}

impl gpui::Render for UpdateDialog {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        use crate::{
            icons::{self, Icon},
            ui,
        };
        use gpui::{div, prelude::*, px, relative, FontWeight};
        use risquette_theme::ActiveTheme;
        let t = *cx.theme();
        let status = self.updater.read(cx).status.clone();
        let updater = self.updater.clone();

        // Branded header: app mark, title and the version jump.
        let (title, subtitle): (String, String) = match &status {
            Status::Available(r) | Status::Downloading { release: r, .. } | Status::Ready { release: r, .. } => (
                format!("Risquette {} is here", r.version),
                format!(
                    "You have {CURRENT_VERSION}{}{}",
                    r.published_at.as_deref().map(|d| format!(" · released {}", short_date(d))).unwrap_or_default(),
                    if r.size > 0 { format!(" · {}", human_size(r.size)) } else { String::new() }
                ),
            ),
            Status::Checking => ("Checking for updates…".into(), format!("Risquette {CURRENT_VERSION}")),
            Status::UpToDate | Status::Idle => ("You're up to date".into(), format!("Risquette {CURRENT_VERSION} is the newest version on the {} channel.", channel())),
            Status::Failed(_) => ("Couldn't update".into(), format!("Risquette {CURRENT_VERSION}")),
            Status::NotConfigured => ("Updates aren't set up".into(), "This build has no update server. Set one in Settings → Updates.".into()),
        };
        let header_icon = match &status {
            Status::UpToDate | Status::Idle => Icon::CircleCheck,
            Status::Failed(_) => Icon::Flag,
            Status::Checking => Icon::RefreshCw,
            _ => Icon::Download,
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .p(px(18.))
            .rounded(px(8.))
            .bg(gpui::linear_gradient(
                135.,
                gpui::linear_color_stop(t.colors.brand, 0.),
                gpui::linear_color_stop(t.colors.brand.opacity(0.72), 1.),
            ))
            .child(
                div()
                    .size(px(48.))
                    .flex_none()
                    .rounded(px(12.))
                    .bg(gpui::white().opacity(0.18))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icons::sized(header_icon, px(26.), gpui::white())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .min_w_0()
                    .child(div().text_size(t.fs.xl + px(2.)).font_weight(FontWeight::BOLD).text_color(gpui::white()).child(title))
                    .child(div().text_size(t.fs.s).text_color(gpui::white().opacity(0.85)).child(subtitle)),
            );

        let notes = |r: &Release, cx: &mut Context<Self>| {
            let src = if r.notes.trim().is_empty() { "_No release notes._".to_string() } else { r.notes.clone() };
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(ui::field_label("WHAT'S NEW", cx))
                .child(
                    div()
                        .id("update-notes")
                        .max_h(px(280.))
                        .overflow_y_scroll()
                        .p(px(12.))
                        .rounded(t.metrics.radius)
                        .border_1()
                        .border_color(t.colors.card_border)
                        .bg(t.colors.bg)
                        .child(crate::markdown::render("update-notes-md", &src, cx)),
                )
        };

        let progress_bar = |fraction: f32| {
            div()
                .h(px(8.))
                .w_full()
                .rounded(px(4.))
                .bg(t.colors.button)
                .overflow_hidden()
                .child(div().h_full().w(relative(fraction.clamp(0.02, 1.))).rounded(px(4.)).bg(t.colors.brand))
        };

        let mut body = div().flex().flex_col().gap(px(14.)).child(header);
        let mut buttons = div().flex().items_center().gap(px(8.)).pt(px(4.));
        let release_page = match &status {
            Status::Available(r) | Status::Downloading { release: r, .. } | Status::Ready { release: r, .. } => r.release_url.clone(),
            _ => None,
        };
        if let Some(url) = release_page {
            buttons = buttons.child(ui::button_with_icon("upd-page", Icon::ExternalLink, "Release page", cx).on_click(move |_, _, cx| cx.open_url(&url)));
        }
        buttons = buttons.child(div().flex_1());
        match &status {
            Status::Available(r) => {
                body = body.child(notes(r, cx));
                let (u1, u2) = (updater.clone(), updater.clone());
                buttons = buttons
                    .child(ui::button("upd-skip", "Skip this version", cx).on_click(move |_, _, cx| {
                        u1.update(cx, |u, cx| u.skip(cx));
                        crate::nav::close_modal(cx);
                    }))
                    .child(ui::button("upd-later", "Later", cx).on_click(|_, _, cx| crate::nav::close_modal(cx)))
                    .child(ui::button_main_with_icon("upd-go", Icon::Download, "Download & install", cx).on_click(move |_, _, cx| u2.update(cx, |u, cx| u.download(cx))));
            }
            Status::Downloading { release, done, total } => {
                body = body.child(notes(release, cx));
                let fraction = if *total > 0 { *done as f32 / *total as f32 } else { 0. };
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(progress_bar(fraction))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .text_size(t.fs.s)
                                .text_color(t.colors.text_light)
                                .child("Downloading…")
                                .child(if *total > 0 { format!("{} of {} · {:.0}%", human_size(*done), human_size(*total), fraction * 100.) } else { human_size(*done) }),
                        ),
                );
                buttons = buttons.child(ui::button("upd-bg", "Continue in background", cx).on_click(|_, _, cx| crate::nav::close_modal(cx)));
            }
            Status::Ready { release, .. } => {
                body = body.child(notes(release, cx)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(t.fs.s)
                        .text_color(t.colors.green)
                        .child(icons::icon(Icon::ShieldCheck, t.colors.green))
                        .child("Downloaded and verified. Risquette will close, update and reopen."),
                );
                let u = updater.clone();
                buttons = buttons
                    .child(ui::button("upd-later2", "Later", cx).on_click(|_, _, cx| crate::nav::close_modal(cx)))
                    .child(ui::button_main_with_icon("upd-install", Icon::RefreshCw, "Install & restart", cx).on_click(move |_, _, cx| u.update(cx, |u, cx| u.install(cx))));
            }
            Status::Checking => {
                body = body.child(progress_bar(0.35));
            }
            Status::Failed(e) => {
                body = body.child(div().text_size(t.fs.normal).text_color(t.colors.red).child(e.clone()));
                let u = updater.clone();
                buttons = buttons
                    .child(ui::button("upd-close", "Close", cx).on_click(|_, _, cx| crate::nav::close_modal(cx)))
                    .child(ui::button_main_with_icon("upd-retry", Icon::RefreshCw, "Try again", cx).on_click(move |_, _, cx| u.update(cx, |u, cx| u.check(true, cx))));
            }
            Status::UpToDate | Status::Idle | Status::NotConfigured => {
                let u = updater.clone();
                let configured = status != Status::NotConfigured;
                buttons = buttons
                    .when(configured, |b| b.child(ui::button_with_icon("upd-again", Icon::RefreshCw, "Check again", cx).on_click(move |_, _, cx| u.update(cx, |u, cx| u.check(true, cx)))))
                    .child(ui::button_main("upd-ok", "Done", cx).on_click(|_, _, cx| crate::nav::close_modal(cx)));
            }
        }
        crate::modals::frame("Software update", 560., body.child(buttons), cx)
    }
}

/// The toolbar pill shown while an update is on offer.
pub fn toolbar_pill(cx: &App) -> Option<gpui::AnyElement> {
    use crate::icons::{self, Icon};
    use gpui::{div, prelude::*, px, FontWeight};
    use risquette_theme::ActiveTheme;
    let t = cx.theme();
    let label = match &Updater::entity(cx).read(cx).status {
        Status::Available(_) => "Update available".to_string(),
        Status::Downloading { done, total, .. } if *total > 0 => format!("Downloading {:.0}%", *done as f32 / *total as f32 * 100.),
        Status::Downloading { .. } => "Downloading…".to_string(),
        Status::Ready { .. } => "Restart to update".to_string(),
        _ => return None,
    };
    let brand = t.colors.brand;
    Some(
        div()
            .id("nav-update")
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .px(px(10.))
            .mr(px(4.))
            .rounded(px(14.))
            .bg(brand.opacity(0.12))
            .text_color(brand)
            .text_size(t.fs.s)
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .hover(move |s| s.bg(brand.opacity(0.2)))
            .child(icons::sized(Icon::Download, px(14.), brand))
            .child(label)
            .on_click(|_, _, cx| open_dialog(cx))
            .into_any_element(),
    )
}

/// `"41.0 MB"` style sizes.
pub fn human_size(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 1.0 { format!("{mb:.1} MB") } else { format!("{} KB", (bytes / 1000).max(1)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_hex() {
        assert_eq!(human_size(41_000_000), "41.0 MB");
        assert_eq!(human_size(500), "1 KB");
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
    }

    #[test]
    fn answer_parses() {
        let a: Answer = serde_json::from_str(r#"{"available":true,"current":"0.1.0","latest":{"version":"0.2.0","name":"Risquette 0.2.0","notes":"n","url":"https://x/y.exe","size":5,"sha256":null}}"#).unwrap();
        assert!(a.available);
        assert_eq!(a.latest.unwrap().version, "0.2.0");
    }
}
