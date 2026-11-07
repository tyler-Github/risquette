//! Recognise embeddable links: video providers (played in a pop-out player
//! window) and direct images/GIFs (shown inline).

use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum Embed {
    Video {
        provider: &'static str,
        /// What the player window loads.
        player: Player,
        /// A thumbnail URL, when the provider has a predictable one.
        thumbnail: Option<String>,
    },
    Image { url: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Player {
    /// An embeddable player page.
    Page(String),
    /// A direct video file, played with an HTML `<video>` element.
    File(String),
}

impl Player {
    pub fn url(&self) -> &str {
        match self {
            Player::Page(u) | Player::File(u) => u,
        }
    }
}

const VIDEO_EXT: [&str; 4] = [".mp4", ".webm", ".mov", ".m4v"];
const IMAGE_EXT: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif"];

pub fn detect(link: &str) -> Option<Embed> {
    let url = Url::parse(link.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.trim_start_matches("www.").trim_start_matches("m.").to_ascii_lowercase();
    let path = url.path().to_string();
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let query = |k: &str| url.query_pairs().find(|(key, _)| key == k).map(|(_, v)| v.to_string());

    // YouTube
    let yt_id = match host.as_str() {
        "youtu.be" => segs.first().map(|s| s.to_string()),
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => match segs.as_slice() {
            ["watch"] => query("v"),
            ["shorts" | "live" | "embed" | "v", id, ..] => Some(id.to_string()),
            _ => None,
        },
        _ => None,
    };
    if let Some(id) = yt_id.filter(|id| id.len() >= 6 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
        let start = query("t").or(query("start")).and_then(|t| parse_seconds(&t));
        let mut player = format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1&rel=0");
        if let Some(s) = start {
            player.push_str(&format!("&start={s}"));
        }
        return Some(Embed::Video { provider: "YouTube", player: Player::Page(player), thumbnail: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")) });
    }

    match (host.as_str(), segs.as_slice()) {
        ("vimeo.com", [id, ..]) if id.chars().all(|c| c.is_ascii_digit()) => {
            return Some(Embed::Video { provider: "Vimeo", player: Player::Page(format!("https://player.vimeo.com/video/{id}?autoplay=1")), thumbnail: None });
        }
        ("player.vimeo.com", ["video", id, ..]) => {
            return Some(Embed::Video { provider: "Vimeo", player: Player::Page(format!("https://player.vimeo.com/video/{id}?autoplay=1")), thumbnail: None });
        }
        ("streamable.com", [id]) | ("streamable.com", ["e", id]) => {
            return Some(Embed::Video { provider: "Streamable", player: Player::Page(format!("https://streamable.com/e/{id}?autoplay=1")), thumbnail: None });
        }
        ("dailymotion.com", ["video", id, ..]) | ("dai.ly", [id, ..]) => {
            let id = id.split('_').next().unwrap_or(id);
            return Some(Embed::Video {
                provider: "Dailymotion",
                player: Player::Page(format!("https://www.dailymotion.com/embed/video/{id}?autoplay=1")),
                thumbnail: Some(format!("https://www.dailymotion.com/thumbnail/video/{id}")),
            });
        }
        _ => {}
    }

    let lower = path.to_ascii_lowercase();
    // Imgur's .gifv pages are really MP4s.
    if host == "i.imgur.com" && lower.ends_with(".gifv") {
        let mp4 = link.trim().trim_end_matches(".gifv").to_string() + ".mp4";
        return Some(Embed::Video { provider: "Imgur", player: Player::File(mp4), thumbnail: None });
    }
    if VIDEO_EXT.iter().any(|e| lower.ends_with(e)) {
        return Some(Embed::Video { provider: "Video", player: Player::File(link.trim().to_string()), thumbnail: None });
    }
    if IMAGE_EXT.iter().any(|e| lower.ends_with(e)) {
        return Some(Embed::Image { url: link.trim().to_string() });
    }
    None
}

/// The first embeddable link in a markdown/plain text body.
pub fn first_in_text(text: &str) -> Option<(String, Embed)> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '<' || c == '>' || c == '[' || c == ']')
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .find_map(|w| {
            let w = w.trim_end_matches(['.', ',', '!', '?', ';', ':', '"', '\'']);
            detect(w).map(|e| (w.to_string(), e))
        })
}

/// `90`, `90s`, `1m30s`, `1h2m3s` → seconds.
fn parse_seconds(t: &str) -> Option<u32> {
    if let Ok(n) = t.parse() {
        return Some(n);
    }
    let (mut total, mut num) = (0u32, String::new());
    for c in t.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: u32 = num.parse().ok()?;
            num.clear();
            total += match c {
                'h' => n * 3600,
                'm' => n * 60,
                's' => n,
                _ => return None,
            };
        }
    }
    if !num.is_empty() {
        total += num.parse::<u32>().ok()?;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yt(link: &str) -> Option<String> {
        match detect(link)? {
            Embed::Video { provider: "YouTube", player, .. } => Some(player.url().to_string()),
            _ => None,
        }
    }

    #[test]
    fn youtube_variants() {
        let base = "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1&rel=0";
        assert_eq!(yt("https://www.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some(base));
        assert_eq!(yt("https://youtu.be/dQw4w9WgXcQ").as_deref(), Some(base));
        assert_eq!(yt("https://m.youtube.com/watch?v=dQw4w9WgXcQ&feature=share").as_deref(), Some(base));
        assert_eq!(yt("https://youtube.com/shorts/dQw4w9WgXcQ").as_deref(), Some(base));
        assert_eq!(yt("https://youtu.be/dQw4w9WgXcQ?t=1m30s").as_deref(), Some(&*format!("{base}&start=90")));
        assert_eq!(yt("https://www.youtube.com/@channel"), None);
        match detect("https://youtu.be/dQw4w9WgXcQ").unwrap() {
            Embed::Video { thumbnail, .. } => assert_eq!(thumbnail.as_deref(), Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")),
            _ => panic!(),
        }
    }

    #[test]
    fn other_providers_and_media() {
        assert!(matches!(detect("https://vimeo.com/123456"), Some(Embed::Video { provider: "Vimeo", .. })));
        assert!(matches!(detect("https://streamable.com/abc12"), Some(Embed::Video { provider: "Streamable", .. })));
        assert!(matches!(detect("https://www.dailymotion.com/video/x8abc_title"), Some(Embed::Video { provider: "Dailymotion", .. })));
        assert!(matches!(detect("https://example.com/clip.MP4"), Some(Embed::Video { player: Player::File(_), .. })));
        assert!(matches!(detect("https://i.imgur.com/abc.gifv"), Some(Embed::Video { player: Player::File(u), .. }) if u.ends_with(".mp4")));
        assert_eq!(detect("https://i.imgur.com/abc.png"), Some(Embed::Image { url: "https://i.imgur.com/abc.png".into() }));
        assert_eq!(detect("https://example.com/article"), None);
        assert_eq!(detect("ftp://example.com/a.png"), None);
    }

    #[test]
    fn finds_link_in_text() {
        let (link, _) = first_in_text("Check this out (https://youtu.be/dQw4w9WgXcQ).").unwrap();
        assert_eq!(link, "https://youtu.be/dQw4w9WgXcQ");
        assert!(first_in_text("no links here").is_none());
    }
}
