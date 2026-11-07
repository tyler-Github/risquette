//! A blocking client for the [Discuit](https://discuit.org) HTTP API, covering
//! every endpoint of the Discuit API.
//!
//! The client is synchronous on purpose: GPUI runs its own (smol-based)
//! executor, so the UI calls these methods from a background task instead of
//! needing a tokio runtime.
//!
//! Methods that change server state are marked **Mutating** in their docs.

mod client;
mod error;
mod html;
pub mod models;
pub mod tree;

pub use client::{Client, CommentAction, DEFAULT_BASE_URL, FeedQuery, NewPost, NewPostImage, PostAction, mime_for};
pub use error::{ApiError, Error, Result};
pub use models::*;
