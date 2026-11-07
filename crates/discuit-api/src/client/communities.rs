//! §7 Communities: listing, joining, settings, rules, mods, reports, bans,
//! images, community requests and content reports.

use super::{q, seg, Client};
use crate::{models::*, Result};
use serde_json::json;

fn path(community_id: &str, rest: &str) -> String {
    format!("/api/communities/{}{rest}", seg(community_id))
}

impl Client {
    /// `GET /api/communities` — `set` is `all`, `default` or `subscribed`; `q` is a name prefix.
    pub fn communities(&self, set: Option<&str>, query: Option<&str>, sort: Option<&str>, limit: Option<u32>) -> Result<Vec<Community>> {
        let v: Option<Vec<Community>> = self.http_get(
            "/api/communities",
            q([
                ("set", set.map(Into::into)),
                ("q", query.map(Into::into)),
                ("sort", sort.map(Into::into)),
                ("limit", limit.map(|l| l.to_string())),
            ]),
        )?;
        Ok(v.unwrap_or_default())
    }

    /// `POST /api/communities`. **Mutating.**
    pub fn create_community(&self, name: &str, about: &str) -> Result<Community> {
        self.http_post("/api/communities", Vec::new(), json!({ "name": name, "about": about }))
    }

    /// `POST /api/_joinCommunity`. **Mutating.**
    pub fn join_community(&self, community_id: &str, leave: bool) -> Result<Community> {
        self.http_post("/api/_joinCommunity", Vec::new(), json!({ "communityId": community_id, "leave": leave }))
    }

    /// `GET /api/communities/{id}`.
    pub fn community(&self, community_id: &str) -> Result<Community> {
        self.http_get(&path(community_id, ""), Vec::new())
    }

    /// `GET /api/communities/{name}?byName=true`.
    pub fn community_by_name(&self, name: &str) -> Result<Community> {
        self.http_get(&path(name, ""), q([("byName", Some("true".into()))]))
    }

    /// `PUT /api/communities/{id}`. **Mutating.**
    pub fn update_community(&self, community_id: &str, nsfw: bool, about: &str, posting_restricted: bool) -> Result<Community> {
        self.http_put(&path(community_id, ""), Vec::new(), json!({ "nsfw": nsfw, "about": about, "postingRestricted": posting_restricted }))
    }

    // ----- rules -----

    /// `GET /api/communities/{id}/rules`.
    pub fn rules(&self, community_id: &str) -> Result<Vec<CommunityRule>> {
        let v: Option<Vec<CommunityRule>> = self.http_get(&path(community_id, "/rules"), Vec::new())?;
        Ok(v.unwrap_or_default())
    }

    /// `POST /api/communities/{id}/rules`. **Mutating.** Returns the refreshed rules.
    pub fn add_rule(&self, community_id: &str, rule: &str, description: Option<&str>) -> Result<Vec<CommunityRule>> {
        let v: Option<Vec<CommunityRule>> =
            self.http_post(&path(community_id, "/rules"), Vec::new(), json!({ "rule": rule, "description": description }))?;
        Ok(v.unwrap_or_default())
    }

    /// `GET /api/communities/{id}/rules/{ruleID}`.
    pub fn rule(&self, community_id: &str, rule_id: i64) -> Result<CommunityRule> {
        self.http_get(&path(community_id, &format!("/rules/{rule_id}")), Vec::new())
    }

    /// `PUT /api/communities/{id}/rules/{ruleID}`. **Mutating.**
    pub fn update_rule(&self, community_id: &str, rule_id: i64, rule: &str, description: Option<&str>, z_index: i64) -> Result<CommunityRule> {
        self.http_put(
            &path(community_id, &format!("/rules/{rule_id}")),
            Vec::new(),
            json!({ "rule": rule, "description": description, "zIndex": z_index }),
        )
    }

    /// `DELETE /api/communities/{id}/rules/{ruleID}`. **Mutating.**
    pub fn delete_rule(&self, community_id: &str, rule_id: i64) -> Result<CommunityRule> {
        self.http_delete(&path(community_id, &format!("/rules/{rule_id}")), Vec::new())
    }

    // ----- mods -----

    /// `GET /api/communities/{id}/mods`.
    pub fn mods(&self, community_id: &str) -> Result<Vec<User>> {
        let v: Option<Vec<User>> = self.http_get(&path(community_id, "/mods"), Vec::new())?;
        Ok(v.unwrap_or_default())
    }

    /// `POST /api/communities/{id}/mods`. **Mutating.**
    pub fn add_mod(&self, community_id: &str, username: &str) -> Result<Vec<User>> {
        let v: Option<Vec<User>> = self.http_post(&path(community_id, "/mods"), Vec::new(), json!({ "username": username }))?;
        Ok(v.unwrap_or_default())
    }

    /// `DELETE /api/communities/{id}/mods/{username}`. **Mutating.**
    pub fn remove_mod(&self, community_id: &str, username: &str) -> Result<User> {
        self.http_delete(&path(community_id, &format!("/mods/{}", seg(username))), Vec::new())
    }

    // ----- reports -----

    /// `GET /api/communities/{id}/reports` — `filter` is `all`, `posts` or `comments`.
    pub fn reports(&self, community_id: &str, filter: &str, page: u32, limit: u32) -> Result<ReportsPage> {
        self.http_get(
            &path(community_id, "/reports"),
            q([("filter", Some(filter.into())), ("page", Some(page.to_string())), ("limit", Some(limit.to_string()))]),
        )
    }

    /// `DELETE /api/communities/{id}/reports/{reportID}` — resolve a report. **Mutating.**
    pub fn delete_report(&self, community_id: &str, report_id: i64) -> Result<Report> {
        self.http_delete(&path(community_id, &format!("/reports/{report_id}")), Vec::new())
    }

    /// `POST /api/_report`. **Mutating.** `reason` is a report reason id from `/api/_initial`.
    pub fn report(&self, target_type: ContentType, target_id: &str, reason: i64) -> Result<Report> {
        self.http_post("/api/_report", Vec::new(), json!({ "type": target_type.as_str(), "targetId": target_id, "reason": reason }))
    }

    // ----- bans -----

    /// `GET /api/communities/{id}/banned`.
    pub fn banned(&self, community_id: &str) -> Result<Vec<User>> {
        let v: Option<Vec<User>> = self.http_get(&path(community_id, "/banned"), Vec::new())?;
        Ok(v.unwrap_or_default())
    }

    /// `POST /api/communities/{id}/banned`. **Mutating.** `expires` is RFC 3339; `None` bans permanently.
    pub fn ban_from_community(&self, community_id: &str, username: &str, expires: Option<Timestamp>) -> Result<User> {
        let mut body = json!({ "username": username });
        if let Some(e) = expires {
            body["expires"] = json!(e);
        }
        self.http_post(&path(community_id, "/banned"), Vec::new(), body)
    }

    /// `DELETE /api/communities/{id}/banned`. **Mutating.**
    pub fn unban_from_community(&self, community_id: &str, username: &str) -> Result<User> {
        self.http_delete_with(&path(community_id, "/banned"), Vec::new(), json!({ "username": username }))
    }

    // ----- images -----

    /// `POST /api/communities/{id}/pro_pic`. **Mutating.**
    pub fn upload_community_pro_pic(&self, community_id: &str, file_name: &str, bytes: &[u8]) -> Result<Community> {
        self.http_upload(&path(community_id, "/pro_pic"), file_name, bytes)
    }

    /// `DELETE /api/communities/{id}/pro_pic`. **Mutating.**
    pub fn delete_community_pro_pic(&self, community_id: &str) -> Result<Community> {
        self.http_delete(&path(community_id, "/pro_pic"), Vec::new())
    }

    /// `POST /api/communities/{id}/banner_image`. **Mutating.**
    pub fn upload_community_banner(&self, community_id: &str, file_name: &str, bytes: &[u8]) -> Result<Community> {
        self.http_upload(&path(community_id, "/banner_image"), file_name, bytes)
    }

    /// `DELETE /api/communities/{id}/banner_image`. **Mutating.**
    pub fn delete_community_banner(&self, community_id: &str) -> Result<Community> {
        self.http_delete(&path(community_id, "/banner_image"), Vec::new())
    }

    // ----- community requests -----

    /// `POST /api/community_requests`. **Mutating.**
    pub fn request_community(&self, name: &str, note: &str) -> Result<()> {
        self.http_post("/api/community_requests", Vec::new(), json!({ "name": name, "note": note }))
    }

    /// `GET /api/community_requests` (admin).
    pub fn community_requests(&self) -> Result<Vec<CommunityRequest>> {
        let v: Option<Vec<CommunityRequest>> = self.http_get("/api/community_requests", Vec::new())?;
        Ok(v.unwrap_or_default())
    }

    /// `DELETE /api/community_requests/{requestID}` (admin). **Mutating.**
    pub fn delete_community_request(&self, request_id: i64) -> Result<()> {
        self.http_delete(&format!("/api/community_requests/{request_id}"), Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_server::serve;
    use super::*;

    #[test]
    fn unban_sends_body_on_delete() {
        let (base, rx) = serve(200, r#"{"id":"u","username":"bob"}"#, None);
        Client::new(&base).unwrap().unban_from_community("cid", "bob").unwrap();
        let r = rx.recv().unwrap();
        assert!(r.request_line.starts_with("DELETE /api/communities/cid/banned "));
        assert_eq!(r.json(), json!({"username":"bob"}));
    }

    #[test]
    fn join_body() {
        let (base, rx) = serve(200, r#"{"id":"cid","name":"golang"}"#, None);
        Client::new(&base).unwrap().join_community("cid", true).unwrap();
        assert_eq!(rx.recv().unwrap().json(), json!({"communityId":"cid","leave":true}));
    }
}
