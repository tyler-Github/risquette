//! App-wide state stored as GPUI globals, and the background API helper.

use discuit_api::Client;
use gpui::{App, Context, Global, Image, ImageFormat, Task};
use std::{collections::HashMap, sync::Arc};

/// The shared API client.
pub struct AppState {
    pub client: Arc<Client>,
}

impl Global for AppState {}

impl AppState {
    pub fn client(cx: &App) -> Arc<Client> {
        cx.global::<AppState>().client.clone()
    }
}

/// Normalise an instance address typed by the user: add `https://`, drop paths.
pub fn normalize_instance(input: &str) -> Option<String> {
    let input = input.trim().trim_end_matches('/');
    if input.is_empty() {
        return None;
    }
    let with_scheme = if input.contains("://") { input.to_string() } else { format!("https://{input}") };
    let url = url::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    let mut base = format!("{}://{}", url.scheme(), url.host_str()?);
    if let Some(port) = url.port() {
        base.push_str(&format!(":{port}"));
    }
    Some(base)
}

/// Check that `base` is a Discuit instance (GET `/api/_initial`), returning its user count.
pub fn probe_instance(base: &str) -> Result<i64, String> {
    let client = Client::new(base).map_err(|e| e.user_message())?;
    let initial = client.initial().map_err(|e| format!("Not a Discuit instance, or unreachable: {}", e.user_message()))?;
    Ok(initial.no_users)
}

/// Switch the whole app to another instance: new client, fresh caches,
/// session reloaded with that instance's saved login (if any).
pub fn switch_instance(base: String, cx: &mut App) {
    let client = match Client::new(&base) {
        Ok(c) => c,
        Err(e) => {
            crate::nav::toast(e.user_message(), cx);
            return;
        }
    };
    let is_default = base == discuit_api::DEFAULT_BASE_URL;
    crate::storage::update(|s| s.instance = if is_default { None } else { Some(base.clone()) });
    cx.set_global(AppState { client: Arc::new(client) });
    cx.set_global(ImageCache::default());
    crate::session::Session::entity(cx).update(cx, |s, cx| s.reload_for_instance(cx));
    crate::nav::go(crate::nav::Target::Home, cx);
    crate::nav::toast(format!("Connected to {}", base.trim_start_matches("https://")), cx);
}

/// Run a blocking API call on the background executor, then `done` on the main thread.
pub fn api<T: Send + 'static>(
    cx: &mut App,
    call: impl FnOnce(&Client) -> discuit_api::Result<T> + Send + 'static,
    done: impl FnOnce(discuit_api::Result<T>, &mut App) + 'static,
) {
    let client = AppState::client(cx);
    let task = cx.background_executor().spawn(async move { call(&client) });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| done(result, cx)).ok();
    })
    .detach();
}

/// Like [`api`], but `done` gets the view; dropped silently if the view is gone.
/// The returned task should be stored (or detached) by the caller.
pub fn api_view<V: 'static, T: Send + 'static>(
    cx: &mut Context<V>,
    call: impl FnOnce(&Client) -> discuit_api::Result<T> + Send + 'static,
    done: impl FnOnce(&mut V, discuit_api::Result<T>, &mut Context<V>) + 'static,
) -> Task<()> {
    let client = AppState::client(cx);
    let task = cx.background_executor().spawn(async move { call(&client) });
    cx.spawn(async move |this, cx| {
        let result = task.await;
        this.update(cx, |view, cx| {
            done(view, result, cx);
            cx.notify();
        })
        .ok();
    })
}

/// A background API call as a task resolving to a user-facing error message.
pub fn api_task<T: Send + 'static>(
    cx: &App,
    call: impl FnOnce(&Client) -> discuit_api::Result<T> + Send + 'static,
) -> Task<Result<T, String>> {
    let client = AppState::client(cx);
    cx.background_executor().spawn(async move { call(&client).map_err(|e| e.user_message()) })
}

enum Slot {
    Loading,
    Ready(Arc<Image>),
    Failed,
}

/// In-memory cache of downloaded images, keyed by server URL.
#[derive(Default)]
pub struct ImageCache {
    slots: HashMap<String, Slot>,
}

impl Global for ImageCache {}

impl ImageCache {
    /// The image if downloaded; otherwise starts a download (once) and re-renders when it lands.
    pub fn get(url: &str, cx: &mut App) -> Option<Arc<Image>> {
        let cache = cx.default_global::<ImageCache>();
        match cache.slots.get(url) {
            Some(Slot::Ready(img)) => return Some(img.clone()),
            Some(Slot::Loading | Slot::Failed) => return None,
            None => {}
        }
        cache.slots.insert(url.to_string(), Slot::Loading);
        let key = url.to_string();
        let fetch_key = key.clone();
        api(
            cx,
            move |c| c.fetch_bytes(&fetch_key),
            move |result, cx| {
                let slot = match result.map_err(|e| e.to_string()).and_then(|b| image_from_bytes(b).ok_or("unknown image format".into())) {
                    Ok(img) => Slot::Ready(img),
                    Err(err) => {
                        log::warn!("image: {err}");
                        Slot::Failed
                    }
                };
                cx.default_global::<ImageCache>().slots.insert(key, slot);
                cx.refresh_windows();
            },
        );
        None
    }
}

/// Wrap raw bytes as a GPUI image, sniffing the format.
pub fn image_from_bytes(bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = sniff_format(&bytes)?;
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

fn sniff_format(b: &[u8]) -> Option<ImageFormat> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if b.starts_with(b"\x89PNG") {
        Some(ImageFormat::Png)
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if b.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_instance;

    #[test]
    fn instance_addresses() {
        assert_eq!(normalize_instance("discuit.org").as_deref(), Some("https://discuit.org"));
        assert_eq!(normalize_instance(" https://discuit.example.com/some/path/ ").as_deref(), Some("https://discuit.example.com"));
        assert_eq!(normalize_instance("http://localhost:8080").as_deref(), Some("http://localhost:8080"));
        assert_eq!(normalize_instance("ftp://x.org"), None);
        assert_eq!(normalize_instance("   "), None);
    }
}
