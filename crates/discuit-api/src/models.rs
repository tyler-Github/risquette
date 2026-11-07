//! Response types. Fields are lenient (`#[serde(default)]`, `Option`) because
//! the live API sometimes differs from the documented shape — for example
//! `averageColor` is a CSS string like `"rgb(85,84,84)"`, not an object.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Timestamp = DateTime<Utc>;

/// Post titles as shown: trimmed, with line breaks collapsed to spaces
/// (some titles arrive as `"\nTitle "`, which the web client also flattens).
pub(crate) fn de_title<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let raw = Option::<String>::deserialize(d)?.unwrap_or_default();
    Ok(raw.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Pagination cursors arrive as strings, or as (64-bit) integers for some
/// sorts (e.g. `activity`). Normalise them to strings without losing precision.
pub(crate) fn de_cursor<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string, integer or null cursor")
        }
        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_any(V)
        }
        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
            Ok((!v.is_empty()).then(|| v.to_string()))
        }
        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
            Ok(Some(format!("{v:.0}")))
        }
    }
    d.deserialize_option(V)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PostType {
    #[default]
    Text,
    Image,
    Link,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FeedSort {
    #[default]
    Hot,
    Latest,
    Activity,
    TopDay,
    TopWeek,
    TopMonth,
    TopYear,
    TopAll,
}

impl FeedSort {
    pub const ALL: [FeedSort; 8] =
        [FeedSort::Hot, FeedSort::Latest, FeedSort::Activity, FeedSort::TopDay, FeedSort::TopWeek, FeedSort::TopMonth, FeedSort::TopYear, FeedSort::TopAll];

    /// Sorts that work for community feeds. Discuit currently answers
    /// `500` for day/week/month/year on `feed=community`.
    pub const COMMUNITY: [FeedSort; 4] = [FeedSort::Hot, FeedSort::Latest, FeedSort::Activity, FeedSort::TopAll];

    pub fn works_for_community(self) -> bool {
        Self::COMMUNITY.contains(&self)
    }

    pub fn as_query(self) -> &'static str {
        match self {
            FeedSort::Hot => "hot",
            FeedSort::Latest => "latest",
            FeedSort::Activity => "activity",
            FeedSort::TopDay => "day",
            FeedSort::TopWeek => "week",
            FeedSort::TopMonth => "month",
            FeedSort::TopYear => "year",
            FeedSort::TopAll => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FeedSort::Hot => "Hot",
            FeedSort::Latest => "New",
            FeedSort::Activity => "Activity",
            FeedSort::TopDay => "Top: Day",
            FeedSort::TopWeek => "Top: Week",
            FeedSort::TopMonth => "Top: Month",
            FeedSort::TopYear => "Top: Year",
            FeedSort::TopAll => "Top: All time",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FeedType {
    #[default]
    All,
    Home,
    Community,
    Moderating,
}

impl FeedType {
    pub fn as_query(self) -> &'static str {
        match self {
            FeedType::All => "all",
            FeedType::Home => "home",
            FeedType::Community => "community",
            FeedType::Moderating => "moderating",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageCopy {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub box_width: u32,
    #[serde(default)]
    pub box_height: u32,
    #[serde(default)]
    pub object_fit: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub mimetype: Option<String>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub size: u64,
    /// Kept as raw JSON: the live API sends `"rgb(r,g,b)"`, the docs show `{r,g,b}`.
    #[serde(default)]
    pub average_color: Option<serde_json::Value>,
    pub url: String,
    #[serde(default)]
    pub copies: Vec<ImageCopy>,
    #[serde(default)]
    pub alt_text: Option<String>,
}

impl Image {
    /// The average colour as `(r, g, b)`, accepting both shapes.
    pub fn average_rgb(&self) -> Option<(u8, u8, u8)> {
        match self.average_color.as_ref()? {
            serde_json::Value::String(s) => {
                let inner = s.trim().strip_prefix("rgb(")?.strip_suffix(')')?;
                let mut it = inner.split(',').map(|p| p.trim().parse::<u8>().ok());
                Some((it.next()??, it.next()??, it.next()??))
            }
            serde_json::Value::Object(o) => {
                let get = |k: &str| o.get(k)?.as_u64().map(|v| v.min(255) as u8);
                Some((get("r")?, get("g")?, get("b")?))
            }
            _ => None,
        }
    }

    /// The smallest copy at least `min_width` wide, falling back to the
    /// largest copy, then the original.
    pub fn best_url(&self, min_width: u32) -> &str {
        let mut copies: Vec<&ImageCopy> = self.copies.iter().collect();
        copies.sort_by_key(|c| c.width);
        copies.iter().find(|c| c.width >= min_width).or(copies.last()).map(|c| c.url.as_str()).unwrap_or(&self.url)
    }

    pub fn copy(&self, name: &str) -> Option<&ImageCopy> {
        self.copies.iter().find(|c| c.name == name)
    }
}

/// One image of an image post. The live API sends a flat image object
/// (optionally with a `caption`); a nested `{image, caption}` shape is also accepted.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PostImage {
    pub image: Option<Image>,
    pub caption: Option<String>,
}

impl<'de> Deserialize<'de> for PostImage {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Flat {
            #[serde(flatten)]
            image: Image,
            #[serde(default)]
            caption: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Nested {
                image: Image,
                #[serde(default)]
                caption: Option<String>,
            },
            Flat(Flat),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Nested { image, caption } => PostImage { image: Some(image), caption },
            Raw::Flat(f) => PostImage { image: Some(f.image), caption: f.caption },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub url: String,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub image: Option<Image>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub about_me: Option<String>,
    #[serde(default)]
    pub points: i64,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub pro_pic: Option<Image>,
    #[serde(default)]
    pub badges: Option<Vec<Badge>>,
    #[serde(default)]
    pub no_posts: i64,
    #[serde(default)]
    pub no_comments: i64,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub is_banned: bool,
    #[serde(default)]
    pub notifications_new_count: i64,
    #[serde(default)]
    pub home_feed: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_confirmed_at: Option<Timestamp>,
    #[serde(default)]
    pub upvote_notifications_off: bool,
    #[serde(default)]
    pub reply_notifications_off: bool,
    #[serde(default)]
    pub remember_feed_sort: bool,
    #[serde(default)]
    pub require_alt_text: bool,
    #[serde(default)]
    pub last_seen_month: Option<String>,
    #[serde(default)]
    pub banned_at: Option<Timestamp>,
    #[serde(default)]
    pub embeds_off: bool,
    #[serde(default)]
    pub hide_user_profile_pictures: bool,
    #[serde(default)]
    pub modding_list: Option<Vec<Community>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommunityRule {
    #[serde(default)]
    pub id: i64,
    pub rule: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub z_index: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Community {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub about: Option<String>,
    #[serde(default)]
    pub no_members: i64,
    #[serde(default)]
    pub pro_pic: Option<Image>,
    #[serde(default)]
    pub banner_image: Option<Image>,
    #[serde(default)]
    pub posting_restricted: bool,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub user_joined: Option<bool>,
    #[serde(default)]
    pub user_mod: Option<bool>,
    #[serde(default)]
    pub is_muted: bool,
    #[serde(default)]
    pub mods: Option<Vec<User>>,
    #[serde(default)]
    pub rules: Option<Vec<CommunityRule>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: PostType,
    pub public_id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub user_group: Option<String>,
    #[serde(default)]
    pub user_deleted: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub is_pinned_site: bool,
    #[serde(default)]
    pub community_id: String,
    #[serde(default)]
    pub community_name: String,
    #[serde(default)]
    pub community_pro_pic: Option<Image>,
    #[serde(default, deserialize_with = "de_title")]
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub image: Option<Image>,
    #[serde(default)]
    pub images: Option<Vec<PostImage>>,
    #[serde(default)]
    pub link: Option<Link>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub locked_by: Option<String>,
    #[serde(default)]
    pub deleted_as: Option<String>,
    #[serde(default)]
    pub upvotes: i64,
    #[serde(default)]
    pub downvotes: i64,
    pub created_at: Timestamp,
    #[serde(default)]
    pub edited_at: Option<Timestamp>,
    #[serde(default)]
    pub last_activity_at: Option<Timestamp>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub deleted_content: bool,
    #[serde(default)]
    pub no_comments: i64,
    #[serde(default)]
    pub comments: Option<Vec<Comment>>,
    #[serde(default, deserialize_with = "de_cursor")]
    pub comments_next: Option<String>,
    #[serde(default)]
    pub user_voted: Option<bool>,
    #[serde(default)]
    pub user_voted_up: Option<bool>,
    #[serde(default)]
    pub new_comments: i64,
    #[serde(default)]
    pub is_author_muted: bool,
    #[serde(default)]
    pub is_community_muted: bool,
    #[serde(default)]
    pub author: Option<User>,
    /// Only present on `GET /api/posts/{id}` (unless `fetchCommunity=false`).
    #[serde(default)]
    pub community: Option<Community>,
}

impl Post {
    pub fn points(&self) -> i64 {
        self.upvotes - self.downvotes
    }

    /// The first image of an image post (newer posts use `images`, older ones `image`).
    pub fn primary_image(&self) -> Option<&Image> {
        self.images.as_ref().and_then(|imgs| imgs.iter().find_map(|i| i.image.as_ref())).or(self.image.as_ref())
    }

    /// The viewer's vote: `Some(true)` up, `Some(false)` down, `None` none / anonymous.
    pub fn vote(&self) -> Option<bool> {
        if self.user_voted == Some(true) { self.user_voted_up } else { None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    #[serde(default)]
    pub post_id: String,
    #[serde(default)]
    pub post_public_id: Option<String>,
    #[serde(default)]
    pub community_name: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub user_group: Option<String>,
    #[serde(default)]
    pub user_deleted: bool,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub no_replies: i64,
    #[serde(default)]
    pub no_replies_direct: i64,
    #[serde(default)]
    pub ancestors: Option<Vec<String>>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub upvotes: i64,
    #[serde(default)]
    pub downvotes: i64,
    pub created_at: Timestamp,
    #[serde(default)]
    pub edited_at: Option<Timestamp>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub locked_by: Option<String>,
    #[serde(default)]
    pub deleted_as: Option<String>,
    #[serde(default)]
    pub user_voted: Option<bool>,
    #[serde(default)]
    pub user_voted_up: Option<bool>,
    #[serde(default)]
    pub post_deleted: bool,
    #[serde(default)]
    pub author: Option<User>,
}

impl Comment {
    pub fn points(&self) -> i64 {
        self.upvotes - self.downvotes
    }

    pub fn vote(&self) -> Option<bool> {
        if self.user_voted == Some(true) { self.user_voted_up } else { None }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeedPage {
    #[serde(default)]
    pub posts: Option<Vec<Post>>,
    #[serde(default, deserialize_with = "de_cursor")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommentsPage {
    #[serde(default)]
    pub comments: Option<Vec<Comment>>,
    #[serde(default, deserialize_with = "de_cursor")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReportReason {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Initial {
    #[serde(default)]
    pub signups_disabled: bool,
    #[serde(default)]
    pub report_reasons: Option<Vec<ReportReason>>,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub communities: Option<Vec<Community>>,
    #[serde(default)]
    pub no_users: i64,
    #[serde(default)]
    pub banned_from: Option<Vec<String>>,
    #[serde(default)]
    pub lists: Option<Vec<List>>,
    #[serde(default)]
    pub mutes: Option<Mutes>,
    #[serde(default)]
    pub vapid_public_key: Option<String>,
    #[serde(default)]
    pub image_post_submit_req_points: i64,
    #[serde(default)]
    pub link_post_submit_req_points: i64,
}

// ---------------------------------------------------------------------------
// Enums used in requests
// ---------------------------------------------------------------------------

/// Who an action is performed "as" (`userGroup`, `deleteAs`, `lockAs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UserGroup {
    #[default]
    Normal,
    Mods,
    Admins,
}

impl UserGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            UserGroup::Normal => "normal",
            UserGroup::Mods => "mods",
            UserGroup::Admins => "admins",
        }
    }
}

/// `post` or `comment` (reports, lists, `_saved_to`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Post,
    Comment,
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            ContentType::Post => "post",
            ContentType::Comment => "comment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteKind {
    User,
    Community,
}

impl MuteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MuteKind::User => "user",
            MuteKind::Community => "community",
        }
    }
}

// ---------------------------------------------------------------------------
// Users & settings
// ---------------------------------------------------------------------------

/// Body of `POST /api/_settings?action=updateProfile`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUpdate {
    pub about_me: Option<String>,
    pub upvote_notifications_off: bool,
    pub reply_notifications_off: bool,
    pub home_feed: String,
    pub remember_feed_sort: bool,
    pub embeds_off: bool,
    pub hide_user_profile_pictures: bool,
    pub require_alt_text: bool,
}

impl ProfileUpdate {
    pub fn from_user(u: &User) -> Self {
        Self {
            about_me: u.about_me.clone(),
            upvote_notifications_off: u.upvote_notifications_off,
            reply_notifications_off: u.reply_notifications_off,
            home_feed: u.home_feed.clone().unwrap_or_else(|| "all".into()),
            remember_feed_sort: u.remember_feed_sort,
            embeds_off: u.embeds_off,
            hide_user_profile_pictures: u.hide_user_profile_pictures,
            require_alt_text: u.require_alt_text,
        }
    }
}

/// One entry of `GET /api/users/{username}/feed`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "item", rename_all = "lowercase")]
pub enum FeedItem {
    Post(Box<Post>),
    Comment(Box<Comment>),
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserFeedPage {
    #[serde(default)]
    pub items: Option<Vec<FeedItem>>,
    #[serde(default, deserialize_with = "de_cursor")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AdminUsersPage {
    #[serde(default)]
    pub users: Option<Vec<User>>,
    #[serde(default, deserialize_with = "de_cursor")]
    pub next: Option<String>,
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub id: i64,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub username: String,
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub num_items: i64,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub last_updated_at: Option<Timestamp>,
}

impl List {
    pub fn title(&self) -> &str {
        if self.display_name.is_empty() { &self.name } else { &self.display_name }
    }
}

/// Body of `POST /api/users/{username}/lists`.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NewList {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub public: bool,
}

/// Partial update for a list; `None` fields are left unchanged.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListItem {
    pub id: i64,
    #[serde(default)]
    pub list_id: i64,
    pub target_type: ContentType,
    pub target_id: String,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub target_item: Option<serde_json::Value>,
}

impl ListItem {
    pub fn post(&self) -> Option<Post> {
        (self.target_type == ContentType::Post).then(|| serde_json::from_value(self.target_item.clone()?).ok())?
    }

    pub fn comment(&self) -> Option<Comment> {
        (self.target_type == ContentType::Comment).then(|| serde_json::from_value(self.target_item.clone()?).ok())?
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListItemsPage {
    #[serde(default)]
    pub items: Option<Vec<ListItem>>,
    #[serde(default, deserialize_with = "de_cursor")]
    pub next: Option<String>,
}

// ---------------------------------------------------------------------------
// Mutes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Mute {
    /// `u_<n>` / `c_<n>`.
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub muted_user_id: Option<String>,
    #[serde(default)]
    pub muted_community_id: Option<String>,
    #[serde(default)]
    pub muted_user: Option<User>,
    #[serde(default)]
    pub muted_community: Option<Community>,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Mutes {
    #[serde(default)]
    pub community_mutes: Option<Vec<Mute>>,
    #[serde(default)]
    pub user_mutes: Option<Vec<Mute>>,
}

impl Mutes {
    pub fn is_user_muted(&self, user_id: &str) -> bool {
        self.user_mutes.iter().flatten().any(|m| m.muted_user_id.as_deref() == Some(user_id))
    }

    pub fn is_community_muted(&self, community_id: &str) -> bool {
        self.community_mutes.iter().flatten().any(|m| m.muted_community_id.as_deref() == Some(community_id))
    }
}

// ---------------------------------------------------------------------------
// Moderation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModFeedPage {
    #[serde(default)]
    pub no_posts: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub page: i64,
    #[serde(default)]
    pub posts: Option<Vec<Post>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: i64,
    #[serde(default)]
    pub community_id: String,
    #[serde(default)]
    pub post_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub reason_id: i64,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub target_id: String,
    #[serde(default)]
    pub action_taken: Option<String>,
    #[serde(default)]
    pub dealt_at: Option<Timestamp>,
    #[serde(default)]
    pub dealt_by: Option<String>,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub target: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReportDetails {
    #[serde(default)]
    pub no_reports: i64,
    #[serde(default)]
    pub no_post_reports: i64,
    #[serde(default)]
    pub no_comment_reports: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReportsPage {
    #[serde(default)]
    pub details: ReportDetails,
    #[serde(default)]
    pub reports: Option<Vec<Report>>,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub page: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommunityRequest {
    pub id: i64,
    #[serde(default)]
    pub by_user: String,
    #[serde(default)]
    pub community_name: String,
    #[serde(default)]
    pub community_exists: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub denied_note: Option<String>,
    #[serde(default)]
    pub denied_by: Option<String>,
    #[serde(default)]
    pub denied_at: Option<Timestamp>,
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: i64,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub icons: Option<Vec<serde_json::Value>>,
    #[serde(rename = "toURL", default)]
    pub to_url: Option<String>,
    /// `html`, `md`, or empty, describing `title`/`body`.
    #[serde(default)]
    pub text_format: Option<String>,
    #[serde(default)]
    pub seen: bool,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub updated_at: Option<Timestamp>,
}

impl Notification {
    /// Convert server-rendered HTML title/body into markdown for display.
    pub fn normalize(&mut self) {
        let is_html = self.text_format.as_deref() == Some("html")
            || [&self.title, &self.body].iter().any(|t| t.as_deref().is_some_and(|t| t.contains("</") || t.contains("<br")));
        if is_html {
            self.title = self.title.as_deref().map(crate::html::to_markdown);
            self.body = self.body.as_deref().map(crate::html::to_markdown);
            self.text_format = Some("md".into());
        }
    }

    /// The first icon as a URL, accepting either string icons or `{url}` objects.
    pub fn icon_url(&self) -> Option<String> {
        let first = self.icons.as_ref()?.first()?;
        match first {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(o) => o.get("url")?.as_str().map(Into::into),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsPage {
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub new_count: i64,
    #[serde(default)]
    pub items: Option<Vec<Notification>>,
    #[serde(default)]
    pub next: Option<serde_json::Value>,
}

impl NotificationsPage {
    /// The cursor as a string (the server may send a string or a number).
    pub fn next_cursor(&self) -> Option<String> {
        match self.next.as_ref()? {
            serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Admin
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SiteSettings {
    #[serde(default)]
    pub signups_disabled: bool,
    #[serde(default)]
    pub tor_blocked: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsEvent {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub unique_key: Option<String>,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AnalyticsPage {
    #[serde(default)]
    pub events: Option<Vec<AnalyticsEvent>>,
    #[serde(default)]
    pub next: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IpBlock {
    pub id: i64,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub masked_bits: i64,
    #[serde(default)]
    pub created_at: Option<Timestamp>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub cancelled_at: Option<Timestamp>,
    #[serde(default)]
    pub in_effect: bool,
    #[serde(default)]
    pub associated_users: Option<Vec<String>>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct IpBlocksPage {
    #[serde(default)]
    pub blocks: Option<Vec<IpBlock>>,
    #[serde(default)]
    pub next: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LinkInfo {
    #[serde(default)]
    pub title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_flattened() {
        let p: Post = serde_json::from_str(
            r#"{"id":"1","type":"text","publicId":"p","createdAt":"2026-11-01T10:00:00Z","images":[],"title":"\nI Built a  Luxury\r\nMousetrap "}"#,
        )
        .unwrap();
        assert_eq!(p.title, "I Built a Luxury Mousetrap");
    }

    #[test]
    fn post_images_both_shapes() {
        let flat: Vec<PostImage> = serde_json::from_str(r#"[{"id":"i1","url":"/images/i1.jpeg","width":10,"height":5}]"#).unwrap();
        assert_eq!(flat[0].image.as_ref().unwrap().id, "i1");
        let nested: Vec<PostImage> = serde_json::from_str(r#"[{"image":{"id":"i2","url":"/x"},"caption":"hi"}]"#).unwrap();
        assert_eq!(nested[0].image.as_ref().unwrap().id, "i2");
        assert_eq!(nested[0].caption.as_deref(), Some("hi"));
    }

    #[test]
    fn user_feed_items_decode() {
        let json = r#"{"items":[
            {"item":{"id":"p","type":"text","publicId":"x","createdAt":"2026-11-01T10:00:00Z"},"type":"post"},
            {"type":"comment","item":{"id":"c","createdAt":"2026-11-01T10:00:00Z","body":"hi"}}
        ],"next":"c"}"#;
        let page: UserFeedPage = serde_json::from_str(json).unwrap();
        let items = page.items.unwrap();
        assert!(matches!(&items[0], FeedItem::Post(p) if p.public_id == "x"));
        assert!(matches!(&items[1], FeedItem::Comment(c) if c.body == "hi"));
    }

    #[test]
    fn list_item_targets_and_mutes() {
        let li: ListItem = serde_json::from_str(
            r#"{"id":1,"listId":4,"targetType":"post","targetId":"p","targetItem":{"id":"p","publicId":"x","createdAt":"2026-11-01T10:00:00Z"}}"#,
        )
        .unwrap();
        assert_eq!(li.post().unwrap().public_id, "x");
        assert!(li.comment().is_none());

        let m: Mutes = serde_json::from_str(r#"{"communityMutes":[{"id":"c_1","mutedCommunityId":"cc"}],"userMutes":null}"#).unwrap();
        assert!(m.is_community_muted("cc"));
        assert!(!m.is_user_muted("u"));
    }

    #[test]
    fn notification_cursor_and_icons() {
        let p: NotificationsPage = serde_json::from_str(r#"{"items":[{"id":1,"icons":["/i.png"]}],"next":44}"#).unwrap();
        assert_eq!(p.next_cursor().as_deref(), Some("44"));
        assert_eq!(p.items.unwrap()[0].icon_url().as_deref(), Some("/i.png"));
    }

    #[test]
    fn average_color_both_shapes() {
        let mut img = Image { average_color: Some(serde_json::json!("rgb(27,69,157)")), ..Default::default() };
        assert_eq!(img.average_rgb(), Some((27, 69, 157)));
        img.average_color = Some(serde_json::json!({"r": 1, "g": 2, "b": 3}));
        assert_eq!(img.average_rgb(), Some((1, 2, 3)));
    }

    #[test]
    fn decodes_minimal_post() {
        let json = r#"{
            "id": "a", "type": "link", "publicId": "x1", "title": "T",
            "createdAt": "2026-11-01T10:00:00Z", "images": [],
            "link": {"url": "https://example.com", "hostname": "example.com", "image": null},
            "userVoted": null, "userVotedUp": null, "someNewField": 1
        }"#;
        let p: Post = serde_json::from_str(json).unwrap();
        assert_eq!(p.kind, PostType::Link);
        assert_eq!(p.vote(), None);
        let page: FeedPage = serde_json::from_str(r#"{"posts":[],"next":1794021441000000000}"#).unwrap();
        assert_eq!(page.next.as_deref(), Some("1794021441000000000"));
        let page: FeedPage = serde_json::from_str(r#"{"posts":null,"next":null}"#).unwrap();
        assert_eq!(page.next, None);
        assert_eq!(p.link.unwrap().hostname.as_deref(), Some("example.com"));
    }

    #[test]
    fn best_url_picks_smallest_sufficient_copy() {
        let mk = |w, u: &str| ImageCopy { width: w, url: u.into(), ..Default::default() };
        let img = Image { url: "orig".into(), copies: vec![mk(720, "m"), mk(120, "t"), mk(325, "s")], ..Default::default() };
        assert_eq!(img.best_url(300), "s");
        assert_eq!(img.best_url(2000), "m");
        assert_eq!(Image { url: "orig".into(), ..Default::default() }.best_url(1), "orig");
    }
}
