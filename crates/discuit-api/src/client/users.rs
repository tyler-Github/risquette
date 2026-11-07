//! §2 Users (profiles, user feeds, profile pictures, badges, hidden posts).

use super::{q, seg, Client};
use crate::{models::*, Result};
use serde_json::json;

impl Client {
    /// `GET /api/users/{username}`.
    pub fn user(&self, username: &str) -> Result<User> {
        self.http_get(&format!("/api/users/{}", seg(username)), Vec::new())
    }

    /// `GET /api/users/{username}?adminsView=true` (admin only) — extended shape as raw JSON.
    pub fn user_admin_view(&self, username: &str) -> Result<serde_json::Value> {
        self.http_get(&format!("/api/users/{}", seg(username)), q([("adminsView", Some("true".into()))]))
    }

    /// `DELETE /api/users/{username}`. **Mutating.** Deletes an account (own, or any as admin).
    pub fn delete_account(&self, username: &str, password: &str) -> Result<serde_json::Value> {
        self.http_delete_with(&format!("/api/users/{}", seg(username)), Vec::new(), json!({ "password": password }))
    }

    /// `GET /api/users/{username}/feed` — a user's posts and comments, newest first.
    pub fn user_feed(&self, username: &str, filter: Option<&str>, limit: Option<u32>, next: Option<&str>) -> Result<UserFeedPage> {
        self.http_get(
            &format!("/api/users/{}/feed", seg(username)),
            q([
                ("filter", filter.map(Into::into)),
                ("limit", limit.map(|l| l.to_string())),
                ("next", next.map(Into::into)),
            ]),
        )
    }

    /// `POST /api/users/{username}/pro_pic`. **Mutating.**
    pub fn upload_user_pro_pic(&self, username: &str, file_name: &str, bytes: &[u8]) -> Result<User> {
        self.http_upload(&format!("/api/users/{}/pro_pic", seg(username)), file_name, bytes)
    }

    /// `DELETE /api/users/{username}/pro_pic`. **Mutating.**
    pub fn delete_user_pro_pic(&self, username: &str) -> Result<User> {
        self.http_delete(&format!("/api/users/{}/pro_pic", seg(username)), Vec::new())
    }

    /// `POST /api/users/{username}/badges` (admin). **Mutating.**
    pub fn add_badge(&self, username: &str, badge_type: &str) -> Result<Vec<Badge>> {
        self.http_post(&format!("/api/users/{}/badges", seg(username)), Vec::new(), json!({ "type": badge_type }))
    }

    /// `DELETE /api/users/{username}/badges/{badgeId}` (admin). **Mutating.**
    /// With `by_type`, `badge` is a badge type string instead of a numeric id.
    pub fn remove_badge(&self, username: &str, badge: &str, by_type: bool) -> Result<serde_json::Value> {
        self.http_delete(
            &format!("/api/users/{}/badges/{}", seg(username), seg(badge)),
            q([("byType", by_type.then(|| "true".into()))]),
        )
    }

    /// `POST /api/hidden_posts`. **Mutating.** `post_id` is the post's `id` (not `publicId`).
    pub fn hide_post(&self, post_id: &str) -> Result<serde_json::Value> {
        self.http_post("/api/hidden_posts", Vec::new(), json!({ "postId": post_id }))
    }

    /// `DELETE /api/hidden_posts/{postId}`. **Mutating.**
    pub fn unhide_post(&self, post_id: &str) -> Result<serde_json::Value> {
        self.http_delete(&format!("/api/hidden_posts/{}", seg(post_id)), Vec::new())
    }

    /// `GET /api/users` (admin) — 100 per page.
    pub fn admin_users(&self, next: Option<&str>) -> Result<AdminUsersPage> {
        self.http_get("/api/users", q([("next", next.map(Into::into))]))
    }

    /// `GET /api/comments` (admin) — site-wide comments, 100 per page.
    pub fn admin_comments(&self, next: Option<&str>) -> Result<CommentsPage> {
        self.http_get("/api/comments", q([("next", next.map(Into::into))]))
    }
}
