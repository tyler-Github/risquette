//! A global event bus so every view showing a post or comment stays in sync
//! after votes, edits, deletes and so on.

use discuit_api::{Comment, Community, Post};
use gpui::{App, AppContext, Entity, EventEmitter, Global};

#[derive(Debug, Clone)]
pub enum ContentEvent {
    PostUpdated(Box<Post>),
    /// Post removed from view (hidden, deleted, or its author/community muted). Carries `post.id`.
    PostRemoved(String),
    CommentUpdated(Box<Comment>),
    CommentAdded(Box<Comment>),
    CommunityUpdated(Box<Community>),
}

pub struct Bus;

impl EventEmitter<ContentEvent> for Bus {}

struct BusGlobal(Entity<Bus>);

impl Global for BusGlobal {}

pub fn init(cx: &mut App) {
    let bus = cx.new(|_| Bus);
    cx.set_global(BusGlobal(bus));
}

pub fn entity(cx: &App) -> Entity<Bus> {
    cx.global::<BusGlobal>().0.clone()
}

pub fn emit(event: ContentEvent, cx: &mut App) {
    let bus = entity(cx);
    bus.update(cx, |_, cx| cx.emit(event));
}
