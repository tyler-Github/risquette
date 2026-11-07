//! §1 Session & bootstrap.

use super::{Client, q};
use crate::{Result, models::*};
use serde_json::json;

impl Client {
    /// `GET /api/_initial` — bootstrap data, including the logged-in user (if any).
    /// Also establishes the `SID` and `csrftoken` cookies.
    pub fn initial(&self) -> Result<Initial> {
        self.http_get("/api/_initial", Vec::new())
    }

    /// `GET /api/_user` — the logged-in user with `moddingList`. Fails with `not_logged_in` otherwise.
    pub fn me(&self) -> Result<User> {
        self.http_get("/api/_user", Vec::new())
    }

    /// `POST /api/_login`. **Mutating.** Attaches the user to the session cookie.
    pub fn login(&self, username: &str, password: &str) -> Result<User> {
        self.http_post("/api/_login", Vec::new(), json!({ "username": username, "password": password }))
    }

    /// `POST /api/_login?action=logout`. **Mutating.** Destroys the session, then forgets local cookies.
    pub fn logout(&self) -> Result<()> {
        let result: Result<serde_json::Value> = self.http_post("/api/_login", q([("action", Some("logout".into()))]), json!({}));
        self.clear_cookies();
        result.map(|_| ())
    }

    /// `POST /api/_signup`. **Mutating.** `captcha_token` is the hCaptcha response, when the site requires one.
    pub fn signup(&self, username: &str, email: &str, password: &str, captcha_token: Option<&str>) -> Result<User> {
        self.http_post(
            "/api/_signup",
            Vec::new(),
            json!({ "username": username, "email": email, "password": password, "captchaToken": captcha_token.unwrap_or("") }),
        )
    }
}
