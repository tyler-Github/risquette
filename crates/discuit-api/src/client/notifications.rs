//! §8 Notifications & web push.

use super::{q, Client};
use crate::{models::*, Result};

impl Client {
    /// GET with `render=true&format=html`, retrying without `format` if the
    /// server rejects it (`invalid-render-format`). The API docs list `md`
    /// as a format, but the live server rejects it.
    fn get_rendered<T: serde::de::DeserializeOwned>(&self, path: &str, extra: Vec<(&str, String)>) -> Result<T> {
        let mut query = extra.clone();
        query.push(("render", "true".into()));
        query.push(("format", "html".into()));
        match self.http_get(path, query) {
            Err(e) if e.code() == Some("invalid-render-format") => {
                let mut query = extra;
                query.push(("render", "true".into()));
                self.http_get(path, query)
            }
            other => other,
        }
    }

    /// `GET /api/notifications` — 10 per page, rendered for display (text converted to markdown).
    pub fn notifications(&self, next: Option<&str>) -> Result<NotificationsPage> {
        let mut page: NotificationsPage = self.get_rendered("/api/notifications", q([("next", next.map(Into::into))]))?;
        page.items.iter_mut().flatten().for_each(Notification::normalize);
        Ok(page)
    }

    /// `POST /api/notifications?action=resetNewCount`. **Mutating.**
    pub fn reset_new_notification_count(&self) -> Result<serde_json::Value> {
        self.http_post("/api/notifications", q([("action", Some("resetNewCount".into()))]), serde_json::json!({}))
    }

    /// `POST /api/notifications?action=markAllAsSeen[&type=…]`. **Mutating.**
    pub fn mark_all_notifications_seen(&self, kind: Option<&str>) -> Result<serde_json::Value> {
        self.http_post(
            "/api/notifications",
            q([("action", Some("markAllAsSeen".into())), ("type", kind.map(Into::into))]),
            serde_json::json!({}),
        )
    }

    /// `POST /api/notifications?action=deleteAll`. **Mutating.**
    pub fn delete_all_notifications(&self) -> Result<serde_json::Value> {
        self.http_post("/api/notifications", q([("action", Some("deleteAll".into()))]), serde_json::json!({}))
    }

    /// `GET /api/notifications/{id}`.
    pub fn notification(&self, id: i64) -> Result<Notification> {
        let mut n: Notification = self.get_rendered(&format!("/api/notifications/{id}"), Vec::new())?;
        n.normalize();
        Ok(n)
    }

    /// `PUT /api/notifications/{id}?action=markAsSeen&seen=…`. **Mutating.**
    pub fn mark_notification_seen(&self, id: i64, seen: bool) -> Result<Notification> {
        self.http_put(
            &format!("/api/notifications/{id}"),
            q([("action", Some("markAsSeen".into())), ("seen", Some(seen.to_string()))]),
            serde_json::json!({}),
        )
    }

    /// `DELETE /api/notifications/{id}`. **Mutating.**
    pub fn delete_notification(&self, id: i64) -> Result<Notification> {
        self.http_delete(&format!("/api/notifications/{id}"), Vec::new())
    }

    /// `POST /api/push_subscriptions` — register a Web Push subscription. **Mutating.**
    /// Desktop clients normally have no push endpoint; exposed for completeness.
    pub fn push_subscription(&self, subscription: &serde_json::Value) -> Result<serde_json::Value> {
        self.http_post("/api/push_subscriptions", Vec::new(), subscription)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_server::serve_seq;
    use super::*;

    #[test]
    fn falls_back_without_format_and_converts_html() {
        let (base, rx) = serve_seq(vec![
            (400, r#"{"status":400,"code":"invalid-render-format","message":"Invalid render format."}"#, None),
            (200, r#"{"count":1,"newCount":1,"items":[{"id":5,"type":"comment_reply","title":"<b>bob</b> replied","textFormat":"html","toURL":"/g/post/x"}],"next":null}"#, None),
        ]);
        let page = Client::new(&base).unwrap().notifications(None).unwrap();
        let first = rx.recv().unwrap();
        assert!(first.request_line.contains("format=html"), "{}", first.request_line);
        let second = rx.recv().unwrap();
        assert!(!second.request_line.contains("format="), "{}", second.request_line);
        assert!(second.request_line.contains("render=true"));
        assert_eq!(page.items.unwrap()[0].title.as_deref(), Some("**bob** replied"));
    }
}
