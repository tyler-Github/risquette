//! §4 Mutes.

use super::{q, seg, Client};
use crate::{models::*, Result};
use serde_json::json;

/// The all-zero id Discuit treats as "not set" in `POST /api/mutes`.
const ZERO_ID: &str = "000000000000000000000000";

impl Client {
    /// `GET /api/mutes`.
    pub fn mutes(&self) -> Result<Mutes> {
        self.http_get("/api/mutes", Vec::new())
    }

    /// `POST /api/mutes` — mute a user. **Mutating.**
    pub fn mute_user(&self, user_id: &str) -> Result<Mutes> {
        self.http_post("/api/mutes", Vec::new(), json!({ "userId": user_id, "communityId": ZERO_ID }))
    }

    /// `POST /api/mutes` — mute a community. **Mutating.**
    pub fn mute_community(&self, community_id: &str) -> Result<Mutes> {
        self.http_post("/api/mutes", Vec::new(), json!({ "userId": ZERO_ID, "communityId": community_id }))
    }

    /// `DELETE /api/mutes?type=user|community` — clear all mutes of one kind. **Mutating.**
    pub fn clear_mutes(&self, kind: MuteKind) -> Result<Mutes> {
        self.http_delete("/api/mutes", q([("type", Some(kind.as_str().into()))]))
    }

    /// `DELETE /api/mutes/users/{mutedUserID}`. **Mutating.**
    pub fn unmute_user(&self, user_id: &str) -> Result<serde_json::Value> {
        self.http_delete(&format!("/api/mutes/users/{}", seg(user_id)), Vec::new())
    }

    /// `DELETE /api/mutes/communities/{mutedCommunityID}`. **Mutating.**
    pub fn unmute_community(&self, community_id: &str) -> Result<serde_json::Value> {
        self.http_delete(&format!("/api/mutes/communities/{}", seg(community_id)), Vec::new())
    }

    /// `DELETE /api/mutes/{muteID}` — by `u_<n>` / `c_<n>` id. **Mutating.**
    pub fn unmute(&self, mute_id: &str) -> Result<serde_json::Value> {
        self.http_delete(&format!("/api/mutes/{}", seg(mute_id)), Vec::new())
    }
}
