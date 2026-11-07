//! Minimal HTML → markdown conversion for server-rendered notification text
//! (`<b>`, `<strong>`, `<i>`, `<em>`, `<a href>`, `<br>`, `<p>`, entities).

pub fn to_markdown(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut link_stack: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&decode_entities(&rest[..lt]));
        let Some(gt) = rest[lt..].find('>') else {
            out.push_str(&decode_entities(&rest[lt..]));
            rest = "";
            break;
        };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("b" | "strong", _) => out.push_str("**"),
            ("i" | "em", _) => out.push('*'),
            ("br", _) => out.push('\n'),
            ("p" | "div", true) => out.push_str("\n\n"),
            ("a", false) => {
                link_stack.push(attr(tag, "href").unwrap_or_default());
                out.push('[');
            }
            ("a", true) => {
                let href = link_stack.pop().unwrap_or_default();
                out.push_str(&format!("]({href})"));
            }
            _ => {}
        }
    }
    out.push_str(&decode_entities(rest));
    out.trim().to_string()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let i = lower.find(&format!("{name}="))? + name.len() + 1;
    let v = &tag[i..];
    let (q, v) = match v.chars().next()? {
        c @ ('"' | '\'') => (c, &v[1..]),
        _ => (' ', v),
    };
    let end = v.find(q).unwrap_or(v.len());
    Some(decode_entities(&v[..end]))
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_tags() {
        assert_eq!(to_markdown("<b>alice</b> replied to your post <i>Hello</i>"), "**alice** replied to your post *Hello*");
        assert_eq!(to_markdown(r#"See <a href="/golang/post/x">this</a> &amp; more"#), "See [this](/golang/post/x) & more");
        assert_eq!(to_markdown("line<br>two"), "line\ntwo");
        assert_eq!(to_markdown("plain text"), "plain text");
    }
}
