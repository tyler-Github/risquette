//! §3 Lists (saved posts and comments).
//!
//! Every list endpoint exists in two flavours — by owner + list name, and by
//! numeric list id. Both are exposed; the UI uses the id-based ones.

use super::{Client, q, seg};
use crate::{Result, models::*};
use serde_json::json;

fn by_name(username: &str, listname: &str) -> String {
    format!("/api/users/{}/lists/{}", seg(username), seg(listname))
}

impl Client {
    /// `GET /api/users/{username}/lists`. Non-owners only see public lists.
    pub fn user_lists(&self, username: &str, sort: Option<&str>, filter: Option<&str>) -> Result<Vec<List>> {
        let v: Option<Vec<List>> =
            self.http_get(&format!("/api/users/{}/lists", seg(username)), q([("sort", sort.map(Into::into)), ("filter", filter.map(Into::into))]))?;
        Ok(v.unwrap_or_default())
    }

    /// `POST /api/users/{username}/lists`. **Mutating.** Returns the refreshed lists.
    pub fn create_list(&self, username: &str, list: &NewList) -> Result<Vec<List>> {
        let v: Option<Vec<List>> = self.http_post(&format!("/api/users/{}/lists", seg(username)), Vec::new(), list)?;
        Ok(v.unwrap_or_default())
    }

    /// `GET /api/lists/_saved_to` — ids of the viewer's lists containing an item.
    pub fn saved_to(&self, target_id: &str, target_type: ContentType) -> Result<Vec<i64>> {
        let v: Option<Vec<i64>> = self.http_get("/api/lists/_saved_to", q([("id", Some(target_id.into())), ("type", Some(target_type.as_str().into()))]))?;
        Ok(v.unwrap_or_default())
    }

    /// `GET /api/users/{username}/lists/{listname}`.
    pub fn list_by_name(&self, username: &str, listname: &str) -> Result<List> {
        self.http_get(&by_name(username, listname), Vec::new())
    }

    /// `PUT /api/users/{username}/lists/{listname}`. **Mutating.**
    pub fn update_list_by_name(&self, username: &str, listname: &str, patch: &ListPatch) -> Result<List> {
        self.http_put(&by_name(username, listname), Vec::new(), patch)
    }

    /// `DELETE /api/users/{username}/lists/{listname}`. **Mutating.**
    pub fn delete_list_by_name(&self, username: &str, listname: &str) -> Result<List> {
        self.http_delete(&by_name(username, listname), Vec::new())
    }

    /// `GET /api/lists/{listId}`.
    pub fn list(&self, list_id: i64) -> Result<List> {
        self.http_get(&format!("/api/lists/{list_id}"), Vec::new())
    }

    /// `PUT /api/lists/{listId}`. **Mutating.**
    pub fn update_list(&self, list_id: i64, patch: &ListPatch) -> Result<List> {
        self.http_put(&format!("/api/lists/{list_id}"), Vec::new(), patch)
    }

    /// `DELETE /api/lists/{listId}`. **Mutating.**
    pub fn delete_list(&self, list_id: i64) -> Result<List> {
        self.http_delete(&format!("/api/lists/{list_id}"), Vec::new())
    }

    /// `GET /api/users/{username}/lists/{listname}/items`.
    pub fn list_items_by_name(&self, username: &str, listname: &str, limit: Option<u32>, next: Option<&str>, sort: Option<&str>) -> Result<ListItemsPage> {
        self.http_get(&format!("{}/items", by_name(username, listname)), items_query(limit, next, sort))
    }

    /// `POST /api/users/{username}/lists/{listname}/items`. **Mutating.**
    pub fn add_list_item_by_name(&self, username: &str, listname: &str, target_type: ContentType, target_id: &str) -> Result<ListItemsPage> {
        self.http_post(&format!("{}/items", by_name(username, listname)), Vec::new(), item_body(target_type, target_id))
    }

    /// `DELETE /api/users/{username}/lists/{listname}/items` with a target. **Mutating.**
    pub fn remove_list_item_by_name(&self, username: &str, listname: &str, target_type: ContentType, target_id: &str) -> Result<ListItemsPage> {
        self.http_delete_with(&format!("{}/items", by_name(username, listname)), Vec::new(), item_body(target_type, target_id))
    }

    /// `DELETE /api/users/{username}/lists/{listname}/items` with no body. **Mutating — removes every item.**
    pub fn clear_list_by_name(&self, username: &str, listname: &str) -> Result<ListItemsPage> {
        self.http_delete(&format!("{}/items", by_name(username, listname)), Vec::new())
    }

    /// `GET /api/lists/{listId}/items`.
    pub fn list_items(&self, list_id: i64, limit: Option<u32>, next: Option<&str>, sort: Option<&str>) -> Result<ListItemsPage> {
        self.http_get(&format!("/api/lists/{list_id}/items"), items_query(limit, next, sort))
    }

    /// `POST /api/lists/{listId}/items`. **Mutating.**
    pub fn add_list_item(&self, list_id: i64, target_type: ContentType, target_id: &str) -> Result<ListItemsPage> {
        self.http_post(&format!("/api/lists/{list_id}/items"), Vec::new(), item_body(target_type, target_id))
    }

    /// `DELETE /api/lists/{listId}/items` with a target. **Mutating.**
    pub fn remove_list_item(&self, list_id: i64, target_type: ContentType, target_id: &str) -> Result<ListItemsPage> {
        self.http_delete_with(&format!("/api/lists/{list_id}/items"), Vec::new(), item_body(target_type, target_id))
    }

    /// `DELETE /api/lists/{listId}/items` with no body. **Mutating — removes every item.**
    pub fn clear_list(&self, list_id: i64) -> Result<ListItemsPage> {
        self.http_delete(&format!("/api/lists/{list_id}/items"), Vec::new())
    }

    /// `DELETE /api/lists/{listId}/items/{itemId}`. **Mutating.**
    pub fn delete_list_item(&self, list_id: i64, item_id: i64) -> Result<ListItem> {
        self.http_delete(&format!("/api/lists/{list_id}/items/{item_id}"), Vec::new())
    }
}

fn items_query<'a>(limit: Option<u32>, next: Option<&str>, sort: Option<&str>) -> super::Query<'a> {
    q([("limit", limit.map(|l| l.to_string())), ("next", next.map(Into::into)), ("sort", sort.map(Into::into))])
}

fn item_body(target_type: ContentType, target_id: &str) -> serde_json::Value {
    json!({ "targetType": target_type.as_str(), "targetId": target_id })
}
