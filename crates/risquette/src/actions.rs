//! User actions on posts and comments (vote, save, report, delete, moderate…)
//! and the context menus that expose them.

use crate::{
    bus::{self, ContentEvent},
    modals,
    nav::{self, MenuItem, Target},
    session::Session,
    state::{api, AppState},
};
use discuit_api::{Comment, CommentAction, ContentType, Post, PostAction, UserGroup};
use gpui::{App, ClipboardItem};

/// Something with Discuit-style up/down votes.
pub trait Votable {
    fn counts(&mut self) -> (&mut i64, &mut i64, &mut Option<bool>, &mut Option<bool>);
}

impl Votable for Post {
    fn counts(&mut self) -> (&mut i64, &mut i64, &mut Option<bool>, &mut Option<bool>) {
        (&mut self.upvotes, &mut self.downvotes, &mut self.user_voted, &mut self.user_voted_up)
    }
}

impl Votable for Comment {
    fn counts(&mut self) -> (&mut i64, &mut i64, &mut Option<bool>, &mut Option<bool>) {
        (&mut self.upvotes, &mut self.downvotes, &mut self.user_voted, &mut self.user_voted_up)
    }
}

/// Apply a vote locally, mirroring the server: same direction removes it,
/// the opposite direction switches it.
pub fn apply_vote<V: Votable>(item: &mut V, up: bool) {
    let (ups, downs, voted, voted_up) = item.counts();
    let current = if *voted == Some(true) { *voted_up } else { None };
    match current {
        Some(prev) if prev == up => {
            if up { *ups -= 1 } else { *downs -= 1 }
            *voted = Some(false);
            *voted_up = None;
        }
        Some(prev) => {
            if prev { *ups -= 1 } else { *downs -= 1 }
            if up { *ups += 1 } else { *downs += 1 }
            *voted = Some(true);
            *voted_up = Some(up);
        }
        None => {
            if up { *ups += 1 } else { *downs += 1 }
            *voted = Some(true);
            *voted_up = Some(up);
        }
    }
}

/// Opens the login dialog and returns `false` when logged out.
pub fn require_login(cx: &mut App) -> bool {
    if Session::get(cx).is_logged_in() {
        return true;
    }
    modals::open_login(cx);
    false
}

/// Keep fields the server omits in action responses (e.g. community details).
pub fn merge_post(mut fresh: Post, old: &Post) -> Post {
    if fresh.community_pro_pic.is_none() {
        fresh.community_pro_pic = old.community_pro_pic.clone();
    }
    if fresh.community.is_none() {
        fresh.community = old.community.clone();
    }
    if fresh.author.is_none() {
        fresh.author = old.author.clone();
    }
    if fresh.comments.is_none() {
        fresh.comments = old.comments.clone();
    }
    fresh
}

pub fn vote_post(post: &Post, up: bool, cx: &mut App) {
    if !require_login(cx) {
        return;
    }
    let original = post.clone();
    let mut optimistic = post.clone();
    apply_vote(&mut optimistic, up);
    bus::emit(ContentEvent::PostUpdated(Box::new(optimistic)), cx);
    let id = post.id.clone();
    api(cx, move |c| c.vote_post(&id, up), move |result, cx| match result {
        Ok(p) => bus::emit(ContentEvent::PostUpdated(Box::new(merge_post(p, &original))), cx),
        Err(e) => {
            bus::emit(ContentEvent::PostUpdated(Box::new(original)), cx);
            nav::toast(format!("Vote failed: {}", e.user_message()), cx);
        }
    });
}

pub fn vote_comment(comment: &Comment, up: bool, cx: &mut App) {
    if !require_login(cx) {
        return;
    }
    let original = comment.clone();
    let mut optimistic = comment.clone();
    apply_vote(&mut optimistic, up);
    bus::emit(ContentEvent::CommentUpdated(Box::new(optimistic)), cx);
    let id = comment.id.clone();
    api(cx, move |c| c.vote_comment(&id, up), move |result, cx| match result {
        Ok(mut c) => {
            if c.author.is_none() {
                c.author = original.author.clone();
            }
            bus::emit(ContentEvent::CommentUpdated(Box::new(c)), cx)
        }
        Err(e) => {
            bus::emit(ContentEvent::CommentUpdated(Box::new(original)), cx);
            nav::toast(format!("Vote failed: {}", e.user_message()), cx);
        }
    });
}

fn post_action(post: &Post, action: PostAction, done_msg: &'static str, cx: &mut App) {
    let public_id = post.public_id.clone();
    let old = post.clone();
    api(cx, move |c| c.post_action(&public_id, action), move |r, cx| match r {
        Ok(p) => {
            bus::emit(ContentEvent::PostUpdated(Box::new(merge_post(p, &old))), cx);
            nav::toast(done_msg, cx);
        }
        Err(e) => nav::toast(e.user_message(), cx),
    });
}

fn delete_post(post: &Post, as_group: UserGroup, delete_content: bool, cx: &mut App) {
    let public_id = post.public_id.clone();
    let old = post.clone();
    api(cx, move |c| c.delete_post(&public_id, as_group, delete_content), move |r, cx| match r {
        Ok(p) => {
            bus::emit(ContentEvent::PostUpdated(Box::new(merge_post(p, &old))), cx);
            nav::toast("Post deleted", cx);
        }
        Err(e) => nav::toast(e.user_message(), cx),
    });
}

/// The "…" menu for a post, tailored to the viewer's permissions.
pub fn post_menu(post: &Post, cx: &App) -> Vec<MenuItem> {
    let session = Session::get(cx);
    let logged_in = session.is_logged_in();
    let is_author = session.user().is_some_and(|u| u.id == post.user_id);
    let is_mod = session.is_mod_of(&post.community_id);
    let is_admin = session.is_admin();
    let client = AppState::client(cx);
    let web = client.post_web_url(&post.community_name, &post.public_id);
    let mut items = Vec::new();

    let p = post.clone();
    items.push(MenuItem::new("Save to list…", move |_, cx| {
        if require_login(cx) {
            modals::open_save_to_list(ContentType::Post, p.id.clone(), cx);
        }
    }));
    let link = web.clone();
    items.push(MenuItem::new("Copy link", move |_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
        nav::toast("Link copied", cx);
    }));
    let link = web.clone();
    items.push(MenuItem::new("Open in browser", move |_, cx| cx.open_url(&link)));
    let community = post.community_name.clone();
    items.push(MenuItem::new(format!("Go to {}", post.community_name), move |_, cx| {
        nav::go(Target::Community(community.clone()), cx)
    }));
    let author = post.username.clone();
    items.push(MenuItem::new(format!("View @{}", post.username), move |_, cx| nav::go(Target::User(author.clone()), cx)));

    if logged_in && !post.deleted {
        if is_author {
            let p = post.clone();
            items.push(MenuItem::new("Edit", move |_, cx| modals::open_edit_post(&p, cx)));
            let p = post.clone();
            items.push(
                MenuItem::new("Delete", move |_, cx| {
                    let p = p.clone();
                    modals::confirm("Delete post?", "This can't be undone.", "Delete", move |cx| delete_post(&p, UserGroup::Normal, false, cx), cx);
                })
                .danger(),
            );
        }
        if !is_author {
            let p = post.clone();
            items.push(MenuItem::new("Hide", move |_, cx| {
                let id = p.id.clone();
                let removed = id.clone();
                api(cx, move |c| c.hide_post(&id), move |r, cx| match r {
                    Ok(_) => {
                        bus::emit(ContentEvent::PostRemoved(removed), cx);
                        nav::toast("Post hidden", cx);
                    }
                    Err(e) => nav::toast(e.user_message(), cx),
                });
            }));
            let p = post.clone();
            items.push(MenuItem::new("Report", move |_, cx| modals::open_report(ContentType::Post, p.id.clone(), cx)));
            let (uid, uname) = (post.user_id.clone(), post.username.clone());
            items.push(MenuItem::new(format!("Mute @{uname}"), move |_, cx| mute_user(uid.clone(), uname.clone(), cx)));
            let (cid, cname) = (post.community_id.clone(), post.community_name.clone());
            items.push(MenuItem::new(format!("Mute {cname}"), move |_, cx| mute_community(cid.clone(), cname.clone(), cx)));
        }
    }

    if is_mod && !post.deleted {
        let group = if is_admin && !session.is_mod_of_strict(&post.community_id) { UserGroup::Admins } else { UserGroup::Mods };
        let p = post.clone();
        if post.locked {
            items.push(MenuItem::new("Unlock (mod)", move |_, cx| post_action(&p, PostAction::Unlock, "Post unlocked", cx)));
        } else {
            items.push(MenuItem::new("Lock (mod)", move |_, cx| post_action(&p, PostAction::Lock { as_group: group }, "Post locked", cx)));
        }
        let p = post.clone();
        if post.is_pinned {
            items.push(MenuItem::new("Unpin (mod)", move |_, cx| post_action(&p, PostAction::Unpin { site_wide: false }, "Post unpinned", cx)));
        } else {
            items.push(MenuItem::new("Pin (mod)", move |_, cx| post_action(&p, PostAction::Pin { site_wide: false }, "Post pinned", cx)));
        }
        if is_author {
            let p = post.clone();
            items.push(MenuItem::new(format!("Show as {}", group_label(group)), move |_, cx| {
                post_action(&p, PostAction::ChangeAsUser { group }, "Updated", cx)
            }));
        }
        if !is_author {
            let p = post.clone();
            items.push(
                MenuItem::new(format!("Remove ({})", group_label(group)), move |_, cx| {
                    let p = p.clone();
                    modals::confirm("Remove post?", "The post will be removed from the community.", "Remove", move |cx| delete_post(&p, group, false, cx), cx);
                })
                .danger(),
            );
        }
    }
    if is_admin && !post.deleted {
        let p = post.clone();
        if post.is_pinned_site {
            items.push(MenuItem::new("Unpin site-wide (admin)", move |_, cx| post_action(&p, PostAction::Unpin { site_wide: true }, "Unpinned", cx)));
        } else {
            items.push(MenuItem::new("Pin site-wide (admin)", move |_, cx| post_action(&p, PostAction::Pin { site_wide: true }, "Pinned site-wide", cx)));
        }
        let p = post.clone();
        items.push(MenuItem::new("Announce (admin)", move |_, cx| {
            let p = p.clone();
            modals::confirm("Announce post?", "Every user will get a notification.", "Announce", move |cx| post_action(&p, PostAction::Announce, "Announced", cx), cx);
        }));
        let p = post.clone();
        items.push(
            MenuItem::new("Delete with content (admin)", move |_, cx| {
                let p = p.clone();
                modals::confirm("Delete post and its content?", "The body, link and images are removed permanently.", "Delete", move |cx| delete_post(&p, UserGroup::Admins, true, cx), cx);
            })
            .danger(),
        );
    }
    items
}

fn group_label(g: UserGroup) -> &'static str {
    match g {
        UserGroup::Normal => "user",
        UserGroup::Mods => "mod",
        UserGroup::Admins => "admin",
    }
}

pub fn mute_user(user_id: String, username: String, cx: &mut App) {
    api(cx, move |c| c.mute_user(&user_id), move |r, cx| match r {
        Ok(m) => {
            Session::entity(cx).update(cx, |s, cx| s.set_mutes(m, cx));
            nav::toast(format!("Muted @{username}"), cx);
        }
        Err(e) => nav::toast(e.user_message(), cx),
    });
}

pub fn mute_community(community_id: String, name: String, cx: &mut App) {
    api(cx, move |c| c.mute_community(&community_id), move |r, cx| match r {
        Ok(m) => {
            Session::entity(cx).update(cx, |s, cx| s.set_mutes(m, cx));
            nav::toast(format!("Muted {name}"), cx);
        }
        Err(e) => nav::toast(e.user_message(), cx),
    });
}

fn comment_action(comment: &Comment, action: CommentAction, msg: &'static str, cx: &mut App) {
    let public_id = comment.post_public_id.clone().unwrap_or_default();
    let id = comment.id.clone();
    api(cx, move |c| c.comment_action(&public_id, &id, action), move |r, cx| match r {
        Ok(c) => {
            bus::emit(ContentEvent::CommentUpdated(Box::new(c)), cx);
            nav::toast(msg, cx);
        }
        Err(e) => nav::toast(e.user_message(), cx),
    });
}

fn delete_comment(comment: &Comment, group: UserGroup, cx: &mut App) {
    let public_id = comment.post_public_id.clone().unwrap_or_default();
    let id = comment.id.clone();
    api(cx, move |c| c.delete_comment(&public_id, &id, group), move |r, cx| match r {
        Ok(c) => {
            bus::emit(ContentEvent::CommentUpdated(Box::new(c)), cx);
            nav::toast("Comment deleted", cx);
        }
        Err(e) => nav::toast(e.user_message(), cx),
    });
}

/// The "…" menu for a comment.
pub fn comment_menu(comment: &Comment, community_id: &str, cx: &App) -> Vec<MenuItem> {
    let session = Session::get(cx);
    let logged_in = session.is_logged_in();
    let is_author = session.user().is_some_and(|u| Some(&u.id) == comment.user_id.as_ref());
    let is_mod = session.is_mod_of(community_id);
    let is_admin = session.is_admin();
    let client = AppState::client(cx);
    let mut items = Vec::new();

    if let Some(pid) = &comment.post_public_id {
        let link = client.web_url(&format!("/{}/post/{}/{}", comment.community_name.clone().unwrap_or_default(), pid, comment.id));
        items.push(MenuItem::new("Copy link", move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
            nav::toast("Link copied", cx);
        }));
    }
    let body = comment.body.clone();
    items.push(MenuItem::new("Copy text", move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(body.clone()))));
    let c = comment.clone();
    items.push(MenuItem::new("Save to list…", move |_, cx| {
        if require_login(cx) {
            modals::open_save_to_list(ContentType::Comment, c.id.clone(), cx);
        }
    }));
    let author = comment.username.clone();
    items.push(MenuItem::new(format!("View @{author}"), move |_, cx| nav::go(Target::User(author.clone()), cx)));

    if logged_in && !comment.deleted {
        if is_author {
            let c = comment.clone();
            items.push(MenuItem::new("Edit", move |_, cx| modals::open_edit_comment(&c, cx)));
            let c = comment.clone();
            items.push(
                MenuItem::new("Delete", move |_, cx| {
                    let c = c.clone();
                    modals::confirm("Delete comment?", "This can't be undone.", "Delete", move |cx| delete_comment(&c, UserGroup::Normal, cx), cx);
                })
                .danger(),
            );
        } else {
            let c = comment.clone();
            items.push(MenuItem::new("Report", move |_, cx| modals::open_report(ContentType::Comment, c.id.clone(), cx)));
            if let Some(uid) = comment.user_id.clone() {
                let uname = comment.username.clone();
                items.push(MenuItem::new(format!("Mute @{uname}"), move |_, cx| mute_user(uid.clone(), uname.clone(), cx)));
            }
        }
    }
    if is_mod && !comment.deleted {
        let group = if is_admin && !session.is_mod_of_strict(community_id) { UserGroup::Admins } else { UserGroup::Mods };
        let c = comment.clone();
        if comment.locked {
            items.push(MenuItem::new("Unlock (mod)", move |_, cx| comment_action(&c, CommentAction::Unlock, "Comment unlocked", cx)));
        } else {
            items.push(MenuItem::new("Lock (mod)", move |_, cx| comment_action(&c, CommentAction::Lock { as_group: group }, "Comment locked", cx)));
        }
        if is_author {
            let c = comment.clone();
            items.push(MenuItem::new(format!("Show as {}", group_label(group)), move |_, cx| {
                comment_action(&c, CommentAction::ChangeAsUser { group }, "Updated", cx)
            }));
        } else {
            let c = comment.clone();
            items.push(
                MenuItem::new(format!("Remove ({})", group_label(group)), move |_, cx| {
                    let c = c.clone();
                    modals::confirm("Remove comment?", "The comment will be removed.", "Remove", move |cx| delete_comment(&c, group, cx), cx);
                })
                .danger(),
            );
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(up: i64, down: i64, voted: Option<bool>, voted_up: Option<bool>) -> Post {
        Post { upvotes: up, downvotes: down, user_voted: voted, user_voted_up: voted_up, created_at: Default::default(), ..Default::default() }
    }

    #[test]
    fn vote_transitions() {
        let mut p = post(1, 0, Some(false), None);
        apply_vote(&mut p, true);
        assert_eq!((p.upvotes, p.downvotes, p.vote()), (2, 0, Some(true)));
        apply_vote(&mut p, false); // switch
        assert_eq!((p.upvotes, p.downvotes, p.vote()), (1, 1, Some(false)));
        apply_vote(&mut p, false); // remove
        assert_eq!((p.upvotes, p.downvotes, p.vote()), (1, 0, None));
    }
}
