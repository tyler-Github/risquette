//! Log files, crash reports and the crash reporter window.
//!
//! - Everything logged goes to `%LOCALAPPDATA%\Risquette\logs\risquette.log`
//!   (rotated at 2 MB), and the last lines are also kept in memory.
//! - If the app panics, a report (version, system, the panic message and where it
//!   happened, a backtrace and the recent log) is written to
//!   `%LOCALAPPDATA%\Risquette\crashes\`. The app then starts a second copy of
//!   itself as `risquette --crash-report <file>` and exits. A crashed app can't
//!   safely draw UI, so that second copy shows the friendly crash window.
//!
//! Reports never contain passwords or login cookies: the API client only ever
//! logs request methods and URLs with signatures redacted.

use crate::{
    icons::{self, Icon},
    ui,
};
use gpui::{
    App, AppContext, Application, Bounds, ClipboardItem, Context, FontWeight, TitlebarOptions, Window, WindowAppearance, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use risquette_theme::{ActiveTheme, Mode, Theme};
use std::{
    collections::VecDeque,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const NEW_ISSUE_URL: &str = "https://github.com/tyler-Github/risquette/issues/new";
/// Set on the crash reporter process so a crash inside it doesn't loop.
const REPORTER_ENV: &str = "RISQUETTE_CRASH_REPORTER";
const RECENT_LINES: usize = 100;
const LOG_ROTATE_BYTES: u64 = 2 * 1024 * 1024;
const KEEP_CRASH_REPORTS: usize = 10;

fn data_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("RISQUETTE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(custom);
    }
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Risquette")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn crashes_dir() -> PathBuf {
    data_dir().join("crashes")
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

struct Logger {
    /// Parses `RUST_LOG` and echoes to stderr (visible when run from a terminal).
    inner: env_logger::Logger,
    file: Mutex<Option<fs::File>>,
    recent: Mutex<VecDeque<String>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if !self.inner.matches(record) {
            return;
        }
        let line = format!("{} {:<5} {}: {}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"), record.level(), record.target(), record.args());
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = writeln!(file, "{line}");
        }
        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() == RECENT_LINES {
                recent.pop_front();
            }
            recent.push_back(line);
        }
        self.inner.log(record);
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock()
            && let Some(file) = file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

static LOGGER: OnceLock<&'static Logger> = OnceLock::new();

/// Move a log that has grown past `limit` aside (`risquette.1.log`), keeping one old copy.
fn rotate(log: &Path, limit: u64) {
    if fs::metadata(log).is_ok_and(|m| m.len() > limit) {
        let _ = fs::rename(log, log.with_extension("1.log"));
    }
}

/// Start logging to the console and to the log file.
pub fn init_logging() {
    let inner = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("risquette=info,discuit_api=info")).build();
    let dir = logs_dir();
    let file = fs::create_dir_all(&dir).ok().and_then(|_| {
        let path = dir.join("risquette.log");
        rotate(&path, LOG_ROTATE_BYTES);
        fs::OpenOptions::new().create(true).append(true).open(path).ok()
    });
    let max = inner.filter();
    let logger: &'static Logger = Box::leak(Box::new(Logger { inner, file: Mutex::new(file), recent: Mutex::new(VecDeque::new()) }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(max);
        let _ = LOGGER.set(logger);
    }
    log::info!("Risquette {VERSION} starting on {}", system_description());
}

fn recent_log() -> Vec<String> {
    // try_lock: the panic may have happened while a log line was being written.
    LOGGER.get().and_then(|l| l.recent.try_lock().ok()).map(|r| r.iter().cloned().collect()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// System description
// ---------------------------------------------------------------------------

/// "Windows 11 Pro 24H2 (build 26100)", or the OS name elsewhere.
pub fn system_description() -> String {
    static DESC: OnceLock<String> = OnceLock::new();
    DESC.get_or_init(|| {
        #[cfg(windows)]
        {
            let product = windows_reg_string("ProductName").unwrap_or_else(|| "Windows".into());
            let build = windows_reg_string("CurrentBuildNumber").unwrap_or_default();
            let display = windows_reg_string("DisplayVersion").unwrap_or_default();
            // Windows 11 still reports "Windows 10" in ProductName.
            let product = if build.parse::<u32>().is_ok_and(|b| b >= 22000) { product.replace("Windows 10", "Windows 11") } else { product };
            let mut out = product;
            if !display.is_empty() {
                out.push(' ');
                out.push_str(&display);
            }
            if !build.is_empty() {
                out.push_str(&format!(" (build {build})"));
            }
            out.push_str(&format!(", {}", std::env::consts::ARCH));
            out
        }
        #[cfg(not(windows))]
        {
            // e.g. "Ubuntu 24.04.1 LTS (kernel 6.8.0-45-generic, wayland), x86_64"
            let name = std::fs::read_to_string("/etc/os-release").ok().and_then(|t| os_release_name(&t)).unwrap_or_else(|| std::env::consts::OS.to_string());
            let mut details = Vec::new();
            if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
                details.push(format!("kernel {}", kernel.trim()));
            }
            if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
                details.push(session);
            }
            let details = if details.is_empty() { String::new() } else { format!(" ({})", details.join(", ")) };
            format!("{name}{details}, {}", std::env::consts::ARCH)
        }
    })
    .clone()
}

/// The distribution name from `/etc/os-release` (`PRETTY_NAME`, else `NAME`).
#[cfg_attr(windows, allow(dead_code))]
fn os_release_name(text: &str) -> Option<String> {
    let value =
        |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty());
    value("PRETTY_NAME").or_else(|| value("NAME"))
}

#[cfg(windows)]
fn windows_reg_string(name: &str) -> Option<String> {
    use windows::{
        Win32::{
            Foundation::ERROR_SUCCESS,
            System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        },
        core::HSTRING,
    };
    let key = HSTRING::from(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let value = HSTRING::from(name);
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let err = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &key, &value, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
    if err != ERROR_SUCCESS {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..chars]).trim().to_string())
}

// ---------------------------------------------------------------------------
// Crash reports
// ---------------------------------------------------------------------------

/// What a crash report says, separated from where the data comes from (for tests).
pub struct CrashDetails {
    pub message: String,
    pub location: Option<String>,
    pub thread: String,
    pub backtrace: String,
    pub recent_log: Vec<String>,
    pub system: String,
    pub time: chrono::DateTime<chrono::Utc>,
}

pub fn format_report(d: &CrashDetails) -> String {
    let mut out = String::new();
    out.push_str("Risquette crash report\n======================\n\n");
    out.push_str(&format!("Version:  {VERSION} ({})\n", crate::updater::PLATFORM));
    out.push_str(&format!("Time:     {}\n", d.time.format("%Y-%m-%d %H:%M:%S UTC")));
    out.push_str(&format!("System:   {}\n", d.system));
    out.push_str(&format!("Thread:   {}\n\n", d.thread));
    out.push_str("What happened\n-------------\n");
    out.push_str(&d.message);
    out.push('\n');
    if let Some(location) = &d.location {
        out.push_str(&format!("at {location}\n"));
    }
    out.push_str("\nRecent log\n----------\n");
    if d.recent_log.is_empty() {
        out.push_str("(empty)\n");
    }
    for line in &d.recent_log {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\nBacktrace\n---------\n");
    out.push_str(d.backtrace.trim_end());
    out.push('\n');
    out
}

/// The panic message from a panic payload.
fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "(no message)".into()
    }
}

fn write_report(report: &str, time: chrono::DateTime<chrono::Utc>) -> Option<PathBuf> {
    let dir = crashes_dir();
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("crash-{}.txt", time.format("%Y%m%d-%H%M%S")));
    fs::write(&path, report).ok()?;
    prune_reports(&dir, KEEP_CRASH_REPORTS);
    Some(path)
}

/// Keep only the newest `keep` crash reports.
fn prune_reports(dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut reports: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt")))
        .collect();
    // Names sort by time (crash-YYYYMMDD-HHMMSS).
    reports.sort();
    let excess = reports.len().saturating_sub(keep);
    for old in reports.into_iter().take(excess) {
        let _ = fs::remove_file(old);
    }
}

/// Write a crash report and show the crash window whenever anything panics.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        static PANICKING: AtomicBool = AtomicBool::new(false);
        if PANICKING.swap(true, Ordering::SeqCst) {
            return; // a second panic while handling the first
        }
        let time = chrono::Utc::now();
        let details = CrashDetails {
            message: payload_message(info.payload()),
            location: info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
            thread: std::thread::current().name().unwrap_or("unnamed").to_string(),
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
            recent_log: recent_log(),
            system: system_description(),
            time,
        };
        let report = format_report(&details);
        log::error!("crashed: {}", details.message);
        if let Some(logger) = LOGGER.get() {
            log::Log::flush(*logger);
        }
        let path = write_report(&report, time);
        let in_reporter = std::env::var_os(REPORTER_ENV).is_some();
        if let (Some(path), false) = (path, in_reporter)
            && let Ok(exe) = std::env::current_exe()
        {
            let _ = std::process::Command::new(exe).arg("--crash-report").arg(path).env(REPORTER_ENV, "1").spawn();
        }
        // A panic on any thread leaves the app in an unknown state: stop here.
        std::process::exit(101);
    }));
}

/// A GitHub "new issue" link with the bug form filled in from the report.
pub fn issue_url(message: &str, report: &str) -> String {
    const MAX_LOG: usize = 2500; // keep the URL well under GitHub's limit
    let short: String = message.lines().next().unwrap_or_default().chars().take(80).collect();
    let mut logs: String = report.chars().take(MAX_LOG).collect();
    if report.chars().count() > MAX_LOG {
        logs.push_str("\n… (truncated: the full report is in the crash file)");
    }
    let params = [
        ("template", "bug_report.yml".to_string()),
        ("title", format!("[Crash]: {short}")),
        ("what-happened", "Risquette crashed and showed the crash window.\n\nWhat I was doing when it happened:\n".to_string()),
        ("steps", "1. \n2. \n3. ".to_string()),
        ("expected", "Risquette shouldn't crash.".to_string()),
        ("version", VERSION.to_string()),
        ("os", system_description()),
        ("logs", logs),
    ];
    url::Url::parse_with_params(NEW_ISSUE_URL, params.iter().map(|(k, v)| (*k, v.as_str())))
        .map(|u| u.to_string())
        .unwrap_or_else(|_| NEW_ISSUE_URL.to_string())
}

/// The "What happened" message (and location) from a saved report.
fn report_summary(report: &str) -> String {
    let mut lines = report.lines().skip_while(|l| *l != "What happened").skip(2);
    let message = lines.next().unwrap_or("Unknown error").to_string();
    match lines.next().filter(|l| l.starts_with("at ")) {
        Some(at) => format!("{message}\n{at}"),
        None => message,
    }
}

// ---------------------------------------------------------------------------
// The crash window (run as `risquette --crash-report <file>`)
// ---------------------------------------------------------------------------

struct CrashWindow {
    path: PathBuf,
    report: String,
    summary: String,
    show_details: bool,
    copied: bool,
}

impl CrashWindow {
    fn restart(&mut self, cx: &mut Context<Self>) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
        cx.quit();
    }
}

impl Render for CrashWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let red = t.colors.red;

        let header = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .p(px(20.))
            .rounded(px(10.))
            .bg(red.opacity(0.12))
            .border_1()
            .border_color(red.opacity(0.35))
            .child(div().size(px(48.)).flex_none().rounded(px(12.)).bg(red).flex().items_center().justify_center().child(icons::sized(
                Icon::Flag,
                px(24.),
                gpui::white(),
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(t.colors.text).child("Risquette ran into a problem and closed"))
                    .child(
                        div().text_size(t.fs.s).text_color(t.colors.text_light).child("Sorry about that. Anything you hadn't posted yet may have been lost."),
                    ),
            );

        let step = |n: &'static str, title: &'static str, body: &'static str| {
            div()
                .flex()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .rounded_full()
                        .bg(t.colors.brand)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(gpui::white())
                        .child(n),
                )
                .child(
                    // flex_1 + min_w_0 so long text wraps instead of running off the edge.
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_size(t.fs.normal).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text).child(title))
                        .child(div().text_size(t.fs.s).text_color(t.colors.text_light).child(body)),
                )
        };
        let help = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().text_size(t.fs.normal).font_weight(FontWeight::SEMIBOLD).text_color(t.colors.text).child("You can help us fix it"))
            .child(step("1", "Click \"Report the problem\"", "GitHub opens in your browser with the crash details already filled in."))
            .child(step("2", "Tell us what you were doing", "For example \"I opened a post with a video\". Even a short note helps."))
            .child(step(
                "3",
                "Press \"Create\"",
                "You'll need a free GitHub account. If you'd rather not, use \"Copy details\" and share them with someone who can help.",
            ));

        let what = div().flex().flex_col().gap(px(6.)).child(ui::field_label("WHAT WENT WRONG", cx)).child(
            div()
                .p(px(10.))
                .rounded(t.metrics.radius)
                .bg(t.colors.bg)
                .border_1()
                .border_color(t.colors.card_border)
                .font_family("Cascadia Mono")
                .text_size(t.fs.s)
                .text_color(t.colors.text)
                .child(self.summary.clone()),
        );

        let details_toggle = div()
            .id("crash-details-toggle")
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .text_size(t.fs.s)
            .text_color(t.colors.text_light)
            .child(icons::sized(if self.show_details { Icon::ChevronDown } else { Icon::ChevronRight }, px(14.), t.colors.text_light))
            .child(if self.show_details { "Hide technical details" } else { "Show technical details" })
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_details = !this.show_details;
                cx.notify();
            }));

        let details = self.show_details.then(|| {
            div()
                .id("crash-details")
                .max_h(px(220.))
                .overflow_y_scroll()
                .p(px(10.))
                .rounded(t.metrics.radius)
                .bg(t.colors.bg)
                .border_1()
                .border_color(t.colors.card_border)
                .font_family("Cascadia Mono")
                .text_size(px(11.))
                .text_color(t.colors.text_light)
                .child(self.report.clone())
        });

        let path = self.path.clone();
        let report = self.report.clone();
        let issue = issue_url(&self.summary, &self.report);
        let buttons = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(ui::button_main_with_icon("crash-report", Icon::ExternalLink, "Report the problem", cx).on_click(move |_, _, cx| cx.open_url(&issue)))
            .child(ui::button_with_icon("crash-restart", Icon::RefreshCw, "Restart Risquette", cx).on_click(cx.listener(|this, _, _, cx| this.restart(cx))))
            .child(
                ui::button_with_icon("crash-copy", if self.copied { Icon::Check } else { Icon::Copy }, if self.copied { "Copied" } else { "Copy details" }, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(report.clone()));
                        this.copied = true;
                        cx.notify();
                    })),
            )
            .child(ui::button_with_icon("crash-file", Icon::FileText, "Show crash file", cx).on_click(move |_, _, cx| cx.reveal_path(&path)))
            .child(div().flex_1())
            .child(ui::button("crash-close", "Close", cx).on_click(|_, _, cx| cx.quit()));

        div()
            .id("crash-window")
            .size_full()
            .overflow_y_scroll()
            .bg(t.colors.card)
            .text_color(t.colors.text)
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(header)
            .child(help)
            .child(what)
            .child(div().flex().flex_col().gap(px(8.)).child(details_toggle).children(details))
            .child(buttons)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.colors.text_light)
                    .child("The report includes the Risquette version, your system, what went wrong and the app's recent activity log. It never includes your password or login. Have a look before you post it."),
            )
    }
}

/// Show the crash window for the report at `path`. Runs instead of the normal app.
pub fn run_reporter(path: PathBuf) {
    let report = fs::read_to_string(&path).unwrap_or_else(|e| format!("The crash report at {} couldn't be read: {e}", path.display()));
    let summary = report_summary(&report);
    Application::new().with_assets(icons::Assets).run(move |cx: &mut App| {
        let system_dark = matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
        let stored = crate::storage::load();
        let mode = Mode::from_setting(stored.theme.as_deref(), system_dark);
        cx.set_global(Theme::with_contrast(mode, stored.high_contrast));
        let bounds = Bounds::centered(None, size(px(640.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("Risquette crashed".into()), ..Default::default() }),
                window_min_size: Some(size(px(480.), px(420.))),
                app_id: Some("risquette-crash".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|_| CrashWindow { path, report, summary, show_details: false, copied: false }),
        )
        .expect("open crash window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

/// For trying the crash flow: `RISQUETTE_TEST_CRASH=1` panics a few seconds after start.
pub fn schedule_test_crash(cx: &mut App) {
    if std::env::var_os("RISQUETTE_TEST_CRASH").is_none() {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
        cx.update(|_| panic!("Test crash requested with RISQUETTE_TEST_CRASH")).ok();
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details() -> CrashDetails {
        CrashDetails {
            message: "index out of bounds: the len is 3 but the index is 7".into(),
            location: Some("crates\\risquette\\src\\feed.rs:120:9".into()),
            thread: "main".into(),
            backtrace: "   0: risquette::feed::render\n".into(),
            recent_log: vec!["2026-11-07T20:00:00.000Z INFO  risquette: connecting".into()],
            system: "Windows 11 Pro 24H2 (build 26100), x86_64".into(),
            time: chrono::DateTime::parse_from_rfc3339("2026-11-07T20:01:02Z").unwrap().with_timezone(&chrono::Utc),
        }
    }

    #[test]
    fn report_has_the_essentials_and_a_summary() {
        let report = format_report(&details());
        assert!(report.contains(&format!("Version:  {VERSION}")));
        assert!(report.contains("Windows 11 Pro"));
        assert!(report.contains("2026-11-07 20:01:02 UTC"));
        assert!(report.contains("INFO  risquette: connecting"));
        assert_eq!(report_summary(&report), "index out of bounds: the len is 3 but the index is 7\nat crates\\risquette\\src\\feed.rs:120:9");
    }

    #[test]
    fn issue_link_prefills_the_bug_form_and_stays_short() {
        let mut d = details();
        d.recent_log = (0..500).map(|i| format!("log line number {i} with some padding text")).collect();
        let report = format_report(&d);
        let url = issue_url(&report_summary(&report), &report);
        assert!(url.starts_with("https://github.com/tyler-Github/risquette/issues/new?template=bug_report.yml"));
        assert!(url.contains("&version=") && url.contains("&os=") && url.contains("&logs="));
        assert!(url.contains("truncated"));
        assert!(url.len() < 8000, "URL too long: {}", url.len());
    }

    #[test]
    fn linux_distribution_names() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(os_release_name("NAME=Arch Linux\n").as_deref(), Some("Arch Linux"));
        assert_eq!(os_release_name("ID=x\n"), None);
    }

    #[test]
    fn panic_messages_from_str_and_string() {
        assert_eq!(payload_message(&"boom"), "boom");
        assert_eq!(payload_message(&String::from("bang")), "bang");
        assert_eq!(payload_message(&42u8), "(no message)");
    }

    #[test]
    fn rotation_and_pruning() {
        let dir = std::env::temp_dir().join(format!("risquette-crash-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("risquette.log");
        fs::write(&log, vec![b'x'; 100]).unwrap();
        rotate(&log, 50);
        assert!(!log.exists() && dir.join("risquette.1.log").exists());

        for i in 0..5 {
            fs::write(dir.join(format!("crash-20261107-12000{i}.txt")), "r").unwrap();
        }
        prune_reports(&dir, 2);
        let mut left: Vec<String> =
            fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("crash-")).collect();
        left.sort();
        assert_eq!(left, ["crash-20261107-120003.txt", "crash-20261107-120004.txt"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
