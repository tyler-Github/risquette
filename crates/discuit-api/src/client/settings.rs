//! §9 Account settings.

use super::{Client, q};
use crate::{Result, models::*};
use serde_json::json;

impl Client {
    /// `POST /api/_settings?action=updateProfile`. **Mutating.**
    pub fn update_profile(&self, profile: &ProfileUpdate) -> Result<User> {
        self.http_post("/api/_settings", q([("action", Some("updateProfile".into()))]), profile)
    }

    /// `POST /api/_settings?action=changePassword`. **Mutating.**
    pub fn change_password(&self, current: &str, new: &str, repeat: &str) -> Result<User> {
        self.http_post(
            "/api/_settings",
            q([("action", Some("changePassword".into()))]),
            json!({ "password": current, "newPassword": new, "repeatPassword": repeat }),
        )
    }
}
