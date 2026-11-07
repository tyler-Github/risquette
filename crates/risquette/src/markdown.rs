//! Markdown → GPUI elements for post and comment bodies.
//!
//! Supports paragraphs, headings, emphasis, strong, strikethrough, inline
//! code, code blocks, block quotes, ordered/unordered lists, rules, links,
//! bare-URL autolinks and `@user` mentions. Parsed documents are cached.

use crate::nav;
use gpui::{
    div, prelude::*, px, AnyElement, App, ElementId, FontStyle, FontWeight, Global, HighlightStyle,
    InteractiveText, SharedString, StrikethroughStyle, StyledText, UnderlineStyle,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use risquette_theme::ActiveTheme;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Span {
    Strong,
    Emphasis,
    Strike,
    Code,
    Link,
}

#[derive(Debug, Clone, Default)]
struct Inline {
    text: String,
    spans: Vec<(Range<usize>, Span)>,
    links: Vec<(Range<usize>, String)>,
}

impl Inline {
    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[derive(Debug, Clone)]
enum Block {
    Paragraph(Inline),
    Heading(u8, Inline),
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Vec<Block>> },
    Code(String),
    Rule,
}

/// Parse markdown into blocks.
/// Remove HTML comments (`<!-- bot:opt-in -->` and the like), which the web client hides.
fn strip_html_comments(src: &str) -> std::borrow::Cow<'_, str> {
    if !src.contains("<!--") {
        return src.into();
    }
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(end) => rest = &rest[start + 4 + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.into()
}

fn parse(src: &str) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let src = strip_html_comments(src);
    let parser = Parser::new_ext(&src, opts);

    // A stack of open containers; each collects child blocks.
    enum Frame {
        Root(Vec<Block>),
        Quote(Vec<Block>),
        List { start: Option<u64>, items: Vec<Vec<Block>> },
        Item(Vec<Block>),
    }
    let mut stack: Vec<Frame> = vec![Frame::Root(Vec::new())];
    let mut inline: Option<(Option<u8>, Inline)> = None;
    let mut open_spans: Vec<(usize, Span)> = Vec::new();
    let mut open_links: Vec<(usize, String)> = Vec::new();
    let mut code: Option<String> = None;

    fn push_block(stack: &mut [Frame], b: Block) {
        match stack.last_mut() {
            Some(Frame::Root(v) | Frame::Quote(v) | Frame::Item(v)) => v.push(b),
            Some(Frame::List { items, .. }) => {
                if items.is_empty() {
                    items.push(Vec::new());
                }
                items.last_mut().unwrap().push(b);
            }
            None => {}
        }
    }

    // Tight list items contain bare text with no paragraph; open one implicitly.
    fn ensure_inline(inline: &mut Option<(Option<u8>, Inline)>) -> &mut Inline {
        &mut inline.get_or_insert_with(|| (None, Inline::default())).1
    }

    fn flush_inline(stack: &mut [Frame], inline: &mut Option<(Option<u8>, Inline)>) {
        if let Some((level, i)) = inline.take() {
            if !i.is_empty() {
                push_block(stack, match level {
                    Some(l) => Block::Heading(l, i),
                    None => Block::Paragraph(i),
                });
            }
        }
    }

    for event in parser {
        if let Some(c) = code.as_mut() {
            match event {
                Event::Text(t) => c.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let text = code.take().unwrap();
                    push_block(&mut stack, Block::Code(text.trim_end_matches('\n').to_string()));
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Paragraph) => {
                flush_inline(&mut stack, &mut inline);
                inline = Some((None, Inline::default()));
            }
            Event::End(TagEnd::Paragraph) => flush_inline(&mut stack, &mut inline),
            Event::Start(Tag::Heading { level, .. }) => {
                flush_inline(&mut stack, &mut inline);
                let l = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    _ => 4,
                };
                inline = Some((Some(l), Inline::default()));
            }
            Event::End(TagEnd::Heading(_)) => flush_inline(&mut stack, &mut inline),
            Event::Start(Tag::BlockQuote(_)) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::Quote(Vec::new()));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush_inline(&mut stack, &mut inline);
                if let Some(Frame::Quote(v)) = stack.pop() {
                    push_block(&mut stack, Block::Quote(v));
                }
            }
            Event::Start(Tag::List(start)) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::List { start, items: Vec::new() });
            }
            Event::End(TagEnd::List(_)) => {
                flush_inline(&mut stack, &mut inline);
                if let Some(Frame::List { start, items }) = stack.pop() {
                    push_block(&mut stack, Block::List { start, items });
                }
            }
            Event::Start(Tag::Item) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::Item(Vec::new()));
            }
            Event::End(TagEnd::Item) => {
                flush_inline(&mut stack, &mut inline);
                if let Some(Frame::Item(v)) = stack.pop() {
                    if let Some(Frame::List { items, .. }) = stack.last_mut() {
                        items.push(v);
                    }
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush_inline(&mut stack, &mut inline);
                code = Some(String::new());
            }
            Event::Start(Tag::Emphasis) => open_spans.push((ensure_inline(&mut inline).text.len(), Span::Emphasis)),
            Event::Start(Tag::Strong) => open_spans.push((ensure_inline(&mut inline).text.len(), Span::Strong)),
            Event::Start(Tag::Strikethrough) => open_spans.push((ensure_inline(&mut inline).text.len(), Span::Strike)),
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) => {
                if let Some((start, kind)) = open_spans.pop() {
                    let i = ensure_inline(&mut inline);
                    i.spans.push((start..i.text.len(), kind));
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                open_links.push((ensure_inline(&mut inline).text.len(), dest_url.to_string()));
            }
            Event::End(TagEnd::Link) => {
                if let Some((start, url)) = open_links.pop() {
                    let i = ensure_inline(&mut inline);
                    let range = start..i.text.len();
                    i.spans.push((range.clone(), Span::Link));
                    i.links.push((range, url));
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                // Images aren't embedded; show them as links.
                open_links.push((ensure_inline(&mut inline).text.len(), dest_url.to_string()));
            }
            Event::End(TagEnd::Image) => {
                if let Some((start, url)) = open_links.pop() {
                    let i = ensure_inline(&mut inline);
                    if i.text.len() == start {
                        i.text.push_str("[image]");
                    }
                    let range = start..i.text.len();
                    i.spans.push((range.clone(), Span::Link));
                    i.links.push((range, url));
                }
            }
            Event::Text(t) => {
                let in_link = !open_links.is_empty();
                let i = ensure_inline(&mut inline);
                if in_link {
                    i.text.push_str(&t);
                } else {
                    autolink(i, &t);
                }
            }
            Event::Code(t) => {
                let i = ensure_inline(&mut inline);
                let start = i.text.len();
                i.text.push_str(&t);
                i.spans.push((start..i.text.len(), Span::Code));
            }
            Event::SoftBreak => ensure_inline(&mut inline).text.push(' '),
            Event::HardBreak => ensure_inline(&mut inline).text.push('\n'),
            Event::Rule => {
                flush_inline(&mut stack, &mut inline);
                push_block(&mut stack, Block::Rule);
            }
            Event::Html(h) | Event::InlineHtml(h) => ensure_inline(&mut inline).text.push_str(&h),
            _ => {}
        }
    }
    flush_inline(&mut stack, &mut inline);
    while stack.len() > 1 {
        match stack.pop().unwrap() {
            Frame::Quote(v) => push_block(&mut stack, Block::Quote(v)),
            Frame::Item(v) => push_block(&mut stack, Block::Quote(v)),
            Frame::List { start, items } => push_block(&mut stack, Block::List { start, items }),
            Frame::Root(_) => {}
        }
    }
    match stack.pop() {
        Some(Frame::Root(v)) => v,
        _ => Vec::new(),
    }
}

/// Append text, turning bare URLs and `@user` mentions into links.
fn autolink(i: &mut Inline, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let url_at = ["https://", "http://"].iter().filter_map(|p| rest.find(p)).min();
        let mention_at = rest
            .char_indices()
            .find(|&(ix, c)| {
                c == '@'
                    && (ix == 0 || rest[..ix].chars().last().is_some_and(|p| p.is_whitespace() || p == '('))
                    && rest[ix + 1..].chars().next().is_some_and(|n| n.is_ascii_alphanumeric() || n == '_')
            })
            .map(|(ix, _)| ix);
        let next = match (url_at, mention_at) {
            (Some(u), Some(m)) => Some(u.min(m)),
            (a, b) => a.or(b),
        };
        let Some(at) = next else {
            i.text.push_str(rest);
            break;
        };
        i.text.push_str(&rest[..at]);
        let tail = &rest[at..];
        let is_mention = tail.starts_with('@');
        let end = if is_mention {
            1 + tail[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(tail.len() - 1)
        } else {
            let mut e = tail.find(char::is_whitespace).unwrap_or(tail.len());
            // Trailing punctuation is usually not part of the URL.
            while e > 0 && tail[..e].ends_with(['.', ',', ')', '!', '?', ';', ':', '"', '\'']) {
                e -= 1;
            }
            e
        };
        let token = &tail[..end];
        let start = i.text.len();
        i.text.push_str(token);
        let range = start..i.text.len();
        i.spans.push((range.clone(), Span::Link));
        let target = if is_mention { format!("/@{}", &token[1..]) } else { token.to_string() };
        i.links.push((range, target));
        rest = &tail[end..];
    }
}

#[derive(Default)]
struct MarkdownCache(HashMap<u64, Arc<Vec<Block>>>);

impl Global for MarkdownCache {}

fn parsed(src: &str, cx: &mut App) -> Arc<Vec<Block>> {
    let mut h = DefaultHasher::new();
    src.hash(&mut h);
    let key = h.finish();
    let cache = cx.default_global::<MarkdownCache>();
    if let Some(b) = cache.0.get(&key) {
        return b.clone();
    }
    if cache.0.len() > 4000 {
        cache.0.clear();
    }
    let blocks = Arc::new(parse(src));
    cache.0.insert(key, blocks.clone());
    blocks
}

/// Render markdown. `id` must be unique among siblings (used for link hit-testing).
pub fn render(id: impl Into<SharedString>, src: &str, cx: &mut App) -> AnyElement {
    let id: SharedString = id.into();
    let blocks = parsed(src, cx);
    let mut counter = 0usize;
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .min_w_0()
        .children(blocks.iter().map(|b| render_block(&id, b, &mut counter, cx)))
        .into_any_element()
}

fn render_block(id: &SharedString, block: &Block, counter: &mut usize, cx: &App) -> AnyElement {
    let t = cx.theme();
    match block {
        Block::Paragraph(i) => render_inline(id, i, counter, cx).into_any_element(),
        Block::Heading(level, i) => {
            let size = match level {
                1 => t.fs.xxl - px(4.),
                2 => t.fs.xl + px(3.),
                _ => t.fs.xl,
            };
            div().text_size(size).font_weight(FontWeight::BOLD).child(render_inline(id, i, counter, cx)).into_any_element()
        }
        Block::Quote(children) => div()
            .pl(px(12.))
            .border_l_2()
            .border_color(t.colors.comment_line)
            .text_color(t.colors.text_light)
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(children.iter().map(|b| render_block(id, b, counter, cx)))
            .into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(items.iter().enumerate().map(|(n, item)| {
                let marker = match start {
                    Some(s) => format!("{}.", s + n as u64),
                    None => "•".into(),
                };
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_none().min_w(px(14.)).text_color(t.colors.text_light).child(marker))
                    .child(div().flex_1().min_w_0().flex().flex_col().gap(px(4.)).children(item.iter().map(|b| render_block(id, b, counter, cx))))
            }))
            .into_any_element(),
        Block::Code(code) => div()
            .p(px(10.))
            .rounded(t.metrics.radius)
            .bg(t.colors.input)
            .font_family(if cfg!(target_os = "windows") { "Consolas" } else { "Menlo" })
            .text_size(t.fs.s)
            .child(code.clone())
            .into_any_element(),
        Block::Rule => div().h(px(1.)).w_full().bg(t.colors.separator).into_any_element(),
    }
}

fn render_inline(id: &SharedString, i: &Inline, counter: &mut usize, cx: &App) -> AnyElement {
    let t = cx.theme();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = i
        .spans
        .iter()
        .filter(|(r, _)| !r.is_empty())
        .map(|(r, kind)| {
            let style = match kind {
                Span::Strong => HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() },
                Span::Emphasis => HighlightStyle { font_style: Some(FontStyle::Italic), ..Default::default() },
                Span::Strike => HighlightStyle {
                    strikethrough: Some(StrikethroughStyle { thickness: px(1.), color: None }),
                    ..Default::default()
                },
                Span::Code => HighlightStyle { background_color: Some(t.colors.input), ..Default::default() },
                Span::Link => HighlightStyle {
                    color: Some(t.colors.link),
                    underline: Some(UnderlineStyle { thickness: px(1.), color: None, wavy: false }),
                    ..Default::default()
                },
            };
            (r.clone(), style)
        })
        .collect();
    // StyledText wants non-overlapping, sorted highlights; keep the first of any overlap.
    let mut sorted = highlights;
    sorted.sort_by_key(|(r, _)| r.start);
    let mut clean: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for (r, s) in sorted {
        match clean.last_mut() {
            Some((last, ls)) if r.start < last.end => {
                // Merge nested styles into the overlapping region by splitting.
                if r.end <= last.end {
                    let merged = merge(*ls, s);
                    let (a, b, c) = (last.start..r.start, r.clone(), r.end..last.end);
                    let outer = *ls;
                    clean.pop();
                    for (rr, ss) in [(a, outer), (b, merged), (c, outer)] {
                        if !rr.is_empty() {
                            clean.push((rr, ss));
                        }
                    }
                }
            }
            _ => clean.push((r, s)),
        }
    }
    let styled = StyledText::new(SharedString::from(i.text.clone())).with_highlights(clean);
    if i.links.is_empty() {
        return styled.into_any_element();
    }
    *counter += 1;
    let element_id = ElementId::Name(format!("{id}-md-{counter}").into());
    let ranges: Vec<Range<usize>> = i.links.iter().map(|(r, _)| r.clone()).collect();
    let urls: Vec<String> = i.links.iter().map(|(_, u)| u.clone()).collect();
    InteractiveText::new(element_id, styled)
        .on_click(ranges, move |ix, _window, cx| {
            if let Some(url) = urls.get(ix) {
                nav::open_link(url, cx);
            }
        })
        .into_any_element()
}

fn merge(a: HighlightStyle, b: HighlightStyle) -> HighlightStyle {
    HighlightStyle {
        color: b.color.or(a.color),
        font_weight: b.font_weight.or(a.font_weight),
        font_style: b.font_style.or(a.font_style),
        background_color: b.background_color.or(a.background_color),
        underline: b.underline.or(a.underline),
        strikethrough: b.strikethrough.or(a.strikethrough),
        fade_out: b.fade_out.or(a.fade_out),
    }
}

/// Plain-text preview of markdown (for feed excerpts).
pub fn plain_preview(src: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for b in parse(src) {
        let text = match b {
            Block::Paragraph(i) | Block::Heading(_, i) => i.text,
            Block::Quote(_) | Block::List { .. } => continue,
            Block::Code(c) => c,
            Block::Rule => continue,
        };
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(text.trim());
        if out.chars().count() >= max_chars {
            break;
        }
    }
    if out.is_empty() {
        out = src.chars().take(max_chars).collect();
    }
    let mut s: String = out.chars().take(max_chars).collect();
    if out.chars().count() > max_chars {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_html_comments() {
        assert_eq!(strip_html_comments("Hello <!-- altbot:opt-in --> world"), "Hello  world");
        assert_eq!(strip_html_comments("a<!-- open"), "a");
        assert_eq!(plain_preview("Memes! <!-- altbot:opt-in -->", 50), "Memes!");
    }

    fn para(b: &Block) -> &Inline {
        match b {
            Block::Paragraph(i) => i,
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn inline_spans_and_links() {
        let blocks = parse("Hello **bold** and [link](https://a.b) and `code`");
        let i = para(&blocks[0]);
        assert_eq!(i.text, "Hello bold and link and code");
        assert!(i.spans.contains(&(6..10, Span::Strong)));
        assert_eq!(i.links, vec![(15..19, "https://a.b".to_string())]);
        assert!(i.spans.contains(&(24..28, Span::Code)));
    }

    #[test]
    fn autolinks_urls_and_mentions() {
        let blocks = parse("see https://x.org/a. and ask @bob_1!");
        let i = para(&blocks[0]);
        assert_eq!(i.links[0].1, "https://x.org/a");
        assert_eq!(i.links[1].1, "/@bob_1");
        // Emails are not mentions.
        let i2 = parse("mail a@b.com");
        assert!(para(&i2[0]).links.is_empty());
    }

    #[test]
    fn blocks() {
        let blocks = parse("# Title\n\n> quoted\n\n- a\n- b\n\n1. one\n\n```\ncode\n```\n\n---");
        assert!(matches!(&blocks[0], Block::Heading(1, i) if i.text == "Title"));
        assert!(matches!(&blocks[1], Block::Quote(v) if v.len() == 1));
        assert!(matches!(&blocks[2], Block::List { start: None, items } if items.len() == 2));
        assert!(matches!(&blocks[3], Block::List { start: Some(1), .. }));
        assert!(matches!(&blocks[4], Block::Code(c) if c == "code"));
        assert!(matches!(&blocks[5], Block::Rule));
    }

    #[test]
    fn preview() {
        assert_eq!(plain_preview("**Hi** there\n\nsecond", 100), "Hi there second");
        assert_eq!(plain_preview("abcdef", 3), "abc…");
    }
}
