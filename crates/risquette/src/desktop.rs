//! Desktop integration: one running copy per settings profile, and desktop
//! notifications for new Discuit notifications.
//!
//! - **One window.** Each settings folder gets its own lock, so test or portable
//!   profiles don't block each other. A second launch asks the running copy to
//!   come to the front, then exits.
//!   - Windows: a named mutex, and `instance.pid` to find the running window.
//!   - Linux: an exclusive lock on `instance.lock`, and a local socket the second
//!     launch uses to say "show yourself".
//! - **Notifications.** Opt-in (off by default), only while Risquette isn't the
//!   active window. Clicking one opens it in the app.
//!   - Windows: through a tray icon (`Shell_NotifyIcon` balloons, which Windows
//!     10/11 show as toasts without any app registration).
//!   - Linux: the freedesktop notification service over D-Bus; no tray icon.

use crate::{
    nav::{self, Target},
    session::Session,
    storage,
};
use discuit_api::Notification;
use gpui::{App, Global, Window};
use std::{collections::HashSet, sync::OnceLock};

/// What the tray icon, a notification or a second launch asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum TrayEvent {
    Open,
    NotificationClicked,
    Quit,
}

/// Events from other threads (tray window, notification clicks, the socket),
/// delivered to the UI thread by the task started in [`init`].
static EVENTS: OnceLock<smol::channel::Sender<TrayEvent>> = OnceLock::new();

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn send_event(event: TrayEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.try_send(event);
    }
}

/// The main window, so other platforms can bring it to the front through GPUI.
#[cfg(not(windows))]
struct MainWindow(gpui::AnyWindowHandle);

#[cfg(not(windows))]
impl Global for MainWindow {}

// ---------------------------------------------------------------------------
// One running copy
// ---------------------------------------------------------------------------

pub enum Instance {
    /// This is the only copy for this profile.
    First,
    /// Another copy is already running for this profile.
    AlreadyRunning,
}

/// A short, stable key for the settings folder, so each profile gets its own lock.
fn profile_key() -> String {
    let dir = storage::dir().map(|d| d.to_string_lossy().to_lowercase()).unwrap_or_default();
    let hash = dir.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{hash:016x}")
}

fn pid_file() -> Option<std::path::PathBuf> {
    Some(storage::dir()?.join("instance.pid"))
}

/// Claim this profile for this process.
pub fn claim_instance() -> Instance {
    #[cfg(windows)]
    {
        if win::claim(&format!("Local\\Risquette-{}", profile_key())) {
            if let Some(file) = pid_file() {
                let _ = std::fs::create_dir_all(file.parent().unwrap_or(&file));
                let _ = std::fs::write(file, std::process::id().to_string());
            }
            return Instance::First;
        }
        Instance::AlreadyRunning
    }
    #[cfg(target_os = "linux")]
    {
        let Some(dir) = storage::dir() else { return Instance::First };
        let _ = std::fs::create_dir_all(&dir);
        if linux::claim(&dir.join("instance.lock"), &profile_key()) { Instance::First } else { Instance::AlreadyRunning }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    Instance::First
}

/// Bring the already-running copy's window to the front. Returns whether it was found.
pub fn focus_running_instance() -> bool {
    #[cfg(windows)]
    {
        let Some(pid) = pid_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| s.trim().parse::<u32>().ok()) else { return false };
        // The other copy may still be opening its window.
        for _ in 0..20 {
            if win::focus_process_window(pid, "Risquette") {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        false
    }
    #[cfg(target_os = "linux")]
    {
        // The other copy may still be starting up.
        for _ in 0..20 {
            if linux::ask_running_copy_to_show(&profile_key()) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        false
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    false
}

// ---------------------------------------------------------------------------
// Which notifications are new
// ---------------------------------------------------------------------------

/// Notifications already seen by the poller, per account.
#[derive(Default)]
pub struct SeenNotifications {
    user: Option<String>,
    ids: HashSet<i64>,
}

impl SeenNotifications {
    /// Ids of unread notifications that weren't there on the previous poll.
    /// The first poll for an account only records what's there, so starting the
    /// app (or switching accounts) doesn't announce old notifications.
    pub fn fresh(&mut self, user: &str, items: &[(i64, bool)]) -> Vec<i64> {
        let first_poll = self.user.as_deref() != Some(user);
        if first_poll {
            self.user = Some(user.to_string());
            self.ids.clear();
        }
        let mut fresh = Vec::new();
        for &(id, seen) in items {
            if self.ids.insert(id) && !seen && !first_poll {
                fresh.push(id);
            }
        }
        fresh
    }
}

#[derive(Default)]
struct NotifierState {
    seen: SeenNotifications,
    /// Where clicking the last desktop notification should go.
    target: Option<Target>,
}

impl Global for NotifierState {}

/// The in-app page for a notification's link (`/gaming/post/abc`, or a full URL).
fn target_for(n: &Notification) -> Option<Target> {
    let link = n.to_url.as_deref()?;
    let path = if link.starts_with('/') { link.to_string() } else { url::Url::parse(link).ok()?.path().to_string() };
    nav::target_for_path(&path)
}

/// The title and text of a desktop notification for `new` notifications.
fn message_for(new: &[&Notification]) -> (String, String, Target) {
    let plain = |s: Option<&str>, n: usize| s.map(|s| crate::markdown::plain_preview(s, n)).unwrap_or_default();
    match new {
        [one] => {
            let title = plain(one.title.as_deref(), 120);
            let title = if title.is_empty() { one.kind.replace('_', " ") } else { title };
            (title, plain(one.body.as_deref(), 240), target_for(one).unwrap_or(Target::Notifications))
        }
        many => {
            let titles: Vec<String> = many.iter().take(3).map(|n| plain(n.title.as_deref(), 80)).filter(|t| !t.is_empty()).collect();
            (format!("{} new notifications", many.len()), titles.join("\n"), Target::Notifications)
        }
    }
}

/// Called with each page of notifications the app polls: shows a desktop
/// notification for anything new, when enabled and Risquette isn't in front.
pub fn on_notifications(items: &[Notification], cx: &mut App) {
    let Some(user) = Session::get(cx).username().map(str::to_string) else { return };
    let ids: Vec<(i64, bool)> = items.iter().map(|n| (n.id, n.seen)).collect();
    let fresh = cx.default_global::<NotifierState>().seen.fresh(&user, &ids);
    if fresh.is_empty() || !storage::load().desktop_notifications || cx.active_window().is_some() {
        return;
    }
    let new: Vec<&Notification> = items.iter().filter(|n| fresh.contains(&n.id)).collect();
    let (title, body, target) = message_for(&new);
    cx.default_global::<NotifierState>().target = Some(target);
    show_notification(&title, &body);
}

fn show_notification(title: &str, body: &str) {
    #[cfg(windows)]
    win::show_notification(title, body);
    #[cfg(target_os = "linux")]
    linux::show_notification(title, body);
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = (title, body);
}

// ---------------------------------------------------------------------------
// Tray icon
// ---------------------------------------------------------------------------

/// Start listening for tray, notification and second-launch events. Call once, after the main window opens.
pub fn init(window: &Window, cx: &mut App) {
    let (tx, rx) = smol::channel::unbounded::<TrayEvent>();
    let _ = EVENTS.set(tx);
    cx.spawn(async move |cx| {
        while let Ok(event) = rx.recv().await {
            if cx.update(|cx| handle_event(event, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    #[cfg(windows)]
    {
        use raw_window_handle::HasWindowHandle;
        use wry::raw_window_handle;
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw()
        {
            win::set_main_window(h.hwnd.get());
        }
        cx.on_app_quit(|_| {
            win::remove_tray();
            async {}
        })
        .detach();
    }
    #[cfg(not(windows))]
    cx.set_global(MainWindow(window.window_handle()));
    #[cfg(target_os = "linux")]
    linux::listen_for_second_launch();
    sync_tray(cx);
    schedule_test_notification(cx);
}

/// Bring the main window to the front (restoring it if minimized, where the platform allows).
fn bring_to_front(cx: &mut App) {
    #[cfg(windows)]
    {
        let _ = cx;
        win::bring_main_window_to_front();
    }
    #[cfg(not(windows))]
    if let Some(handle) = cx.try_global::<MainWindow>().map(|m| m.0) {
        let _ = cx.update_window(handle, |_, window, _| window.activate_window());
    }
}

/// For trying notifications without an account: `RISQUETTE_TEST_NOTIFICATION=1`
/// shows the tray icon and a sample notification a few seconds after start.
fn schedule_test_notification(cx: &mut App) {
    if std::env::var_os("RISQUETTE_TEST_NOTIFICATION").is_none() {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
        cx.update(|cx| {
            cx.default_global::<NotifierState>().target = Some(Target::Notifications);
            #[cfg(windows)]
            win::show_tray();
            show_notification("Risquette test notification", "If you can see this, desktop notifications work. Click it to open Notifications.");
        })
        .ok();
    })
    .detach();
}

/// Show the tray icon while notifications are on and someone is logged in; hide it otherwise.
pub fn sync_tray(cx: &mut App) {
    let wanted = storage::load().desktop_notifications && Session::get(cx).is_logged_in();
    #[cfg(windows)]
    {
        if wanted {
            win::show_tray();
        } else {
            win::remove_tray();
        }
    }
    #[cfg(not(windows))]
    let _ = wanted;
}

fn handle_event(event: TrayEvent, cx: &mut App) {
    match event {
        TrayEvent::Open => bring_to_front(cx),
        TrayEvent::NotificationClicked => {
            bring_to_front(cx);
            let target = cx.default_global::<NotifierState>().target.take().unwrap_or(Target::Notifications);
            nav::go(target, cx);
        }
        TrayEvent::Quit => cx.quit(),
    }
}

#[cfg(windows)]
mod win {
    use super::{TrayEvent, send_event as send};
    use std::cell::Cell;
    use windows::{
        Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM},
            System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
            UI::{
                Shell::{
                    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
                    NIN_BALLOONUSERCLICK, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
                },
                WindowsAndMessaging::*,
            },
        },
        core::{BOOL, HSTRING, PCWSTR, w},
    };

    const CLASS: PCWSTR = w!("RisquetteTray");
    const CALLBACK: u32 = WM_APP + 1;
    const MENU_OPEN: usize = 1;
    const MENU_QUIT: usize = 2;

    thread_local! {
        static MAIN: Cell<isize> = const { Cell::new(0) };
        static TRAY: Cell<isize> = const { Cell::new(0) };
        static VISIBLE: Cell<bool> = const { Cell::new(false) };
    }

    /// Take the named mutex; false if another process already holds it.
    pub fn claim(name: &str) -> bool {
        match unsafe { CreateMutexW(None, true, &HSTRING::from(name)) } {
            // The handle stays open for the life of the process.
            Ok(_) => (unsafe { GetLastError() }) != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }

    struct Search {
        pid: u32,
        title: Vec<u16>,
        found: HWND,
    }

    unsafe extern "system" fn find_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let mut buf = [0u16; 128];
            let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
            if buf[..len] == search.title[..] {
                search.found = hwnd;
                return false.into();
            }
        }
        true.into()
    }

    fn bring_to_front(hwnd: HWND) {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// Find the window titled `title` in process `pid` and bring it to the front.
    pub fn focus_process_window(pid: u32, title: &str) -> bool {
        let mut search = Search { pid, title: title.encode_utf16().collect(), found: HWND::default() };
        unsafe {
            let _ = EnumWindows(Some(find_window), LPARAM(&mut search as *mut Search as isize));
        }
        if search.found.is_invalid() {
            return false;
        }
        bring_to_front(search.found);
        true
    }

    pub fn set_main_window(hwnd: isize) {
        MAIN.with(|m| m.set(hwnd));
    }

    pub fn bring_main_window_to_front() {
        let hwnd = MAIN.with(|m| m.get());
        if hwnd != 0 {
            bring_to_front(HWND(hwnd as *mut _));
        }
    }

    fn show_menu(hwnd: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open Risquette"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit Risquette"));
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Required so the menu closes when clicking elsewhere.
            let _ = SetForegroundWindow(hwnd);
            let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
            let _ = DestroyMenu(menu);
            match choice.0 as usize {
                MENU_OPEN => send(TrayEvent::Open),
                MENU_QUIT => send(TrayEvent::Quit),
                _ => {}
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == CALLBACK {
            // NOTIFYICON_VERSION_4: the event is in the low word of lParam.
            match (lparam.0 as u32) & 0xffff {
                NIN_BALLOONUSERCLICK => send(TrayEvent::NotificationClicked),
                NIN_SELECT | WM_LBUTTONUP => send(TrayEvent::Open),
                WM_CONTEXTMENU | WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// The hidden window that receives tray clicks (created once).
    fn tray_window() -> Option<HWND> {
        let existing = TRAY.with(|t| t.get());
        if existing != 0 {
            return Some(HWND(existing as *mut _));
        }
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: CLASS, ..Default::default() };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS,
                w!("Risquette tray"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            TRAY.with(|t| t.set(hwnd.0 as isize));
            Some(hwnd)
        }
    }

    fn app_icon() -> HICON {
        unsafe {
            // Resource 1 is the app icon embedded by build.rs.
            GetModuleHandleW(None)
                .ok()
                .and_then(|m| LoadIconW(Some(m.into()), PCWSTR(1 as _)).ok())
                .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                .unwrap_or_default()
        }
    }

    fn copy_wide(dst: &mut [u16], text: &str) {
        let wide: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
        dst[..wide.len()].copy_from_slice(&wide);
        dst[wide.len()] = 0;
    }

    fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
    }

    pub fn show_tray() {
        if VISIBLE.with(|v| v.get()) {
            return;
        }
        let Some(hwnd) = tray_window() else { return };
        let mut data = base_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = CALLBACK;
        data.hIcon = app_icon();
        copy_wide(&mut data.szTip, "Risquette");
        unsafe {
            if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
                VISIBLE.with(|v| v.set(true));
                log::debug!("tray icon added");
            } else {
                log::warn!("couldn't add the tray icon");
            }
        }
    }

    pub fn remove_tray() {
        if !VISIBLE.with(|v| v.get()) {
            return;
        }
        let Some(hwnd) = tray_window() else { return };
        let data = base_data(hwnd);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
        VISIBLE.with(|v| v.set(false));
    }

    pub fn show_notification(title: &str, body: &str) {
        if !VISIBLE.with(|v| v.get()) {
            return;
        }
        let Some(hwnd) = tray_window() else { return };
        let mut data = base_data(hwnd);
        data.uFlags = NIF_INFO;
        copy_wide(&mut data.szInfoTitle, title);
        // Windows shows nothing for an empty body, so fall back to a space.
        copy_wide(&mut data.szInfo, if body.trim().is_empty() { " " } else { body });
        data.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
        data.hBalloonIcon = app_icon();
        unsafe {
            if Shell_NotifyIconW(NIM_MODIFY, &data).as_bool() {
                log::debug!("desktop notification shown: {title}");
            } else {
                log::warn!("couldn't show a desktop notification");
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{TrayEvent, send_event};
    use std::{
        fs::{File, TryLockError},
        io::{Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::{Path, PathBuf},
        sync::OnceLock,
        time::Duration,
    };

    /// Held for the life of the process: the lock that makes this the only copy.
    static LOCK: OnceLock<File> = OnceLock::new();
    static LISTENER: OnceLock<UnixListener> = OnceLock::new();

    fn socket_path(key: &str) -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        dir.join(format!("risquette-{key}.sock"))
    }

    /// Take the profile's lock; false if another copy holds it.
    pub fn claim(lock_path: &Path, key: &str) -> bool {
        let Ok(file) = File::options().create(true).truncate(false).write(true).open(lock_path) else { return true };
        match file.try_lock() {
            Ok(()) => {
                let _ = LOCK.set(file);
                // We own the profile now, so any socket file left behind is stale.
                let path = socket_path(key);
                let _ = std::fs::remove_file(&path);
                match UnixListener::bind(&path) {
                    Ok(listener) => {
                        let _ = LISTENER.set(listener);
                    }
                    Err(e) => log::warn!("single-instance socket: {e}"),
                }
                true
            }
            Err(TryLockError::WouldBlock) => false,
            // Locking isn't supported here (unusual file system): don't block startup.
            Err(TryLockError::Error(e)) => {
                log::warn!("single-instance lock: {e}");
                true
            }
        }
    }

    /// Accept "show" requests from later launches (on a background thread).
    pub fn listen_for_second_launch() {
        let Some(listener) = LISTENER.get().and_then(|l| l.try_clone().ok()) else { return };
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut buf = [0u8; 16];
                if let Ok(n) = stream.read(&mut buf)
                    && buf[..n].starts_with(b"show")
                {
                    send_event(TrayEvent::Open);
                }
            }
        });
    }

    /// Ask the running copy to come to the front; false if it couldn't be reached.
    pub fn ask_running_copy_to_show(key: &str) -> bool {
        UnixStream::connect(socket_path(key)).and_then(|mut s| s.write_all(b"show\n")).is_ok()
    }

    /// Show a desktop notification; clicking it opens the item in the app.
    pub fn show_notification(title: &str, body: &str) {
        let (title, body) = (title.to_string(), body.to_string());
        // Waiting for the click blocks, so it gets its own thread.
        std::thread::spawn(move || {
            // Inside an AppImage the icon file sits at its root; otherwise rely on the icon theme.
            let icon = std::env::var_os("APPDIR")
                .map(|dir| PathBuf::from(dir).join("risquette.png"))
                .filter(|p| p.exists())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "risquette".to_string());
            let shown = notify_rust::Notification::new().appname("Risquette").summary(&title).body(&body).icon(&icon).action("default", "Open").show();
            match shown {
                Ok(handle) => handle.wait_for_action(|action| {
                    if action == "default" {
                        send_event(TrayEvent::NotificationClicked);
                    }
                }),
                Err(e) => log::warn!("couldn't show a desktop notification: {e}"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_poll_records_without_announcing() {
        let mut seen = SeenNotifications::default();
        assert!(seen.fresh("alice", &[(1, false), (2, true)]).is_empty());
        // A new unread one is announced once; seen ones never are.
        assert_eq!(seen.fresh("alice", &[(3, false), (4, true), (1, false)]), [3]);
        assert!(seen.fresh("alice", &[(3, false)]).is_empty());
        // Switching account starts over.
        assert!(seen.fresh("bob", &[(5, false)]).is_empty());
        assert_eq!(seen.fresh("bob", &[(6, false), (5, false)]), [6]);
    }

    fn note(id: i64, title: &str, to: &str) -> Notification {
        serde_json::from_value(serde_json::json!({ "id": id, "type": "comment_reply", "title": title, "toURL": to })).unwrap()
    }

    #[test]
    fn one_notification_links_to_its_page() {
        let n = note(1, "**@bob** replied to your comment", "/gaming/post/abc123");
        let (title, _, target) = message_for(&[&n]);
        assert_eq!(title, "@bob replied to your comment");
        assert!(matches!(target, Target::Post { .. }), "{target:?}");
    }

    #[test]
    fn several_notifications_are_summarised() {
        let (a, b) = (note(1, "@bob replied", "/x/post/1"), note(2, "@cat replied", "https://discuit.org/x/post/2"));
        let (title, body, target) = message_for(&[&a, &b]);
        assert_eq!(title, "2 new notifications");
        assert_eq!(body, "@bob replied\n@cat replied");
        assert_eq!(target, Target::Notifications);
    }

    #[test]
    fn profile_key_is_stable() {
        assert_eq!(profile_key(), profile_key());
        assert_eq!(profile_key().len(), 16);
    }
}
