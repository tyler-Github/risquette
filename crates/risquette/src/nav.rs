//! App-wide navigation, modals, context menus and toasts, routed to the root view.

use crate::{icons::Icon, root::Root};
use gpui::{AnyView, App, Global, Pixels, Point, SharedString, WeakEntity, Window};
use std::rc::Rc;

/// Somewhere the app can go.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Home,
    All,
    Post { public_id: String },
    Community(String),
    User(String),
    NewPost { community: Option<String> },
    Notifications,
    Settings,
    Lists(String),
    List { id: i64, title: String },
    Communities,
    ModTools(String),
    Admin,
}

/// A page to open once the session has loaded (from the command line).
pub struct StartupTarget(pub Option<Target>);

impl Global for StartupTarget {}

pub struct RootHandle(pub WeakEntity<Root>);

impl Global for RootHandle {}

fn with_root(cx: &mut App, f: impl FnOnce(&mut Root, &mut gpui::Context<Root>) + 'static) {
    let Some(root) = cx.try_global::<RootHandle>().and_then(|h| h.0.upgrade()) else { return };
    // Deferred so callers can be inside another entity's update (or Root's own).
    cx.defer(move |cx| {
        root.update(cx, f);
    });
}

pub fn go(target: Target, cx: &mut App) {
    with_root(cx, move |root, cx| root.navigate(target, cx));
}

pub fn open_modal(view: impl Into<AnyView>, cx: &mut App) {
    let view = view.into();
    with_root(cx, move |root, cx| root.set_modal(Some(view), cx));
}

pub fn close_modal(cx: &mut App) {
    with_root(cx, |root, cx| root.set_modal(None, cx));
}

pub fn toast(message: impl Into<SharedString>, cx: &mut App) {
    let message = message.into();
    with_root(cx, move |root, cx| root.show_toast(message, cx));
}

/// What a menu entry does when chosen.
pub type MenuAction = Rc<dyn Fn(&mut Window, &mut App)>;

/// One entry of a context menu.
#[derive(Clone)]
pub struct MenuItem {
    pub label: SharedString,
    pub icon: Option<Icon>,
    pub danger: bool,
    pub action: MenuAction,
}

impl MenuItem {
    pub fn new(label: impl Into<SharedString>, action: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        let label: SharedString = label.into();
        Self { icon: guess_icon(&label), label, danger: false, action: Rc::new(action) }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// Pick an icon from a menu label so every menu gets consistent icons.
fn guess_icon(label: &str) -> Option<Icon> {
    let l = label.to_lowercase();
    let starts = |p: &str| l.starts_with(p);
    Some(if starts("save") {
        Icon::Bookmark
    } else if starts("copy link") {
        Icon::Link
    } else if starts("copy") {
        Icon::Copy
    } else if starts("open in browser") {
        Icon::ExternalLink
    } else if starts("go to") {
        Icon::Compass
    } else if starts("view @") || starts("profile") {
        Icon::User
    } else if starts("public lists") {
        Icon::List
    } else if starts("edit") {
        Icon::Pencil
    } else if starts("delete") || starts("remove") || starts("clear") {
        Icon::Trash
    } else if starts("hide") {
        Icon::EyeOff
    } else if starts("report") {
        Icon::Flag
    } else if starts("mute") || starts("unmute") {
        Icon::VolumeX
    } else if starts("unlock") {
        Icon::LockOpen
    } else if starts("lock") {
        Icon::Lock
    } else if starts("pin") || starts("unpin") {
        Icon::Pin
    } else if starts("announce") || l.contains("badge") {
        Icon::Sparkles
    } else if starts("show as") || starts("ban") || starts("unban") {
        Icon::Shield
    } else if starts("mark as") {
        Icon::MailCheck
    } else if l.contains("default") {
        Icon::LayoutGrid
    } else {
        return None;
    })
}

pub fn open_menu(position: Point<Pixels>, items: Vec<MenuItem>, cx: &mut App) {
    if items.is_empty() {
        return;
    }
    with_root(cx, move |root, cx| root.set_menu(Some((position, items)), cx));
}

/// Open a link: Discuit links navigate in-app, everything else goes to the browser.
pub fn open_link(url: &str, cx: &mut App) {
    let client = crate::state::AppState::client(cx);
    let base = client.base_url().clone();
    let path = if url.starts_with('/') {
        Some(url.to_string())
    } else {
        url::Url::parse(url).ok().filter(|u| u.host_str() == base.host_str()).map(|u| u.path().to_string())
    };
    match path.and_then(|p| target_for_path(&p)) {
        Some(t) => go(t, cx),
        None => {
            let absolute = client.absolute_url(url).map(|u| u.to_string()).unwrap_or_else(|_| url.to_string());
            cx.open_url(&absolute);
        }
    }
}

/// Map a Discuit web path to an in-app target.
pub fn target_for_path(path: &str) -> Option<Target> {
    let parts: Vec<&str> = path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [] => Some(Target::Home),
        [u] if u.starts_with('@') => Some(Target::User(u[1..].to_string())),
        [u, "lists"] if u.starts_with('@') => Some(Target::Lists(u[1..].to_string())),
        ["notifications"] => Some(Target::Notifications),
        ["settings"] => Some(Target::Settings),
        ["communities"] => Some(Target::Communities),
        ["new"] => Some(Target::NewPost { community: None }),
        ["admin", ..] => Some(Target::Admin),
        ["about" | "terms" | "privacy-policy" | "guidelines" | "images" | "api", ..] => None,
        [c] => Some(Target::Community(c.to_string())),
        [_c, "post", id, ..] => Some(Target::Post { public_id: id.to_string() }),
        [c, "modtools", ..] => Some(Target::ModTools(c.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(target_for_path("/@alice"), Some(Target::User("alice".into())));
        assert_eq!(target_for_path("/golang"), Some(Target::Community("golang".into())));
        assert_eq!(target_for_path("/golang/post/k3J9dQ"), Some(Target::Post { public_id: "k3J9dQ".into() }));
        assert_eq!(target_for_path("/golang/post/k3J9dQ/5c3e2a"), Some(Target::Post { public_id: "k3J9dQ".into() }));
        assert_eq!(target_for_path("/notifications"), Some(Target::Notifications));
        assert_eq!(target_for_path("/about"), None);
        assert_eq!(target_for_path("/"), Some(Target::Home));
    }
}
