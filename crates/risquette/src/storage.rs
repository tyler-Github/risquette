//! Small on-disk state: the session cookie and UI preferences.
//!
//! Stored as JSON in the platform config directory
//! (`%APPDATA%\Risquette` on Windows, `~/.config/risquette` on Linux,
//! `~/Library/Application Support/Risquette` on macOS).

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stored {
    /// Cookies per instance base URL.
    #[serde(default)]
    pub sessions: BTreeMap<String, String>,
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

fn dir() -> Option<PathBuf> {
    // `RISQUETTE_CONFIG_DIR` keeps a separate profile (testing, portable use).
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
