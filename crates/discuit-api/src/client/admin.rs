//! §10 Admin, site settings, analytics & IP blocks.

use super::{q, Client};
use crate::{models::*, Result};
use serde_json::json;

impl Client {
    /// `POST /api/_admin` — site-wide ban. **Mutating.** `delete_content_days` also removes recent content.
    pub fn admin_ban_user(&self, username: &str, delete_content_days: Option<u32>) -> Result<serde_json::Value> {
        let mut body = json!({ "action": "ban_user", "username": username });
        if let Some(d) = delete_content_days {
            body["deleteContentDays"] = json!(d);
        }
        self.http_post("/api/_admin", Vec::new(), body)
    }

    /// `POST /api/_admin` `unban_user`. **Mutating.**
    pub fn admin_unban_user(&self, username: &str) -> Result<serde_json::Value> {
        self.http_post("/api/_admin", Vec::new(), json!({ "action": "unban_user", "username": username }))
    }

    /// `POST /api/_admin` `add_default_forum` / `remove_default_forum`. **Mutating.**
    pub fn admin_set_default_community(&self, name: &str, default: bool) -> Result<serde_json::Value> {
        let action = if default { "add_default_forum" } else { "remove_default_forum" };
        self.http_post("/api/_admin", Vec::new(), json!({ "action": action, "name": name }))
    }

    /// `POST /api/_admin` `deny_comm` — deny a community request. **Mutating.**
    pub fn admin_deny_community_request(&self, request_id: i64, note: &str) -> Result<serde_json::Value> {
        self.http_post("/api/_admin", Vec::new(), json!({ "action": "deny_comm", "id": request_id, "body": note }))
    }

    /// `GET /api/site_settings`.
    pub fn site_settings(&self) -> Result<SiteSettings> {
        self.http_get("/api/site_settings", Vec::new())
    }

    /// `PUT /api/site_settings`. **Mutating.**
    pub fn update_site_settings(&self, settings: &SiteSettings) -> Result<SiteSettings> {
        self.http_put("/api/site_settings", Vec::new(), settings)
    }

    /// `GET /api/analytics/bss`.
    pub fn analytics_bss(&self, limit: Option<u32>, next: Option<&str>) -> Result<AnalyticsPage> {
        self.http_get("/api/analytics/bss", q([("limit", limit.map(|l| l.to_string())), ("next", next.map(Into::into))]))
    }

    /// `POST /api/analytics` — only `pwa_use` is accepted. **Mutating.**
    pub fn record_analytics(&self, event: &str) -> Result<serde_json::Value> {
        self.http_post("/api/analytics", Vec::new(), json!({ "event": event }))
    }

    /// `GET /api/ipblocks`.
    pub fn ip_blocks(&self, limit: Option<u32>, next: Option<&str>) -> Result<IpBlocksPage> {
        self.http_get("/api/ipblocks", q([("limit", limit.map(|l| l.to_string())), ("next", next.map(Into::into))]))
    }

    /// `POST /api/ipblocks`. **Mutating.** `expires_in_hours = 0` is permanent.
    pub fn create_ip_block(&self, address: &str, expires_in_hours: u32, note: &str) -> Result<IpBlock> {
        self.http_post("/api/ipblocks", Vec::new(), json!({ "address": address, "expiresIn": expires_in_hours, "note": note }))
    }

    /// `DELETE /api/ipblocks`. **Mutating — cancels every block.**
    pub fn cancel_all_ip_blocks(&self) -> Result<IpBlocksPage> {
        self.http_delete("/api/ipblocks", Vec::new())
    }

    /// `GET /api/ipblocks/{id}`.
    pub fn ip_block(&self, id: i64) -> Result<IpBlock> {
        self.http_get(&format!("/api/ipblocks/{id}"), Vec::new())
    }

    /// `DELETE /api/ipblocks/{id}`. **Mutating.**
    pub fn cancel_ip_block(&self, id: i64) -> Result<IpBlock> {
        self.http_delete(&format!("/api/ipblocks/{id}"), Vec::new())
    }
}
