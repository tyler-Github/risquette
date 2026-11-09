use serde::Deserialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The JSON error envelope Discuit returns: `{"HTTPStatus": 404, "code": "...", "message": "..."}`.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(rename = "HTTPStatus", alias = "status", default)]
    pub status: u16,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The server answered with a non-2xx status.
    #[error("{} ({status}){}", .api.as_ref().and_then(|a| a.code.clone()).unwrap_or_else(|| "http error".into()), .api.as_ref().and_then(|a| a.message.clone()).map(|m| format!(": {m}")).unwrap_or_default())]
    Status { status: u16, api: Option<ApiError> },
    /// `429 too_many_requests`. Discuit sends no `Retry-After`, so callers back off themselves.
    #[error("rate limited by the server, try again shortly")]
    RateLimited,
    #[error("network error: {0}")]
    Transport(#[from] ureq::Error),
    #[error("could not decode response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),
}

impl Error {
    /// The Discuit error code (e.g. `not_logged_in`), when the server sent one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Status { api, .. } => api.as_ref()?.code.as_deref(),
            Error::RateLimited => Some("too_many_requests"),
            _ => None,
        }
    }

    pub fn is_not_logged_in(&self) -> bool {
        self.code() == Some("not_logged_in")
    }

    /// The server couldn't be reached at all (no connection, the address
    /// didn't resolve, or it timed out), as opposed to answering with an error.
    pub fn is_network(&self) -> bool {
        matches!(self, Error::Transport(ureq::Error::Io(_) | ureq::Error::Timeout(_) | ureq::Error::HostNotFound | ureq::Error::ConnectionFailed))
    }

    /// A short message suitable for showing in the UI.
    pub fn user_message(&self) -> String {
        match self {
            Error::Status { status, api } => {
                let msg = api.as_ref().and_then(|a| a.message.clone()).filter(|m| !m.trim().is_empty());
                match (msg, *status) {
                    (Some(m), _) => m,
                    (None, 401) => "You need to log in to do that.".into(),
                    (None, 403) => "You don't have permission to do that.".into(),
                    (None, 404) => "Not found.".into(),
                    (None, s) => format!("The server returned an error ({s})."),
                }
            }
            Error::RateLimited => "You're doing that too fast. Please wait a moment and try again.".into(),
            Error::Transport(ureq::Error::Timeout(_)) => "The server took too long to answer. Check your internet connection and try again.".into(),
            _ if self.is_network() => "Couldn't reach the server. Check your internet connection.".into(),
            Error::Transport(e) => format!("Network error: {e}"),
            Error::Decode(e) => format!("Unexpected response from the server: {e}"),
            Error::Url(e) => format!("Invalid address: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_errors_are_recognised_and_explained() {
        let offline = Error::Transport(ureq::Error::HostNotFound);
        assert!(offline.is_network());
        assert_eq!(offline.user_message(), "Couldn't reach the server. Check your internet connection.");
        let io = Error::Transport(ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)));
        assert!(io.is_network());
        // The server answering, even with an error, means it can be reached.
        assert!(!Error::Status { status: 500, api: None }.is_network());
        assert!(!Error::RateLimited.is_network());
    }
}
