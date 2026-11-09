//! Unsent comments, replies and posts, kept on disk so nothing typed is lost
//! by navigating away, closing the window, a crash or a send that failed.
//!
//! Drafts live in memory and are written to `drafts.json` beside `state.json`
//! at most once a second (and when the app quits). Each is keyed by instance
//! and what it's for (see [`key`]), removed once it's sent or emptied, and
//! dropped after [`MAX_AGE_DAYS`] days.

use crate::state::AppState;
use gpui::App;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// Drafts untouched for this long are forgotten.
pub const MAX_AGE_DAYS: i64 = 30;

/// One draft: named text fields (`body`, or `title`/`body`/`url`/… for a post).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Unix seconds when it was last changed.
    #[serde(default)]
    pub saved: i64,
}

impl Draft {
    pub fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }

    /// Nothing worth keeping: every field is blank.
    fn is_blank(&self) -> bool {
        self.fields.values().all(|v| v.trim().is_empty())
    }
}

struct Store {
    drafts: BTreeMap<String, Draft>,
    dirty: bool,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn file() -> Option<PathBuf> {
    Some(crate::storage::dir()?.join("drafts.json"))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Drop drafts that are blank or older than [`MAX_AGE_DAYS`].
fn prune(drafts: &mut BTreeMap<String, Draft>, now: i64) {
    drafts.retain(|_, d| !d.is_blank() && now - d.saved < MAX_AGE_DAYS * 24 * 60 * 60);
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let store = guard.get_or_insert_with(|| {
        let mut drafts: BTreeMap<String, Draft> = file().and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        prune(&mut drafts, now());
        Store { drafts, dirty: false }
    });
    f(store)
}

/// The key for a draft of `kind` (`comment`, `reply`, `post`) about `id`, on the current instance.
pub fn key(kind: &str, id: &str, cx: &App) -> String {
    format!("{}|{kind}|{id}", AppState::client(cx).base_url().as_str().trim_end_matches('/'))
}

pub fn get(key: &str) -> Option<Draft> {
    with_store(|s| s.drafts.get(key).cloned())
}

/// Save a draft's fields; blank fields remove it.
pub fn set(key: &str, fields: BTreeMap<String, String>) {
    let draft = Draft { fields, saved: now() };
    with_store(|s| {
        if draft.is_blank() {
            if s.drafts.remove(key).is_some() {
                s.dirty = true;
            }
        } else if s.drafts.get(key).map(|d| &d.fields) != Some(&draft.fields) {
            s.drafts.insert(key.to_string(), draft);
            s.dirty = true;
        }
    });
    start_writer();
}

/// A draft with just a `body` (comments and replies).
pub fn set_body(key: &str, body: &str) {
    set(key, BTreeMap::from([("body".to_string(), body.to_string())]));
}

pub fn remove(key: &str) {
    with_store(|s| {
        if s.drafts.remove(key).is_some() {
            s.dirty = true;
        }
    });
    start_writer();
}

/// Write changes to disk now (also done every second in the background).
pub fn flush() {
    let snapshot = with_store(|s| std::mem::take(&mut s.dirty).then(|| s.drafts.clone()));
    let (Some(drafts), Some(file)) = (snapshot, file()) else { return };
    let result = std::fs::create_dir_all(file.parent().unwrap_or(&file)).and_then(|_| {
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&drafts).unwrap_or_default())?;
        std::fs::rename(&tmp, &file)
    });
    if let Err(e) = result {
        log::warn!("could not save drafts: {e}");
    }
}

/// A background thread that writes changed drafts once a second.
fn start_writer() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("drafts".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    flush();
                }
            })
            .map(|_| ())
            .unwrap_or_else(|e| log::warn!("drafts writer: {e}"));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(body: &str, saved: i64) -> Draft {
        Draft { fields: BTreeMap::from([("body".to_string(), body.to_string())]), saved }
    }

    #[test]
    fn blank_and_old_drafts_are_dropped() {
        let now = 1_000_000_000;
        let day = 24 * 60 * 60;
        let mut drafts = BTreeMap::from([
            ("fresh".to_string(), draft("hello", now - day)),
            ("blank".to_string(), draft("  \n ", now)),
            ("old".to_string(), draft("forgotten", now - (MAX_AGE_DAYS + 1) * day)),
        ]);
        prune(&mut drafts, now);
        assert_eq!(drafts.keys().collect::<Vec<_>>(), ["fresh"]);
    }

    #[test]
    fn drafts_round_trip_through_json() {
        let d = Draft { fields: BTreeMap::from([("title".into(), "Hi".into()), ("body".into(), "**there**".into())]), saved: 42 };
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Draft>(&json).unwrap(), d);
        assert_eq!(d.field("title"), "Hi");
        assert_eq!(d.field("url"), "");
        // Older or hand-edited files with missing fields still load.
        assert_eq!(serde_json::from_str::<Draft>("{}").unwrap(), Draft::default());
    }
}
