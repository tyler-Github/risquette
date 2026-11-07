//! §5 Posts, images & feeds, and §6 Comments.
//!
//! Post paths (`/api/posts/{postID}`) take the post's `publicId`; vote and
//! hide bodies take the post's `id`.

use super::{q, seg, Client};
use crate::{models::*, Result};
use serde::Serialize;
use serde_json::json;

/// Parameters for `GET /api/posts`.
#[derive(Debug, Clone, Default)]
pub struct FeedQuery {
    pub feed: FeedType,
    pub sort: FeedSort,
    pub community_id: Option<String>,
    pub limit: Option<u32>,
    pub next: Option<String>,
}

/// One image of an image post.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewPostImage {
    pub image_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

/// Body of `POST /api/posts`.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NewPost {
    #[serde(rename = "type")]
    pub kind: PostType,
    pub title: String,
    /// Community name.
    pub community: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_group: Option<UserGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<NewPostImage>,
}

/// Moderation actions for `PUT /api/posts/{postID}?action=…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostAction {
    Lock { as_group: UserGroup },
    Unlock,
    ChangeAsUser { group: UserGroup },
    Pin { site_wide: bool },
    Unpin { site_wide: bool },
    Announce,
}

/// Moderation actions for `PUT /api/posts/{postID}/comments/{commentID}?action=…`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentAction {
    Lock { as_group: UserGroup },
    Unlock,
    ChangeAsUser { group: UserGroup },
}

impl Client {
    // ----- feeds -----

    /// `GET /api/posts` — a page of the all / home / community / moderating feed.
    pub fn feed(&self, f: &FeedQuery) -> Result<FeedPage> {
        self.http_get(
            "/api/posts",
            q([
                ("feed", Some(f.feed.as_query().into())),
                ("sort", Some(f.sort.as_query().into())),
                ("communityId", f.community_id.clone()),
                ("limit", f.limit.map(|l| l.to_string())),
                ("next", f.next.clone()),
            ]),
        )
    }

    /// `GET /api/posts?filter=deleted|locked` — mod-tools view of a community (page-numbered).
    pub fn mod_feed(&self, community_id: &str, filter: &str, page: u32, limit: u32) -> Result<ModFeedPage> {
        self.http_get(
            "/api/posts",
            q([
                ("feed", Some("community".into())),
                ("filter", Some(filter.into())),
                ("communityId", Some(community_id.into())),
                ("page", Some(page.to_string())),
                ("limit", Some(limit.to_string())),
            ]),
        )
    }

    // ----- posts -----

    /// `GET /api/posts/{publicId}` — post with its first page of comments and its community.
    pub fn post(&self, public_id: &str) -> Result<Post> {
        self.http_get(&format!("/api/posts/{}", seg(public_id)), Vec::new())
    }

    /// `POST /api/posts`. **Mutating.**
    pub fn create_post(&self, post: &NewPost) -> Result<Post> {
        self.http_post("/api/posts", Vec::new(), post)
    }

    /// `PUT /api/posts/{publicId}` — edit title and/or body. **Mutating.**
    pub fn edit_post(&self, public_id: &str, title: &str, body: Option<&str>) -> Result<Post> {
        self.http_put(&format!("/api/posts/{}", seg(public_id)), Vec::new(), json!({ "title": title, "body": body }))
    }

    /// `PUT /api/posts/{publicId}?action=…` — lock, pin, announce, … **Mutating.**
    pub fn post_action(&self, public_id: &str, action: PostAction) -> Result<Post> {
        let query = match action {
            PostAction::Lock { as_group } => q([("action", Some("lock".into())), ("lockAs", Some(as_group.as_str().into()))]),
            PostAction::Unlock => q([("action", Some("unlock".into()))]),
            PostAction::ChangeAsUser { group } => {
                q([("action", Some("changeAsUser".into())), ("userGroup", Some(group.as_str().into()))])
            }
            PostAction::Pin { site_wide } => q([("action", Some("pin".into())), ("siteWide", Some(site_wide.to_string()))]),
            PostAction::Unpin { site_wide } => q([("action", Some("unpin".into())), ("siteWide", Some(site_wide.to_string()))]),
            PostAction::Announce => q([("action", Some("announce".into()))]),
        };
        self.http_put(&format!("/api/posts/{}", seg(public_id)), query, json!({}))
    }

    /// `DELETE /api/posts/{publicId}`. **Mutating.** `delete_content` also wipes the body/link/images.
    pub fn delete_post(&self, public_id: &str, delete_as: UserGroup, delete_content: bool) -> Result<Post> {
        self.http_delete(
            &format!("/api/posts/{}", seg(public_id)),
            q([("deleteAs", Some(delete_as.as_str().into())), ("deleteContent", Some(delete_content.to_string()))]),
        )
    }

    /// `POST /api/_postVote`. **Mutating.** Same direction again removes the vote.
    pub fn vote_post(&self, post_id: &str, up: bool) -> Result<Post> {
        self.http_post("/api/_postVote", Vec::new(), json!({ "postId": post_id, "up": up }))
    }

    /// `POST /api/_uploads` — upload an image for a post. **Mutating.**
    pub fn upload_image(&self, file_name: &str, bytes: &[u8]) -> Result<Image> {
        self.http_upload("/api/_uploads", file_name, bytes)
    }

    /// `PUT /api/images/{imageID}` — set alt text. **Mutating.**
    pub fn set_image_alt_text(&self, image_id: &str, alt_text: &str) -> Result<Image> {
        self.http_put(&format!("/api/images/{}", seg(image_id)), Vec::new(), json!({ "altText": alt_text }))
    }

    // ----- comments -----

    /// `GET /api/posts/{publicId}/comments` — a further page of comments.
    pub fn comments(&self, public_id: &str, next: Option<&str>) -> Result<CommentsPage> {
        self.http_get(&format!("/api/posts/{}/comments", seg(public_id)), q([("next", next.map(Into::into))]))
    }

    /// `GET /api/posts/{publicId}/comments?parentId=` — replies to one comment (a bare array).
    pub fn comment_replies(&self, public_id: &str, parent_id: &str) -> Result<Vec<Comment>> {
        let v: Option<Vec<Comment>> =
            self.http_get(&format!("/api/posts/{}/comments", seg(public_id)), q([("parentId", Some(parent_id.into()))]))?;
        Ok(v.unwrap_or_default())
    }

    /// `GET /api/comments/{commentID}`.
    pub fn comment(&self, comment_id: &str) -> Result<Comment> {
        self.http_get(&format!("/api/comments/{}", seg(comment_id)), Vec::new())
    }

    /// `POST /api/posts/{publicId}/comments`. **Mutating.**
    pub fn add_comment(&self, public_id: &str, parent_id: Option<&str>, body: &str, as_group: Option<UserGroup>) -> Result<Comment> {
        self.http_post(
            &format!("/api/posts/{}/comments", seg(public_id)),
            q([("userGroup", as_group.map(|g| g.as_str().into()))]),
            json!({ "parentCommentId": parent_id, "body": body }),
        )
    }

    /// `PUT /api/posts/{publicId}/comments/{commentID}` — edit body. **Mutating.**
    pub fn edit_comment(&self, public_id: &str, comment_id: &str, body: &str) -> Result<Comment> {
        self.http_put(&comment_path(public_id, comment_id), Vec::new(), json!({ "body": body }))
    }

    /// `PUT /api/posts/{publicId}/comments/{commentID}?action=…`. **Mutating.**
    pub fn comment_action(&self, public_id: &str, comment_id: &str, action: CommentAction) -> Result<Comment> {
        let query = match action {
            CommentAction::Lock { as_group } => q([("action", Some("lock".into())), ("lockAs", Some(as_group.as_str().into()))]),
            CommentAction::Unlock => q([("action", Some("unlock".into()))]),
            CommentAction::ChangeAsUser { group } => {
                q([("action", Some("changeAsUser".into())), ("userGroup", Some(group.as_str().into()))])
            }
        };
        self.http_put(&comment_path(public_id, comment_id), query, json!({}))
    }

    /// `DELETE /api/posts/{publicId}/comments/{commentID}`. **Mutating.**
    pub fn delete_comment(&self, public_id: &str, comment_id: &str, delete_as: UserGroup) -> Result<Comment> {
        self.http_delete(&comment_path(public_id, comment_id), q([("deleteAs", Some(delete_as.as_str().into()))]))
    }

    /// `POST /api/_commentVote`. **Mutating.** Same direction again removes the vote.
    pub fn vote_comment(&self, comment_id: &str, up: bool) -> Result<Comment> {
        self.http_post("/api/_commentVote", Vec::new(), json!({ "commentId": comment_id, "up": up }))
    }

    // ----- utilities -----

    /// `GET /api/_link_info` — server-side Open Graph title lookup (login required).
    pub fn link_info(&self, url: &str) -> Result<LinkInfo> {
        self.http_get("/api/_link_info", q([("url", Some(url.into()))]))
    }
}

fn comment_path(public_id: &str, comment_id: &str) -> String {
    format!("/api/posts/{}/comments/{}", seg(public_id), seg(comment_id))
}

#[cfg(test)]
mod tests {
    use super::super::test_server::serve;
    use super::*;

    const POST: &str = r#"{"id":"p1","type":"text","publicId":"abc","title":"T","createdAt":"2026-11-01T10:00:00Z"}"#;

    #[test]
    fn create_post_body() {
        let (base, rx) = serve(200, POST, None);
        let c = Client::new(&base).unwrap();
        let p = c
            .create_post(&NewPost {
                kind: PostType::Image,
                title: "Sunset".into(),
                community: "golang".into(),
                images: vec![NewPostImage { image_id: "img1".into(), caption: Some("bay".into()) }],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(p.public_id, "abc");
        let r = rx.recv().unwrap();
        assert!(r.request_line.starts_with("POST /api/posts "));
        assert_eq!(
            r.json(),
            json!({"type":"image","title":"Sunset","community":"golang","images":[{"imageId":"img1","caption":"bay"}]})
        );
    }

    #[test]
    fn post_action_and_delete_queries() {
        let (base, rx) = serve(200, POST, None);
        Client::new(&base).unwrap().post_action("abc", PostAction::Lock { as_group: UserGroup::Mods }).unwrap();
        assert!(rx.recv().unwrap().request_line.starts_with("PUT /api/posts/abc?action=lock&lockAs=mods "));

        let (base, rx) = serve(200, POST, None);
        Client::new(&base).unwrap().delete_post("abc", UserGroup::Normal, false).unwrap();
        assert!(rx.recv().unwrap().request_line.starts_with("DELETE /api/posts/abc?deleteAs=normal&deleteContent=false "));
    }

    #[test]
    fn add_comment_body_and_group() {
        let (base, rx) = serve(200, r#"{"id":"c1","createdAt":"2026-11-01T10:00:00Z"}"#, None);
        Client::new(&base).unwrap().add_comment("abc", Some("c0"), "hi", Some(UserGroup::Mods)).unwrap();
        let r = rx.recv().unwrap();
        assert!(r.request_line.starts_with("POST /api/posts/abc/comments?userGroup=mods "));
        assert_eq!(r.json(), json!({"parentCommentId":"c0","body":"hi"}));
    }

    #[test]
    fn upload_is_multipart() {
        let (base, rx) = serve(200, r#"{"id":"i1","url":"/images/i1.jpeg"}"#, None);
        let img = Client::new(&base).unwrap().upload_image("a.jpg", b"\xFF\xD8\xFFdata").unwrap();
        assert_eq!(img.id, "i1");
        let r = rx.recv().unwrap();
        assert!(r.request_line.starts_with("POST /api/_uploads "));
        assert!(r.header("Content-Type").unwrap().starts_with("multipart/form-data; boundary="));
        assert!(String::from_utf8_lossy(&r.body).contains("name=\"image\"; filename=\"a.jpg\""));
    }
}
