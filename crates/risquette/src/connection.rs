//! Whether the Discuit server can be reached, and catching up once it can again.
//!
//! The API client reports when requests stop getting through and when they
//! start again. While the server can't be reached, the window shows a
//! reconnecting screen (after a short grace period, so a single blip doesn't
//! flash it) and the server is checked every so often (1 s, then backing off to
//! 30 s), or straight away with "Try now". "Use a different server" sets the
//! screen aside for a slim banner, so Settings stays reachable. When the server
//! answers again:
//!
//! - images that failed to download are fetched again,
//! - a session that couldn't load (starting offline) is loaded,
//! - pages that failed to load reload themselves ([`on_reconnect`]).
//!
//! Only loading is retried: nothing that sends or changes data is repeated.

use crate::{session::Session, state::AppState};
use discuit_api::Client;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Subscription, Task};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

/// How long the server must stay unreachable before the reconnecting screen shows.
const GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionEvent {
    Lost,
    Restored,
}

pub struct Connection {
    online: bool,
    /// When the server stopped answering.
    lost_at: Option<Instant>,
    /// When the next automatic check is due.
    next_check: Option<Instant>,
    /// The reconnecting screen was set aside ("Use a different server").
    dismissed: bool,
    /// A check is in flight.
    checking: bool,
    /// Checks the server every so often while offline.
    _probe: Option<Task<()>>,
}

impl EventEmitter<ConnectionEvent> for Connection {}

impl Connection {
    pub fn is_checking(&self) -> bool {
        self.checking
    }

    /// Show the full reconnecting screen (offline for a moment, and not set aside).
    pub fn show_screen(&self) -> bool {
        !self.online && !self.dismissed && self.lost_at.is_some_and(|t| t.elapsed() >= GRACE)
    }

    /// Show the slim banner instead (offline, with the screen set aside).
    pub fn show_banner(&self) -> bool {
        !self.online && self.dismissed
    }

    /// Whole seconds until the next automatic check.
    pub fn seconds_to_next_check(&self) -> Option<u64> {
        self.next_check.map(|t| t.saturating_duration_since(Instant::now()).as_secs_f32().ceil() as u64)
    }

    /// Set the reconnecting screen aside, leaving the banner.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.dismissed = true;
        cx.notify();
    }

    /// Ask the server whether it can be reached; the client reports the answer.
    pub fn check_now(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        cx.notify();
        let client = AppState::client(cx);
        // Any answer counts, so the home page (a small, read-only GET) will do.
        let task = cx.background_executor().spawn(async move {
            let _ = client.fetch_bytes("/");
        });
        cx.spawn(async move |this, cx| {
            task.await;
            this.update(cx, |c, cx| {
                c.checking = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Check the server now and then, re-rendering every second for the
    /// countdown (and to bring up the screen once the grace period is over).
    fn start_probing(&mut self, cx: &mut Context<Self>) {
        self._probe = Some(cx.spawn(async move |this, cx| {
            let mut delay = 1;
            loop {
                let due = Instant::now() + Duration::from_secs(delay);
                let alive = this.update(cx, |c, cx| {
                    c.next_check = Some(due);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
                while Instant::now() < due {
                    cx.background_executor().timer(Duration::from_millis(250).min(due - Instant::now())).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
                let started = this.update(cx, |c, cx| {
                    c.next_check = None;
                    c.check_now(cx);
                });
                if started.is_err() {
                    break;
                }
                // Wait for the answer before counting down to the next check.
                while this.update(cx, |c, _| c.checking).unwrap_or(false) {
                    cx.background_executor().timer(Duration::from_millis(250)).await;
                }
                delay = (delay * 2).min(30);
            }
        }));
    }
}

struct ConnectionGlobal(Entity<Connection>);

impl Global for ConnectionGlobal {}

/// Reports from the client, which arrive on background threads.
static REPORTS: OnceLock<smol::channel::Sender<bool>> = OnceLock::new();

/// Start following the connection. Call before the first request, so a
/// failure at startup isn't missed (the client only reports changes).
pub fn init(cx: &mut App) {
    let connection = cx.new(|_| Connection { online: true, lost_at: None, next_check: None, dismissed: false, checking: false, _probe: None });
    cx.set_global(ConnectionGlobal(connection));
    let (tx, rx) = smol::channel::unbounded();
    let _ = REPORTS.set(tx);
    cx.spawn(async move |cx| {
        while let Ok(reachable) = rx.recv().await {
            if cx.update(|cx| set_reachable(reachable, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    watch(&AppState::client(cx));
}

/// Have `client` report to us. Every new client needs this (after switching instance).
pub fn watch(client: &Client) {
    client.on_reachability(|reachable| {
        if let Some(tx) = REPORTS.get() {
            let _ = tx.try_send(reachable);
        }
    });
}

pub fn entity(cx: &App) -> Entity<Connection> {
    cx.global::<ConnectionGlobal>().0.clone()
}

/// Run `retry` on the view when the connection comes back, to reload what failed.
pub fn on_reconnect<V: 'static>(cx: &mut Context<V>, retry: impl Fn(&mut V, &mut Context<V>) + 'static) -> Subscription {
    cx.subscribe(&entity(cx), move |view, _, event: &ConnectionEvent, cx| {
        if *event == ConnectionEvent::Restored {
            retry(view, cx);
        }
    })
}

fn set_reachable(reachable: bool, cx: &mut App) {
    // (changed, and the outage lasted long enough to have been shown)
    let (changed, noticed) = entity(cx).update(cx, |c, cx| {
        if c.online == reachable {
            return (false, false);
        }
        let noticed = c.lost_at.is_some_and(|t| t.elapsed() >= GRACE);
        c.online = reachable;
        c.dismissed = false;
        c.next_check = None;
        c.lost_at = if reachable { None } else { Some(Instant::now()) };
        if reachable {
            c._probe = None;
            log::info!("the server can be reached again");
            cx.emit(ConnectionEvent::Restored);
        } else {
            log::warn!("the server can't be reached; checking again shortly");
            c.start_probing(cx);
            cx.emit(ConnectionEvent::Lost);
        }
        cx.notify();
        (true, noticed)
    });
    if changed && reachable {
        catch_up(noticed, cx);
    }
}

/// Fetch again what couldn't be fetched while offline. `announce`: say so with a toast.
fn catch_up(announce: bool, cx: &mut App) {
    crate::state::ImageCache::forget_failed(cx);
    let session = Session::entity(cx);
    if session.read(cx).error.is_some() {
        session.update(cx, |s, cx| s.refresh(cx));
    }
    if announce {
        crate::nav::toast("Back online", cx);
    }
    cx.refresh_windows();
}
