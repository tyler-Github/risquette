//! Small on-disk state: saved logins and UI preferences.
//!
//! Stored as JSON in the platform config directory
//! (`%APPDATA%\Risquette` on Windows, `~/.config/risquette` on Linux,
//! `~/Library/Application Support/Risquette` on macOS).

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stored {
    /// Cookies of the active login per instance base URL.
    #[serde(default)]
    pub sessions: BTreeMap<String, String>,
    /// Every account the user is logged in to, across instances (for switching).
    #[serde(default)]
    pub accounts: Vec<SavedAccount>,
    /// `"light"` or `"dark"`; `None` follows the OS.
    #[serde(default)]
    pub theme: Option<String>,
    /// Last feed sort, as its query value.
    #[serde(default)]
    pub feed_sort: Option<String>,
    /// Sidebar shown as an icon rail.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// The Discuit instance to connect to (base URL); `None` = discuit.org.
    #[serde(default)]
    pub instance: Option<String>,
    /// `"home"` or `"all"`: what opens on launch. `None` = Home when logged in.
    #[serde(default)]
    pub startup_page: Option<String>,
    /// Open videos in the web browser instead of the in-app player.
    #[serde(default)]
    pub videos_in_browser: bool,
    /// Windows notifications for new Discuit notifications. Opt-in: off until
    /// the user turns them on in Settings.
    #[serde(default)]
    pub desktop_notifications: bool,
    /// Update server base URL; `None` = the build's default.
    #[serde(default)]
    pub update_server: Option<String>,
    /// `"stable"` or `"beta"`.
    #[serde(default)]
    pub update_channel: Option<String>,
    /// Check for updates automatically (default on).
    #[serde(default)]
    pub auto_update_disabled: bool,
    /// A version the user chose to skip.
    #[serde(default)]
    pub skipped_version: Option<String>,
}

/// A login kept for switching accounts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedAccount {
    /// The instance base URL, as used for `sessions`.
    pub instance: String,
    pub username: String,
    /// The session cookies for this login.
    pub cookies: String,
    /// Profile picture URL, for the account list.
    #[serde(default)]
    pub avatar: Option<String>,
}

impl Stored {
    /// Saved accounts on `instance`, in the order they were added.
    pub fn accounts_on(&self, instance: &str) -> Vec<SavedAccount> {
        self.accounts.iter().filter(|a| a.instance == instance).cloned().collect()
    }

    /// Add or refresh a saved account (matched by instance and username, ignoring case).
    pub fn remember_account(&mut self, account: SavedAccount) {
        match self.accounts.iter_mut().find(|a| a.instance == account.instance && a.username.eq_ignore_ascii_case(&account.username)) {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }

    /// Forget a saved account; returns it if it existed.
    pub fn forget_account(&mut self, instance: &str, username: &str) -> Option<SavedAccount> {
        let index = self.accounts.iter().position(|a| a.instance == instance && a.username.eq_ignore_ascii_case(username))?;
        Some(self.accounts.remove(index))
    }
}

/// The settings folder (`RISQUETTE_CONFIG_DIR` overrides it, for test or portable profiles).
pub fn dir() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("RISQUETTE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(custom));
    }
    let base = dirs::config_dir()?;
    Some(base.join(if cfg!(target_os = "linux") { "risquette" } else { "Risquette" }))
}

fn file() -> Option<PathBuf> {
    Some(dir()?.join("state.json"))
}

pub fn load() -> Stored {
    file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(state: &Stored) {
    let (Some(dir), Some(file)) = (dir(), file()) else { return };
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| {
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(state).unwrap_or_default())?;
        std::fs::rename(&tmp, &file)
    }) {
        log::warn!("could not save state: {e}");
    }
}

/// Load, modify, save.
pub fn update(f: impl FnOnce(&mut Stored)) {
    let mut s = load();
    f(&mut s);
    save(&s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(instance: &str, username: &str, cookies: &str) -> SavedAccount {
        SavedAccount { instance: instance.into(), username: username.into(), cookies: cookies.into(), avatar: None }
    }

    #[test]
    fn saved_accounts_are_kept_per_instance() {
        let mut s = Stored::default();
        s.remember_account(account("https://discuit.org/", "alice", "SID=1"));
        s.remember_account(account("https://discuit.org/", "bob", "SID=2"));
        s.remember_account(account("https://other.example/", "alice", "SID=3"));
        // Logging in again refreshes the entry instead of duplicating it.
        s.remember_account(account("https://discuit.org/", "Alice", "SID=4"));
        let names: Vec<_> = s.accounts_on("https://discuit.org/").into_iter().map(|a| (a.username, a.cookies)).collect();
        assert_eq!(names, [("Alice".to_string(), "SID=4".to_string()), ("bob".into(), "SID=2".into())]);
        assert_eq!(s.forget_account("https://discuit.org/", "BOB").map(|a| a.cookies).as_deref(), Some("SID=2"));
        assert!(s.forget_account("https://discuit.org/", "bob").is_none());
        assert_eq!(s.accounts_on("https://other.example/").len(), 1);
    }

    #[test]
    fn old_state_files_still_load() {
        let s: Stored = serde_json::from_str(r#"{"sessions":{"https://discuit.org/":"SID=1"},"theme":"dark"}"#).unwrap();
        assert!(s.accounts.is_empty());
        // Desktop notifications are opt-in.
        assert!(!s.desktop_notifications);
    }
}
