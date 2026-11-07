//! HTTP plumbing shared by every endpoint group.
//!
//! Endpoints live in the sibling modules (`session`, `posts`, …), each adding
//! methods to [`Client`].

mod admin;

mod communities;
mod lists;
mod mutes;
mod notifications;
mod posts;
mod session;
mod settings;
mod users;

pub use posts::{CommentAction, FeedQuery, NewPost, NewPostImage, PostAction};

use crate::{Error, Result, error::ApiError};

use serde::{Serialize, de::DeserializeOwned};
use std::{collections::BTreeMap, sync::Mutex, time::Duration};
use url::Url;

pub const DEFAULT_BASE_URL: &str = "https://discuit.org";
const CSRF_COOKIE: &str = "csrftoken";
const MAX_BODY: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Post,
    Put,
    Delete,
}

/// Body of a request.
pub(crate) enum Body {
    None,
    Json(serde_json::Value),
    Multipart { boundary: String, bytes: Vec<u8> },
}

/// Query parameters, built with [`q`].
pub(crate) type Query<'a> = Vec<(&'a str, String)>;

/// Build a query list, skipping `None` values.
pub(crate) fn q<'a>(pairs: impl IntoIterator<Item = (&'a str, Option<String>)>) -> Query<'a> {
    pairs.into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect()
}

/// Blocking Discuit client. Share it behind an `Arc`; all methods take `&self`.
///
/// Discuit authenticates with an `SID` cookie and requires the `csrftoken`
/// cookie to be echoed in an `X-Csrf-Token` header on every non-GET request.
/// Both cookies are set on any request, so call a GET (e.g. [`Client::initial`])
/// before the first write.
pub struct Client {
    agent: ureq::Agent,
    base: Url,
    cookies: Mutex<BTreeMap<String, String>>,
}

impl Client {
    pub fn new(base_url: &str) -> Result<Self> {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(concat!("Risquette/", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(60)))
            .build();
        Ok(Self { agent: ureq::Agent::new_with_config(config), base: Url::parse(base_url)?, cookies: Mutex::new(BTreeMap::new()) })
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    /// Resolve a server-relative path such as `/images/abc.jpeg?sig=..`.
    pub fn absolute_url(&self, path_or_url: &str) -> Result<Url> {
        Ok(self.base.join(path_or_url)?)
    }

    /// Web URL of a post, for "open in browser" / "copy link".
    pub fn post_web_url(&self, community: &str, public_id: &str) -> String {
        self.web_url(&format!("/{}/post/{}", seg(community), seg(public_id)))
    }

    /// Absolute web URL for a site path such as `/@alice` or `/golang`.
    pub fn web_url(&self, path: &str) -> String {
        self.base.join(path).map(|u| u.to_string()).unwrap_or_else(|_| self.base.to_string())
    }

    /// Serialized cookies, for persisting a session between launches.
    pub fn export_cookies(&self) -> String {
        let jar = self.cookies.lock().unwrap();
        jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    pub fn import_cookies(&self, header: &str) {
        let mut jar = self.cookies.lock().unwrap();
        for pair in header.split(';') {
            if let Some((k, v)) = pair.trim().split_once('=') {
                jar.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    pub fn clear_cookies(&self) {
        self.cookies.lock().unwrap().clear();
    }

    /// Download raw bytes (images) from a server-relative or absolute URL.
    pub fn fetch_bytes(&self, path_or_url: &str) -> Result<Vec<u8>> {
        let url = self.absolute_url(path_or_url)?;
        self.send_raw(Method::Get, url, Body::None)
    }

    // ---------------------------------------------------------------------
    // Typed helpers used by the endpoint modules
    // ---------------------------------------------------------------------

    pub(crate) fn http_get<T: DeserializeOwned>(&self, path: &str, query: Query) -> Result<T> {
        self.request(Method::Get, path, query, Body::None)
    }

    pub(crate) fn http_post<T: DeserializeOwned>(&self, path: &str, query: Query, body: impl Serialize) -> Result<T> {
        self.request(Method::Post, path, query, Body::Json(serde_json::to_value(body)?))
    }

    pub(crate) fn http_put<T: DeserializeOwned>(&self, path: &str, query: Query, body: impl Serialize) -> Result<T> {
        self.request(Method::Put, path, query, Body::Json(serde_json::to_value(body)?))
    }

    pub(crate) fn http_delete<T: DeserializeOwned>(&self, path: &str, query: Query) -> Result<T> {
        self.request(Method::Delete, path, query, Body::None)
    }

    pub(crate) fn http_delete_with<T: DeserializeOwned>(&self, path: &str, query: Query, body: impl Serialize) -> Result<T> {
        self.request(Method::Delete, path, query, Body::Json(serde_json::to_value(body)?))
    }

    /// `multipart/form-data` upload with the file in the `image` part.
    pub(crate) fn http_upload<T: DeserializeOwned>(&self, path: &str, file_name: &str, bytes: &[u8]) -> Result<T> {
        let (boundary, body) = multipart_image(file_name, bytes);
        self.request(Method::Post, path, Vec::new(), Body::Multipart { boundary, bytes: body })
    }

    fn url(&self, path: &str, query: &Query) -> Result<Url> {
        let mut url = self.base.join(path)?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(url)
    }

    fn request<T: DeserializeOwned>(&self, method: Method, path: &str, query: Query, body: Body) -> Result<T> {
        let url = self.url(path, &query)?;
        let bytes = self.send_raw(method, url, body)?;
        // Some endpoints answer `200` with an empty body; treat that as `null`.
        let bytes: &[u8] = if bytes.iter().all(u8::is_ascii_whitespace) { b"null" } else { &bytes };
        Ok(serde_json::from_slice(bytes)?)
    }

    fn send_raw(&self, method: Method, url: Url, body: Body) -> Result<Vec<u8>> {
        // Only send our cookies to our own origin.
        let same_origin = url.origin() == self.base.origin();
        let (cookie_header, csrf) = if same_origin {
            let jar = self.cookies.lock().unwrap();
            let header = jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ");
            (header, jar.get(CSRF_COOKIE).cloned())
        } else {
            (String::new(), None)
        };
        log::debug!("{method:?} {}", redact(&url));
        let u = url.as_str();

        macro_rules! with_headers {
            ($req:expr) => {{
                let mut req = $req;
                if !cookie_header.is_empty() {
                    req = req.header("Cookie", &cookie_header);
                }
                if method != Method::Get {
                    if let Some(t) = &csrf {
                        req = req.header("X-Csrf-Token", t);
                    }
                }
                req
            }};
        }

        let response = match (method, body) {
            (Method::Get, _) => with_headers!(self.agent.get(u)).call()?,
            (Method::Delete, Body::None) => with_headers!(self.agent.delete(u)).call()?,
            (method, body) => {
                let req = match method {
                    Method::Post => self.agent.post(u),
                    Method::Put => self.agent.put(u),
                    _ => self.agent.delete(u).force_send_body(),
                };
                let req = with_headers!(req);
                match body {
                    Body::None => req.header("Content-Type", "application/json").send(&b"{}"[..])?,
                    Body::Json(v) => req.header("Content-Type", "application/json").send(serde_json::to_vec(&v)?)?,
                    Body::Multipart { boundary, bytes } => req.header("Content-Type", format!("multipart/form-data; boundary={boundary}")).send(bytes)?,
                }
            }
        };

        if same_origin {
            self.store_cookies(response.headers());
        }
        let status = response.status().as_u16();
        let bytes = response.into_body().with_config().limit(MAX_BODY).read_to_vec()?;
        match status {
            200..=299 => Ok(bytes),
            429 => Err(Error::RateLimited),
            _ => {
                let api: Option<ApiError> = serde_json::from_slice(&bytes).ok();
                if api.is_none() && status == 403 {
                    // An IP block returns an HTML page instead of JSON.
                    log::warn!("403 without JSON body (possibly IP-blocked)");
                }
                Err(Error::Status { status, api })
            }
        }
    }

    fn store_cookies(&self, headers: &ureq::http::HeaderMap) {
        let mut jar = self.cookies.lock().unwrap();
        for value in headers.get_all("set-cookie") {
            let Ok(s) = value.to_str() else { continue };
            let first = s.split(';').next().unwrap_or("");
            if let Some((k, v)) = first.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                let lower = s.to_ascii_lowercase();
                let expired = lower.contains("max-age=0") || lower.contains("max-age=-") || v.is_empty();
                if expired {
                    jar.remove(k);
                } else {
                    jar.insert(k.to_string(), v.to_string());
                }
            }
        }
    }
}

/// Percent-encode one path segment.
pub(crate) fn seg(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20")
}

/// Hide image signatures in logs.
fn redact(url: &Url) -> String {
    let s = url.as_str();
    match s.find("sig=") {
        Some(i) => format!("{}sig=…", &s[..i]),
        None => s.to_string(),
    }
}

fn multipart_image(file_name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let boundary = format!("----RisquetteBoundary{nanos:x}");
    let mime = mime_for(file_name);
    let safe_name: String = file_name.chars().filter(|c| *c != '"' && *c != '\r' && *c != '\n').collect();
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{safe_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

pub fn mime_for(file_name: &str) -> &'static str {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "heic" => "image/heic",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! A one-shot local HTTP server that records the request it receives and
    //! answers with a canned response. Lets us verify write endpoints (method,
    //! path, headers, body) without touching a real Discuit instance.
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    pub struct Recorded {
        pub request_line: String,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Recorded {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
        }
        pub fn json(&self) -> serde_json::Value {
            serde_json::from_slice(&self.body).unwrap()
        }
    }

    /// Start a server answering `status` + `body` once. Returns its base URL and a receiver for the request.
    pub fn serve(status: u16, body: &'static str, set_cookie: Option<&'static str>) -> (String, mpsc::Receiver<Recorded>) {
        serve_seq(vec![(status, body, set_cookie)])
    }

    /// Answer a sequence of requests with the given responses, in order.
    pub fn serve_seq(responses: Vec<(u16, &'static str, Option<&'static str>)>) -> (String, mpsc::Receiver<Recorded>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for (status, body, set_cookie) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let header_end = loop {
                    let n = stream.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let mut lines = head.split("\r\n");
                let request_line = lines.next().unwrap().to_string();
                let headers: Vec<(String, String)> =
                    lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect();
                let len: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.parse().unwrap()).unwrap_or(0);
                while buf.len() < header_end + len {
                    let n = stream.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                }
                let req_body = buf[header_end..header_end + len].to_vec();
                let cookie = set_cookie.map(|c| format!("Set-Cookie: {c}\r\n")).unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(resp.as_bytes()).unwrap();
                tx.send(Recorded { request_line, headers, body: req_body }).unwrap();
            }
        });
        (format!("http://{addr}"), rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_roundtrip_and_urls() {
        let c = Client::new(DEFAULT_BASE_URL).unwrap();
        c.import_cookies("SID=abc; csrftoken=def");
        assert_eq!(c.export_cookies(), "SID=abc; csrftoken=def");
        assert_eq!(c.absolute_url("/images/a.jpeg?sig=1").unwrap().as_str(), "https://discuit.org/images/a.jpeg?sig=1");
        let u = c.url("/api/posts", &vec![("next", "1.a b".into())]).unwrap();
        assert_eq!(u.as_str(), "https://discuit.org/api/posts?next=1.a+b");
        assert_eq!(seg("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn get_stores_cookies_and_writes_send_csrf() {
        let (base, rx) = test_server::serve(200, "{}", Some("csrftoken=tok123; Path=/"));
        let c = Client::new(&base).unwrap();
        let _: serde_json::Value = c.http_get("/api/_initial", Vec::new()).unwrap();
        let req = rx.recv().unwrap();
        assert!(req.request_line.starts_with("GET /api/_initial"));
        assert_eq!(req.header("X-Csrf-Token"), None, "GETs carry no CSRF header");

        // Same jar, new server: point a client at it with the cookie copied over.
        let (base2, rx2) = test_server::serve(200, r#"{"success":true}"#, None);
        let c2 = Client::new(&base2).unwrap();
        c2.import_cookies(&c.export_cookies());
        let _: serde_json::Value = c2.http_post("/api/hidden_posts", Vec::new(), serde_json::json!({"postId": "p"})).unwrap();
        let req = rx2.recv().unwrap();
        assert!(req.request_line.starts_with("POST /api/hidden_posts"));
        assert_eq!(req.header("X-Csrf-Token"), Some("tok123"));
        assert_eq!(req.header("Cookie"), Some("csrftoken=tok123"));
        assert_eq!(req.json()["postId"], "p");
    }

    #[test]
    fn empty_body_decodes_as_unit_and_errors_parse() {
        let (base, _rx) = test_server::serve(200, "", None);
        let c = Client::new(&base).unwrap();
        let () = c.http_post("/api/community_requests", Vec::new(), serde_json::json!({})).unwrap();

        let (base, _rx) = test_server::serve(409, r#"{"HTTPStatus":409,"code":"user_exists","message":"taken"}"#, None);
        let c = Client::new(&base).unwrap();
        let err = c.http_post::<()>("/api/_signup", Vec::new(), serde_json::json!({})).unwrap_err();
        assert_eq!(err.code(), Some("user_exists"));
        assert_eq!(err.user_message(), "taken");
    }

    #[test]
    fn multipart_shape() {
        let (b, body) = multipart_image("cat.PNG", b"\x89PNGdata");
        let s = String::from_utf8_lossy(&body);
        assert!(s.starts_with(&format!("--{b}\r\n")));
        assert!(s.contains("name=\"image\"; filename=\"cat.PNG\""));
        assert!(s.contains("Content-Type: image/png"));
        assert!(s.ends_with(&format!("\r\n--{b}--\r\n")));
    }
}
