//! The user's own arrangements, kept in settings: favourite communities (in
//! their chosen order), custom feeds made of several communities, and keyword
//! filters that hide posts from feeds. Favourites and custom feeds are per
//! instance, since community ids and names differ between servers.

use crate::{
    state::AppState,
    storage::{self, CustomFeed, FeedCommunity},
};
use discuit_api::Post;
use gpui::App;

/// The settings key for the current instance.
fn instance(cx: &App) -> String {
    AppState::client(cx).base_url().as_str().to_string()
}

/// Change the settings, then redraw so the sidebar and feeds pick it up.
fn update(cx: &mut App, f: impl FnOnce(&mut storage::Stored)) {
    storage::update(f);
    cx.refresh_windows();
}

// ---------------------------------------------------------------------------
// Favourite communities
// ---------------------------------------------------------------------------

/// Favourite community names, in the order shown.
pub fn favourites(cx: &App) -> Vec<String> {
    storage::load().favourite_communities.get(&instance(cx)).cloned().unwrap_or_default()
}

pub fn is_favourite(name: &str, cx: &App) -> bool {
    favourites(cx).iter().any(|f| f.eq_ignore_ascii_case(name))
}

/// Add a community to the end of the favourites, or take it out.
pub fn toggle_favourite(name: &str, cx: &mut App) {
    let key = instance(cx);
    update(cx, |s| {
        let list = s.favourite_communities.entry(key).or_default();
        match list.iter().position(|f| f.eq_ignore_ascii_case(name)) {
            Some(ix) => {
                list.remove(ix);
            }
            None => list.push(name.to_string()),
        }
    });
}

/// Move the favourite at `from` to position `to`.
pub fn move_favourite(from: usize, to: usize, cx: &mut App) {
    let key = instance(cx);
    update(cx, |s| move_item(s.favourite_communities.entry(key).or_default(), from, to));
}

/// Move `list[from]` so it ends up at index `to` (clamped); out-of-range `from` does nothing.
pub fn move_item<T>(list: &mut Vec<T>, from: usize, to: usize) {
    if from >= list.len() {
        return;
    }
    let item = list.remove(from);
    list.insert(to.min(list.len()), item);
}

// ---------------------------------------------------------------------------
// Custom feeds
// ---------------------------------------------------------------------------

pub fn custom_feeds(cx: &App) -> Vec<CustomFeed> {
    storage::load().custom_feeds.get(&instance(cx)).cloned().unwrap_or_default()
}

pub fn custom_feed(id: u64, cx: &App) -> Option<CustomFeed> {
    custom_feeds(cx).into_iter().find(|f| f.id == id)
}

/// Create a feed and return its id.
pub fn create_custom_feed(name: &str, communities: Vec<FeedCommunity>, cx: &mut App) -> u64 {
    let key = instance(cx);
    let mut id = 0;
    update(cx, |s| {
        let feeds = s.custom_feeds.entry(key).or_default();
        id = next_id(feeds);
        feeds.push(CustomFeed { id, name: name.trim().to_string(), communities });
    });
    id
}

/// One more than the highest id in use.
pub fn next_id(feeds: &[CustomFeed]) -> u64 {
    feeds.iter().map(|f| f.id).max().map_or(1, |m| m + 1)
}

fn edit_feed(id: u64, cx: &mut App, f: impl FnOnce(&mut CustomFeed)) {
    let key = instance(cx);
    update(cx, |s| {
        if let Some(feed) = s.custom_feeds.entry(key).or_default().iter_mut().find(|f| f.id == id) {
            f(feed);
        }
    });
}

pub fn rename_custom_feed(id: u64, name: &str, cx: &mut App) {
    let name = name.trim().to_string();
    edit_feed(id, cx, |f| f.name = name);
}

pub fn delete_custom_feed(id: u64, cx: &mut App) {
    let key = instance(cx);
    update(cx, |s| s.custom_feeds.entry(key).or_default().retain(|f| f.id != id));
}

/// Whether a feed includes a community.
pub fn feed_has(feed: &CustomFeed, community_id: &str) -> bool {
    feed.communities.iter().any(|c| c.id == community_id)
}

/// Add a community to a feed, or take it out.
pub fn toggle_feed_community(id: u64, community: FeedCommunity, cx: &mut App) {
    edit_feed(id, cx, |f| match f.communities.iter().position(|c| c.id == community.id) {
        Some(ix) => {
            f.communities.remove(ix);
        }
        None => f.communities.push(community),
    });
}

// ---------------------------------------------------------------------------
// Keyword filters
// ---------------------------------------------------------------------------

pub fn keyword_filters() -> Vec<String> {
    storage::load().keyword_filters
}

/// Add a filter (ignoring blanks and duplicates). Returns whether it was added.
pub fn add_keyword(keyword: &str, cx: &mut App) -> bool {
    let keyword = keyword.trim().to_string();
    if keyword.is_empty() || keyword_filters().iter().any(|k| k.eq_ignore_ascii_case(&keyword)) {
        return false;
    }
    update(cx, |s| s.keyword_filters.push(keyword));
    true
}

pub fn remove_keyword(keyword: &str, cx: &mut App) {
    update(cx, |s| s.keyword_filters.retain(|k| k != keyword));
}

/// Whether `text` contains `keyword` as whole words, ignoring case: "art"
/// matches "Art!" and "modern art" but not "start". A keyword can be a phrase.
pub fn keyword_matches(text: &str, keyword: &str) -> bool {
    let keyword = keyword.trim().to_lowercase();
    if keyword.is_empty() {
        return false;
    }
    let text = text.to_lowercase();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(at) = text[from..].find(&keyword) {
        let start = from + at;
        let end = start + keyword.len();
        let before_ok = text[..start].chars().next_back().is_none_or(|c| !is_word(c));
        let after_ok = text[end..].chars().next().is_none_or(|c| !is_word(c));
        if before_ok && after_ok {
            return true;
        }
        // Step past this match's first character and look again.
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Whether a post's title or body matches any of `keywords`.
pub fn post_filtered(post: &Post, keywords: &[String]) -> bool {
    keywords.iter().any(|k| keyword_matches(&post.title, k) || post.body.as_deref().is_some_and(|b| keyword_matches(b, k)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_match_whole_words_in_any_case() {
        assert!(keyword_matches("Modern ART is fun", "art"));
        assert!(keyword_matches("art!", "Art"));
        assert!(!keyword_matches("Let's start", "art"));
        assert!(!keyword_matches("artistic", "art"));
        // Phrases, and a later whole-word match after an earlier partial one.
        assert!(keyword_matches("The World Cup final", "world cup"));
        assert!(keyword_matches("start the art show", "art"));
        // Non-ASCII text doesn't trip over character boundaries.
        assert!(keyword_matches("Ça va, café?", "café"));
        assert!(!keyword_matches("cafés", "café"));
        assert!(!keyword_matches("anything", "  "));
    }

    #[test]
    fn posts_are_filtered_by_title_or_body() {
        let mut p: Post =
            serde_json::from_value(serde_json::json!({ "id": "1", "publicId": "1", "title": "Election results", "createdAt": "2026-01-01T00:00:00Z" }))
                .unwrap();
        assert!(post_filtered(&p, &["election".into()]));
        assert!(!post_filtered(&p, &["cats".into()]));
        p.body = Some("Look at these cats".into());
        assert!(post_filtered(&p, &["cats".into()]));
        assert!(!post_filtered(&p, &[]));
    }

    #[test]
    fn moving_and_numbering() {
        let mut v = vec!["a", "b", "c", "d"];
        move_item(&mut v, 0, 2);
        assert_eq!(v, ["b", "c", "a", "d"]);
        move_item(&mut v, 3, 0);
        assert_eq!(v, ["d", "b", "c", "a"]);
        move_item(&mut v, 1, 99);
        assert_eq!(v, ["d", "c", "a", "b"]);
        move_item(&mut v, 9, 0);
        assert_eq!(v, ["d", "c", "a", "b"]);
        assert_eq!(next_id(&[]), 1);
        let feeds = [CustomFeed { id: 4, ..Default::default() }, CustomFeed { id: 2, ..Default::default() }];
        assert_eq!(next_id(&feeds), 5);
    }
}
