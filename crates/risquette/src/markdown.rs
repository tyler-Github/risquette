//! Markdown → GPUI elements for post and comment bodies.
//!
//! Follows the Discuit web client, which renders GitHub-flavoured markdown
//! (react-markdown with remark-gfm and `skipHtml`): paragraphs, headings 1–6,
//! emphasis, strong, strikethrough, inline code, code blocks, block quotes,
//! ordered/unordered lists, task lists, tables, footnotes, rules, links and
//! bare-URL autolinks. Like the web client, raw HTML is dropped and images show
//! as links to the image. On top of that, `@user` mentions become links.
//! Parsed documents are cached.

use crate::{icons, nav};
use gpui::{
    AnyElement, App, ElementId, FontStyle, FontWeight, Global, HighlightStyle, InteractiveText, SharedString, StrikethroughStyle, StyledText, UnderlineStyle,
    div, prelude::*, px,
};
use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
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
    /// A footnote reference, such as `[1]`.
    Note,
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

#[derive(Debug, Clone, Default)]
struct Item {
    /// `Some(checked)` for a task list item (`- [ ]` / `- [x]`).
    task: Option<bool>,
    blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
enum Block {
    Paragraph(Inline),
    /// Level 1–6.
    Heading(u8, Inline),
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Code(String),
    Rule,
    Table {
        aligns: Vec<Alignment>,
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    /// Footnote definitions, numbered in the order they're first referenced.
    Footnotes(Vec<(usize, Vec<Block>)>),
}

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

/// An open container while parsing; each collects child blocks.
enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List { start: Option<u64>, items: Vec<Item> },
    Item(Item),
    Table { aligns: Vec<Alignment>, head: Vec<Inline>, rows: Vec<Vec<Inline>>, in_head: bool },
    Footnote(String, Vec<Block>),
}

fn push_block(stack: &mut [Frame], b: Block) {
    match stack.last_mut() {
        Some(Frame::Root(v) | Frame::Quote(v) | Frame::Footnote(_, v)) => v.push(b),
        Some(Frame::Item(item)) => item.blocks.push(b),
        Some(Frame::List { items, .. }) => {
            if items.is_empty() {
                items.push(Item::default());
            }
            items.last_mut().unwrap().blocks.push(b);
        }
        Some(Frame::Table { .. }) | None => {}
    }
}

/// Close the innermost container, adding it to its parent.
fn close_frame(stack: &mut Vec<Frame>, notes: &mut Vec<(String, Vec<Block>)>) {
    match stack.pop() {
        Some(Frame::Quote(v)) => push_block(stack, Block::Quote(v)),
        Some(Frame::List { start, items }) => push_block(stack, Block::List { start, items }),
        Some(Frame::Item(item)) => match stack.last_mut() {
            Some(Frame::List { items, .. }) => items.push(item),
            _ => push_block(stack, Block::Quote(item.blocks)),
        },
        Some(Frame::Table { aligns, head, rows, .. }) => push_block(stack, Block::Table { aligns, head, rows }),
        Some(Frame::Footnote(label, v)) => notes.push((label, v)),
        Some(root @ Frame::Root(_)) => stack.push(root),
        None => {}
    }
}

/// Tight list items and table cells contain bare text with no paragraph; open one implicitly.
fn ensure_inline(inline: &mut Option<(Option<u8>, Inline)>) -> &mut Inline {
    &mut inline.get_or_insert_with(|| (None, Inline::default())).1
}

fn flush_inline(stack: &mut [Frame], inline: &mut Option<(Option<u8>, Inline)>) {
    if let Some((level, i)) = inline.take()
        && !i.is_empty()
    {
        push_block(
            stack,
            match level {
                Some(l) => Block::Heading(l, i),
                None => Block::Paragraph(i),
            },
        );
    }
}

/// Parse markdown into blocks.
fn parse(src: &str) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let src = strip_html_comments(src);
    let parser = Parser::new_ext(&src, opts);

    let mut stack: Vec<Frame> = vec![Frame::Root(Vec::new())];
    let mut inline: Option<(Option<u8>, Inline)> = None;
    let mut open_spans: Vec<(usize, Span)> = Vec::new();
    let mut open_links: Vec<(usize, String)> = Vec::new();
    let mut code: Option<String> = None;
    // Inside an image: its alt text is replaced by the image's address.
    let mut image_depth = 0usize;
    let mut notes: Vec<(String, Vec<Block>)> = Vec::new();
    let mut note_numbers: Vec<String> = Vec::new();

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
        if image_depth > 0 && !matches!(event, Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image)) {
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
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                inline = Some((Some(l), Inline::default()));
            }
            Event::End(TagEnd::Heading(_)) => flush_inline(&mut stack, &mut inline),
            Event::Start(Tag::BlockQuote(_)) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::Quote(Vec::new()));
            }
            Event::Start(Tag::List(start)) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::List { start, items: Vec::new() });
            }
            Event::Start(Tag::Item) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::Item(Item::default()));
            }
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item(item)) = stack.iter_mut().rev().find(|f| matches!(f, Frame::Item(_))) {
                    item.task = Some(checked);
                }
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::Footnote(label.to_string(), Vec::new()));
            }
            Event::End(TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition) => {
                flush_inline(&mut stack, &mut inline);
                close_frame(&mut stack, &mut notes);
            }
            Event::Start(Tag::Table(aligns)) => {
                flush_inline(&mut stack, &mut inline);
                stack.push(Frame::Table { aligns, head: Vec::new(), rows: Vec::new(), in_head: false });
            }
            Event::Start(Tag::TableHead) => {
                if let Some(Frame::Table { in_head, .. }) = stack.last_mut() {
                    *in_head = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(Frame::Table { in_head, .. }) = stack.last_mut() {
                    *in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(Frame::Table { rows, .. }) = stack.last_mut() {
                    rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => inline = Some((None, Inline::default())),
            Event::End(TagEnd::TableCell) => {
                let cell = inline.take().map(|(_, i)| i).unwrap_or_default();
                if let Some(Frame::Table { head, rows, in_head, .. }) = stack.last_mut() {
                    if *in_head {
                        head.push(cell);
                    } else if let Some(row) = rows.last_mut() {
                        row.push(cell);
                    }
                }
            }
            Event::End(TagEnd::Table) => close_frame(&mut stack, &mut notes),
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
                // Images aren't embedded: like the web client, show the image's address as a link.
                image_depth += 1;
                if image_depth == 1 {
                    let i = ensure_inline(&mut inline);
                    let start = i.text.len();
                    i.text.push_str(&dest_url);
                    let range = start..i.text.len();
                    i.spans.push((range.clone(), Span::Link));
                    i.links.push((range, dest_url.to_string()));
                }
            }
            Event::End(TagEnd::Image) => image_depth = image_depth.saturating_sub(1),
            Event::FootnoteReference(label) => {
                let n = match note_numbers.iter().position(|l| *l == *label) {
                    Some(ix) => ix + 1,
                    None => {
                        note_numbers.push(label.to_string());
                        note_numbers.len()
                    }
                };
                let i = ensure_inline(&mut inline);
                let start = i.text.len();
                i.text.push_str(&format!("[{n}]"));
                i.spans.push((start..i.text.len(), Span::Note));
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
            // Raw HTML is dropped, as on the web client; text between tags still shows.
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }
    flush_inline(&mut stack, &mut inline);
    while stack.len() > 1 {
        close_frame(&mut stack, &mut notes);
    }
    let mut blocks = match stack.pop() {
        Some(Frame::Root(v)) => v,
        _ => Vec::new(),
    };
    // Footnotes go at the end, numbered by first reference; unreferenced ones are dropped.
    let mut numbered: Vec<(usize, Vec<Block>)> =
        notes.into_iter().filter_map(|(label, v)| note_numbers.iter().position(|l| *l == label).map(|ix| (ix + 1, v))).collect();
    if !numbered.is_empty() {
        numbered.sort_by_key(|(n, _)| *n);
        numbered.dedup_by_key(|(n, _)| *n);
        blocks.push(Block::Footnotes(numbered));
    }
    blocks
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
    div().flex().flex_col().gap(px(8.)).min_w_0().children(blocks.iter().map(|b| render_block(&id, b, &mut counter, cx))).into_any_element()
}

/// The monospace font for code blocks.
fn mono_font() -> &'static str {
    if cfg!(target_os = "windows") {
        "Consolas"
    } else if cfg!(target_os = "macos") {
        "Menlo"
    } else {
        "DejaVu Sans Mono"
    }
}

fn render_blocks(id: &SharedString, blocks: &[Block], gap: f32, counter: &mut usize, cx: &App) -> gpui::Div {
    div().min_w_0().flex().flex_col().gap(px(gap)).children(blocks.iter().map(|b| render_block(id, b, counter, cx)))
}

fn render_block(id: &SharedString, block: &Block, counter: &mut usize, cx: &App) -> AnyElement {
    let t = cx.theme();
    match block {
        Block::Paragraph(i) => render_inline(id, i, counter, cx).into_any_element(),
        Block::Heading(level, i) => {
            let size = match level {
                1 => t.fs.xxl - px(4.),
                2 => t.fs.xl + px(3.),
                3 => t.fs.xl,
                4 => t.fs.l,
                5 => t.fs.normal,
                _ => t.fs.s,
            };
            div()
                .text_size(size)
                .font_weight(FontWeight::BOLD)
                .when(*level == 6, |d| d.text_color(t.colors.text_light))
                .child(render_inline(id, i, counter, cx))
                .into_any_element()
        }
        Block::Quote(children) => render_blocks(id, children, 6., counter, cx)
            .pl(px(12.))
            .border_l_2()
            .border_color(t.colors.comment_line)
            .text_color(t.colors.text_light)
            .into_any_element(),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(items.iter().enumerate().map(|(n, item)| {
                let marker = match (item.task, start) {
                    (Some(true), _) => icons::sized(icons::Icon::SquareCheck, px(15.), t.colors.brand).mt(px(2.)).into_any_element(),
                    (Some(false), _) => icons::sized(icons::Icon::Square, px(15.), t.colors.text_light).mt(px(2.)).into_any_element(),
                    (None, Some(s)) => div().text_color(t.colors.text_light).child(format!("{}.", s + n as u64)).into_any_element(),
                    (None, None) => div().text_color(t.colors.text_light).child("•").into_any_element(),
                };
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_none().min_w(px(14.)).child(marker))
                    .child(render_blocks(id, &item.blocks, 4., counter, cx).flex_1().when(item.task == Some(true), |d| d.text_color(t.colors.text_light)))
            }))
            .into_any_element(),
        Block::Code(code) => {
            div().p(px(10.)).rounded(t.metrics.radius).bg(t.colors.input).font_family(mono_font()).text_size(t.fs.s).child(code.clone()).into_any_element()
        }
        Block::Rule => div().h(px(1.)).w_full().bg(t.colors.separator).into_any_element(),
        Block::Table { aligns, head, rows } => {
            let cols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
            let row_el = |cells: &[Inline], header: bool, first: bool, counter: &mut usize| {
                div()
                    .flex()
                    .when(header, |d| d.bg(t.colors.input).font_weight(FontWeight::BOLD))
                    .when(!first, |d| d.border_t_1().border_color(t.colors.separator))
                    .children((0..cols).map(|c| {
                        let cell = div().flex_grow().flex_shrink().flex_basis(px(0.)).min_w_0().px(px(10.)).py(px(6.));
                        let cell = match aligns.get(c) {
                            Some(Alignment::Center) => cell.text_center(),
                            Some(Alignment::Right) => cell.text_right(),
                            _ => cell,
                        };
                        cell.when(c > 0, |d| d.border_l_1().border_color(t.colors.separator))
                            .when_some(cells.get(c), |d, i| d.child(render_inline(id, i, counter, cx)))
                    }))
            };
            div()
                .flex()
                .flex_col()
                .border_1()
                .border_color(t.colors.separator)
                .rounded(t.metrics.radius)
                .overflow_hidden()
                .when(!head.is_empty(), |d| d.child(row_el(head, true, true, counter)))
                .children(rows.iter().enumerate().map(|(n, r)| row_el(r, false, head.is_empty() && n == 0, counter)))
                .into_any_element()
        }
        Block::Footnotes(notes) => div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .pt(px(8.))
            .border_t_1()
            .border_color(t.colors.separator)
            .text_size(t.fs.s)
            .text_color(t.colors.text_light)
            .children(notes.iter().map(|(n, blocks)| {
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_none().min_w(px(14.)).child(format!("{n}.")))
                    .child(render_blocks(id, blocks, 4., counter, cx).flex_1())
            }))
            .into_any_element(),
    }
}

/// Highlight styles for `spans`, combining the styles of nested spans.
fn highlights(spans: &[(Range<usize>, Span)], cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
    let t = cx.theme();
    let style = |kind: Span| match kind {
        Span::Strong => HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() },
        Span::Emphasis => HighlightStyle { font_style: Some(FontStyle::Italic), ..Default::default() },
        Span::Strike => HighlightStyle { strikethrough: Some(StrikethroughStyle { thickness: px(1.), color: None }), ..Default::default() },
        Span::Code => HighlightStyle { background_color: Some(t.colors.input), ..Default::default() },
        Span::Link => {
            HighlightStyle { color: Some(t.colors.link), underline: Some(UnderlineStyle { thickness: px(1.), color: None, wavy: false }), ..Default::default() }
        }
        Span::Note => HighlightStyle { color: Some(t.colors.link), ..Default::default() },
    };
    segments(spans).into_iter().map(|(seg, kinds)| (seg, kinds.into_iter().fold(HighlightStyle::default(), |acc, k| merge(acc, style(k))))).collect()
}

/// Cut the text at every span edge; each piece lists the spans covering it,
/// outer (longer) ones first so inner ones can override them.
fn segments(spans: &[(Range<usize>, Span)]) -> Vec<(Range<usize>, Vec<Span>)> {
    let mut edges: Vec<usize> = spans.iter().filter(|(r, _)| !r.is_empty()).flat_map(|(r, _)| [r.start, r.end]).collect();
    edges.sort_unstable();
    edges.dedup();
    let mut out = Vec::new();
    for w in edges.windows(2) {
        let seg = w[0]..w[1];
        let mut covering: Vec<&(Range<usize>, Span)> = spans.iter().filter(|(r, _)| r.start <= seg.start && seg.end <= r.end).collect();
        if covering.is_empty() {
            continue;
        }
        covering.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));
        out.push((seg, covering.into_iter().map(|(_, k)| *k).collect()));
    }
    out
}

fn render_inline(id: &SharedString, i: &Inline, counter: &mut usize, cx: &App) -> AnyElement {
    let styled = StyledText::new(SharedString::from(i.text.clone())).with_highlights(highlights(&i.spans, cx));
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
            Block::Code(c) => c,
            Block::Quote(_) | Block::List { .. } | Block::Rule | Block::Table { .. } | Block::Footnotes(_) => continue,
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
        let blocks = parse("# Title\n\n> quoted\n\n- a\n- b\n\n1. one\n\n```\ncode\n```\n\n---\n\n###### Small");
        assert!(matches!(&blocks[0], Block::Heading(1, i) if i.text == "Title"));
        assert!(matches!(&blocks[1], Block::Quote(v) if v.len() == 1));
        assert!(matches!(&blocks[2], Block::List { start: None, items } if items.len() == 2));
        assert!(matches!(&blocks[3], Block::List { start: Some(1), .. }));
        assert!(matches!(&blocks[4], Block::Code(c) if c == "code"));
        assert!(matches!(&blocks[5], Block::Rule));
        assert!(matches!(&blocks[6], Block::Heading(6, i) if i.text == "Small"));
    }

    #[test]
    fn tables() {
        let blocks = parse("| Name | Score |\n| :--- | ---: |\n| **Ann** | 3 |\n| Bob |\n\nAfter");
        let Block::Table { aligns, head, rows } = &blocks[0] else { panic!("expected table, got {:?}", blocks[0]) };
        assert_eq!(aligns, &[Alignment::Left, Alignment::Right]);
        assert_eq!(head.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Name", "Score"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].text, "Ann");
        assert!(rows[0][0].spans.contains(&(0..3, Span::Strong)));
        assert_eq!(para(&blocks[1]).text, "After");
    }

    #[test]
    fn task_lists() {
        let blocks = parse("- [x] done\n- [ ] todo\n- plain");
        let Block::List { items, .. } = &blocks[0] else { panic!() };
        assert_eq!(items.iter().map(|i| i.task).collect::<Vec<_>>(), [Some(true), Some(false), None]);
        assert_eq!(para(&items[1].blocks[0]).text, "todo");
    }

    #[test]
    fn footnotes_are_numbered_and_moved_to_the_end() {
        let blocks = parse("[^b]: Second.\n\nOne[^a] two[^b].\n\n[^a]: First.\n\n[^unused]: Never shown.");
        assert_eq!(para(&blocks[0]).text, "One[1] two[2].");
        let Block::Footnotes(notes) = blocks.last().unwrap() else { panic!("expected footnotes") };
        assert_eq!(notes.len(), 2);
        assert!(matches!(&notes[0], (1, b) if para(&b[0]).text == "First."));
        assert!(matches!(&notes[1], (2, b) if para(&b[0]).text == "Second."));
    }

    #[test]
    fn html_is_dropped_and_images_show_their_address() {
        assert_eq!(para(&parse("a <b>bold</b> b<br>c")[0]).text, "a bold bc");
        assert!(parse("<div>\nhidden\n</div>").is_empty());
        let blocks = parse("look ![a cat](https://x.org/cat.png)");
        let i = para(&blocks[0]);
        assert_eq!(i.text, "look https://x.org/cat.png");
        assert_eq!(i.links, vec![(5..26, "https://x.org/cat.png".to_string())]);
    }

    #[test]
    fn nested_styles_combine() {
        // Bold-italic text containing a link with code inside, then plain bold-italic.
        let blocks = parse("***[a `b`](https://a.b) c***");
        let i = para(&blocks[0]);
        assert_eq!(i.text, "a b c");
        let segs = segments(&i.spans);
        let kinds = |ix: usize| -> Vec<Span> {
            let mut k = segs[ix].1.clone();
            k.sort_by_key(|s| *s as u8);
            k
        };
        assert_eq!(segs.iter().map(|(r, _)| r.clone()).collect::<Vec<_>>(), [0..2, 2..3, 3..5]);
        assert_eq!(kinds(0), [Span::Strong, Span::Emphasis, Span::Link]);
        assert_eq!(kinds(1), [Span::Strong, Span::Emphasis, Span::Code, Span::Link]);
        assert_eq!(kinds(2), [Span::Strong, Span::Emphasis]);
    }

    #[test]
    fn preview() {
        assert_eq!(plain_preview("**Hi** there\n\nsecond", 100), "Hi there second");
        assert_eq!(plain_preview("abcdef", 3), "abc…");
    }
}
