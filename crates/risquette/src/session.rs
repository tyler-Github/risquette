//! Login state shared by every view.

use crate::{state::AppState, storage};
use discuit_api::{Community, Initial, Mutes, ReportReason, User};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// User logged in, out, or their data changed.
    Changed,
}

pub struct Session {
    pub user: Option<User>,
    /// Unread (unseen) notifications among those we've loaded.
    pub unread_notifications: i64,
    pub initial: Option<Initial>,
    pub loaded: bool,
    pub error: Option<String>,
    /// The account being switched to, until the reload confirms it's still logged in.
    pending_switch: Option<String>,
    _task: Option<Task<()>>,
}

pub struct SessionGlobal(pub Entity<Session>);

impl Global for SessionGlobal {}

impl EventEmitter<SessionEvent> for Session {}

impl Session {
    pub fn init(cx: &mut App) {
        let session = cx.new(|cx| {
            let mut s = Session { user: None, unread_notifications: 0, initial: None, loaded: false, error: None, pending_switch: None, _task: None };
            s.bootstrap(cx);
            s
        });
        cx.set_global(SessionGlobal(session));
    }

    pub fn entity(cx: &App) -> Entity<Session> {
        cx.global::<SessionGlobal>().0.clone()
    }

    pub fn get(cx: &App) -> &Session {
        cx.global::<SessionGlobal>().0.read(cx)
    }

    // ----- queries -----

    pub fn user(&self) -> Option<&User> {
        self.user.as_ref()
    }

    pub fn is_logged_in(&self) -> bool {
        self.user.is_some()
    }

    pub fn username(&self) -> Option<&str> {
        self.user.as_ref().map(|u| u.username.as_str())
    }

    pub fn is_admin(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.is_admin)
    }

    /// Whether the user moderates the community (by id). Admins count as mods.
    pub fn is_mod_of(&self, community_id: &str) -> bool {
        self.is_admin() || self.user.as_ref().and_then(|u| u.modding_list.as_ref()).is_some_and(|l| l.iter().any(|c| c.id == community_id))
    }

    /// Whether the user is actually on the community's mod list (admins not implied).
    pub fn is_mod_of_strict(&self, community_id: &str) -> bool {
        self.user.as_ref().and_then(|u| u.modding_list.as_ref()).is_some_and(|l| l.iter().any(|c| c.id == community_id))
    }

    /// Joined communities when logged in, otherwise the default set.
    pub fn initial(&self) -> Option<&discuit_api::Initial> {
        self.initial.as_ref()
    }

    pub fn communities(&self) -> &[Community] {
        self.initial.as_ref().and_then(|i| i.communities.as_deref()).unwrap_or(&[])
    }

    pub fn report_reasons(&self) -> &[ReportReason] {
        self.initial.as_ref().and_then(|i| i.report_reasons.as_deref()).unwrap_or(&[])
    }

    pub fn mutes(&self) -> Option<&Mutes> {
        self.initial.as_ref().and_then(|i| i.mutes.as_ref())
    }

    pub fn signups_disabled(&self) -> bool {
        self.initial.as_ref().is_some_and(|i| i.signups_disabled)
    }

    pub fn notifications_new_count(&self) -> i64 {
        self.user.as_ref().map_or(0, |u| u.notifications_new_count)
    }

    /// What the bell shows: notifications that are new (arrived since the page
    /// was last opened) or still unread, whichever is larger.
    pub fn notification_badge(&self) -> i64 {
        if self.user.is_none() {
            return 0;
        }
        self.notifications_new_count().max(self.unread_notifications)
    }

    pub fn set_unread_notifications(&mut self, n: i64, cx: &mut Context<Self>) {
        if self.unread_notifications != n {
            self.unread_notifications = n;
            cx.notify();
        }
    }

    // ----- lifecycle -----

    /// Start over against the current client (after switching instance).
    pub fn reload_for_instance(&mut self, cx: &mut Context<Self>) {
        self.user = None;
        self.initial = None;
        self.unread_notifications = 0;
        self.loaded = false;
        cx.emit(SessionEvent::Changed);
        self.bootstrap(cx);
    }

    fn bootstrap(&mut self, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let base = client.base_url().to_string();
        if let Some(cookies) = storage::load().sessions.get(&base) {
            client.import_cookies(cookies);
        }
        self.refresh(cx);
    }

    /// Re-fetch `/api/_initial` (and `/api/_user` when logged in).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let task = cx.background_executor().spawn(async move {
            let initial = client.initial()?;
            // `_initial` omits `moddingList`; fetch the full user when logged in.
            let user = match &initial.user {
                Some(_) => client.me().ok().or(initial.user.clone()),
                None => None,
            };
            persist_cookies(&client);
            Ok::<_, discuit_api::Error>((initial, user))
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.loaded = true;
                match result {
                    Ok((initial, user)) => {
                        this.user = user;
                        this.initial = Some(initial);
                        this.error = None;
                        if let Some(expected) = this.pending_switch.take()
                            && !this.user.as_ref().is_some_and(|u| u.username.eq_ignore_ascii_case(&expected))
                        {
                            // The saved login has expired or was logged out elsewhere.
                            let base = AppState::client(cx).base_url().to_string();
                            storage::update(|s| {
                                s.forget_account(&base, &expected);
                            });
                            crate::nav::toast(format!("@{expected} was logged out. Log in again to use that account."), cx);
                        }
                        this.remember_current_account(cx);
                    }
                    Err(e) => {
                        log::error!("bootstrap: {e}");
                        this.error = Some(e.user_message());
                    }
                }
                cx.emit(SessionEvent::Changed);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Log in. Resolves to an error message on failure.
    pub fn login(&mut self, username: String, password: String, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let client = AppState::client(cx);
        let task = cx.background_executor().spawn(async move {
            // A GET first guarantees we hold a CSRF cookie.
            if client.export_cookies().is_empty() {
                client.initial()?;
            }
            client.login(username.trim(), &password)?;
            let user = client.me()?;
            let initial = client.initial()?;
            persist_cookies(&client);
            Ok::<_, discuit_api::Error>((user, initial))
        });
        cx.spawn(async move |this, cx| {
            let (user, initial) = task.await.map_err(|e| e.user_message())?;
            this.update(cx, |this, cx| {
                this.user = Some(user);
                this.initial = Some(initial);
                this.remember_current_account(cx);
                cx.emit(SessionEvent::Changed);
                cx.notify();
            })
            .map_err(|e| e.to_string())
        })
    }

    /// Log in to another account while keeping the current one saved for switching.
    /// If the login fails, the current account stays active.
    pub fn add_account(&mut self, username: String, password: String, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let client = AppState::client(cx);
        let task = cx.background_executor().spawn(async move {
            let previous = client.export_cookies();
            // A fresh session, so the current account's login isn't replaced.
            client.clear_cookies();
            let result = (|| {
                client.initial()?;
                client.login(username.trim(), &password)?;
                let user = client.me()?;
                let initial = client.initial()?;
                Ok::<_, discuit_api::Error>((user, initial))
            })();
            if result.is_err() {
                client.clear_cookies();
                client.import_cookies(&previous);
            }
            persist_cookies(&client);
            result
        });
        cx.spawn(async move |this, cx| {
            let (user, initial) = task.await.map_err(|e| e.user_message())?;
            this.update(cx, |this, cx| {
                this.user = Some(user);
                this.initial = Some(initial);
                this.unread_notifications = 0;
                this.remember_current_account(cx);
                cx.emit(SessionEvent::Changed);
                cx.notify();
            })
            .map_err(|e| e.to_string())
        })
    }

    /// Accounts saved on the current instance (including the active one).
    pub fn saved_accounts(&self, cx: &App) -> Vec<storage::SavedAccount> {
        storage::load().accounts_on(AppState::client(cx).base_url().as_str())
    }

    /// Make a saved account the active one and reload everything for it.
    pub fn switch_account(&mut self, username: &str, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let base = client.base_url().to_string();
        let Some(target) = storage::load().accounts_on(&base).into_iter().find(|a| a.username.eq_ignore_ascii_case(username)) else { return };
        if self.username().is_some_and(|u| u.eq_ignore_ascii_case(username)) {
            return;
        }
        self.remember_current_account(cx);
        client.clear_cookies();
        client.import_cookies(&target.cookies);
        storage::update(|s| {
            s.sessions.insert(base, target.cookies.clone());
        });
        self.pending_switch = Some(target.username.clone());
        self.user = None;
        self.initial = None;
        self.unread_notifications = 0;
        self.loaded = false;
        cx.emit(SessionEvent::Changed);
        cx.notify();
        self.refresh(cx);
        crate::nav::toast(format!("Switched to @{}", target.username), cx);
    }

    /// Forget a saved account that isn't the active one, and end its session on the server.
    pub fn remove_account(&mut self, username: &str, cx: &mut Context<Self>) {
        if self.username().is_some_and(|u| u.eq_ignore_ascii_case(username)) {
            self.logout(cx);
            return;
        }
        let base = AppState::client(cx).base_url().to_string();
        let mut removed = None;
        storage::update(|s| removed = s.forget_account(&base, username));
        if let Some(account) = removed {
            end_session(account.instance, account.cookies, cx);
        }
        cx.notify();
    }

    /// Save the active login (cookies, username, picture) for account switching.
    fn remember_current_account(&self, cx: &App) {
        let Some(user) = &self.user else { return };
        let client = AppState::client(cx);
        let cookies = client.export_cookies();
        if cookies.is_empty() {
            return;
        }
        let account = storage::SavedAccount {
            instance: client.base_url().to_string(),
            username: user.username.clone(),
            cookies,
            avatar: user.pro_pic.as_ref().map(|i| i.best_url(64).to_string()),
        };
        storage::update(|s| s.remember_account(account));
    }

    /// Create an account (logs in on success).
    pub fn signup(&mut self, username: String, email: String, password: String, captcha: Option<String>, cx: &mut Context<Self>) -> Task<Result<(), String>> {
        let client = AppState::client(cx);
        let task = cx.background_executor().spawn(async move {
            if client.export_cookies().is_empty() {
                client.initial()?;
            }
            client.signup(username.trim(), email.trim(), &password, captcha.as_deref())?;
            let user = client.me()?;
            let initial = client.initial()?;
            persist_cookies(&client);
            Ok::<_, discuit_api::Error>((user, initial))
        });
        cx.spawn(async move |this, cx| {
            let (user, initial) = task.await.map_err(|e| {
                if e.code().is_some_and(|c| c.starts_with("captcha")) {
                    "This instance requires a captcha to sign up. Please create your account on the website, then log in here.".to_string()
                } else {
                    e.user_message()
                }
            })?;
            this.update(cx, |this, cx| {
                this.user = Some(user);
                this.initial = Some(initial);
                this.remember_current_account(cx);
                cx.emit(SessionEvent::Changed);
                cx.notify();
            })
            .map_err(|e| e.to_string())
        })
    }

    /// Log out of the active account. If other accounts are saved on this
    /// instance, switch to the next one; otherwise browse logged out.
    pub fn logout(&mut self, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let base = client.base_url().to_string();
        if let Some(username) = self.username().map(str::to_string) {
            storage::update(|s| {
                s.forget_account(&base, &username);
            });
        }
        if let Some(next) = storage::load().accounts_on(&base).into_iter().next() {
            // End this session on the server, then continue as the next account.
            end_session(base, client.export_cookies(), cx);
            self.user = None;
            self.switch_account(&next.username, cx);
            return;
        }
        let task = cx.background_executor().spawn(async move {
            if let Err(e) = client.logout() {
                log::warn!("logout: {e}");
            }
            client.clear_cookies();
            persist_cookies(&client);
            // Re-establish an anonymous session.
            client.initial().ok()
        });
        self.user = None;
        cx.emit(SessionEvent::Changed);
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let initial = task.await;
            this.update(cx, |this, cx| {
                this.initial = initial;
                cx.emit(SessionEvent::Changed);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Replace the user (after a settings change, pro-pic upload, …).
    pub fn set_user(&mut self, user: User, cx: &mut Context<Self>) {
        // Keep moddingList if the new object lacks it.
        let modding = self.user.as_ref().and_then(|u| u.modding_list.clone());
        let mut user = user;
        if user.modding_list.is_none() {
            user.modding_list = modding;
        }
        self.user = Some(user);
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    pub fn set_mutes(&mut self, mutes: Mutes, cx: &mut Context<Self>) {
        if let Some(i) = self.initial.as_mut() {
            i.mutes = Some(mutes);
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    /// Update a community's joined state in the sidebar list.
    pub fn community_updated(&mut self, community: Community, cx: &mut Context<Self>) {
        if let Some(list) = self.initial.as_mut().and_then(|i| i.communities.as_mut()) {
            let joined = community.user_joined.unwrap_or(false);
            if let Some(pos) = list.iter().position(|c| c.id == community.id) {
                if joined {
                    list[pos] = community;
                } else if self.user.is_some() {
                    list.remove(pos);
                }
            } else if joined {
                list.push(community);
                list.sort_by_key(|c| c.name.to_lowercase());
            }
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    pub fn set_notification_count(&mut self, n: i64, cx: &mut Context<Self>) {
        if let Some(u) = self.user.as_mut() {
            u.notifications_new_count = n;
            cx.notify();
        }
    }
}

/// Log out a session (by its cookies) in the background, without touching the active one.
fn end_session(instance: String, cookies: String, cx: &App) {
    cx.background_executor()
        .spawn(async move {
            match discuit_api::Client::new(&instance) {
                Ok(other) => {
                    other.import_cookies(&cookies);
                    if let Err(e) = other.logout() {
                        log::warn!("logging out a saved account: {e}");
                    }
                }
                Err(e) => log::warn!("logging out a saved account: {e}"),
            }
        })
        .detach();
}

fn persist_cookies(client: &discuit_api::Client) {
    let base = client.base_url().to_string();
    let cookies = client.export_cookies();
    storage::update(|s| {
        if cookies.is_empty() {
            s.sessions.remove(&base);
        } else {
            s.sessions.insert(base, cookies);
        }
    });
}
