//! Turn Discuit's flat comment list into a tree, then into rows for display.

use crate::Comment;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct CommentNode {
    pub comment: Comment,
    pub children: Vec<CommentNode>,
}

/// Build a forest from a flat list. Order of siblings follows the order the
/// server returned them in. Comments whose parent is missing from the list
/// (e.g. on a later page) become roots so nothing is dropped.
pub fn build_tree(comments: Vec<Comment>) -> Vec<CommentNode> {
    let ids: HashSet<String> = comments.iter().map(|c| c.id.clone()).collect();
    let mut children: HashMap<String, Vec<Comment>> = HashMap::new();
    let mut roots = Vec::new();
    for c in comments {
        match c.parent_id.clone() {
            Some(p) if ids.contains(&p) => children.entry(p).or_default().push(c),
            _ => roots.push(c),
        }
    }
    fn attach(c: Comment, children: &mut HashMap<String, Vec<Comment>>) -> CommentNode {
        let kids = children.remove(&c.id).unwrap_or_default();
        CommentNode { children: kids.into_iter().map(|k| attach(k, children)).collect(), comment: c }
    }
    roots.into_iter().map(|c| attach(c, &mut children)).collect()
}

/// How replies at each level of a thread are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommentSort {
    /// Most upvotes first (the server's own order).
    #[default]
    Best,
    /// The longest thread first: the most comments counting all replies,
    /// then the deepest, then the most upvotes.
    Top,
    /// Newest first.
    New,
    /// Oldest first, as the conversation happened.
    Old,
}

impl CommentSort {
    pub const ALL: [CommentSort; 4] = [CommentSort::Best, CommentSort::Top, CommentSort::New, CommentSort::Old];

    pub fn label(self) -> &'static str {
        match self {
            CommentSort::Best => "Best",
            CommentSort::Top => "Top",
            CommentSort::New => "New",
            CommentSort::Old => "Old",
        }
    }

    /// The value saved in settings.
    pub fn as_str(self) -> &'static str {
        match self {
            CommentSort::Best => "best",
            CommentSort::Top => "top",
            CommentSort::New => "new",
            CommentSort::Old => "old",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// Order each level of a forest by `sort`: top-level comments among
/// themselves, and each comment's replies among themselves, so replies always
/// stay under their parent. Ties are broken by id so the order is stable.
pub fn sort_tree(nodes: &mut [CommentNode], sort: CommentSort) {
    // Thread sizes are worked out once per level rather than inside the comparison.
    let mut keyed: Vec<((usize, usize), CommentNode)> = nodes.iter().map(|n| (thread_size(n), n.clone())).collect();
    keyed.sort_by(|(size_a, a), (size_b, b)| {
        let (a, b) = (&a.comment, &b.comment);
        let best = || b.upvotes.cmp(&a.upvotes).then((b.upvotes - b.downvotes).cmp(&(a.upvotes - a.downvotes))).then(b.created_at.cmp(&a.created_at));
        let order = match sort {
            CommentSort::Best => best(),
            // (comments, depth): bigger first, then deeper, then the best.
            CommentSort::Top => size_b.cmp(size_a).then_with(best),
            CommentSort::New => b.created_at.cmp(&a.created_at),
            CommentSort::Old => a.created_at.cmp(&b.created_at),
        };
        order.then_with(|| a.id.cmp(&b.id))
    });
    for (slot, (_, node)) in nodes.iter_mut().zip(keyed) {
        *slot = node;
    }
    for n in nodes {
        sort_tree(&mut n.children, sort);
    }
}

/// A thread's length: how many comments it holds (this one and every reply
/// under it), and how deep its longest chain of replies goes.
fn thread_size(n: &CommentNode) -> (usize, usize) {
    n.children.iter().map(thread_size).fold((1, 1), |(count, depth), (c, d)| (count + c, depth.max(d + 1)))
}

/// One visible row of a comment thread.
#[derive(Debug, Clone)]
pub struct CommentRow {
    pub comment: Comment,
    /// Visual depth relative to the thread (not the server's `depth`).
    pub level: usize,
    pub collapsed: bool,
    /// Replies that exist on the server but are not loaded in this tree.
    pub unloaded_replies: i64,
}

/// Flatten a forest depth-first, skipping descendants of collapsed comments.
pub fn flatten(nodes: &[CommentNode], collapsed: &HashSet<String>) -> Vec<CommentRow> {
    fn walk(n: &CommentNode, level: usize, collapsed: &HashSet<String>, out: &mut Vec<CommentRow>) {
        let is_collapsed = collapsed.contains(&n.comment.id);
        let loaded = n.children.len() as i64;
        out.push(CommentRow { comment: n.comment.clone(), level, collapsed: is_collapsed, unloaded_replies: (n.comment.no_replies_direct - loaded).max(0) });
        if !is_collapsed {
            for c in &n.children {
                walk(c, level + 1, collapsed, out);
            }
        }
    }
    let mut out = Vec::new();
    for n in nodes {
        walk(n, 0, collapsed, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parent: Option<&str>, direct: i64) -> Comment {
        Comment { id: id.into(), parent_id: parent.map(Into::into), no_replies_direct: direct, created_at: Default::default(), ..Default::default() }
    }

    fn scored(id: &str, parent: Option<&str>, up: i64, down: i64, minute: u32) -> Comment {
        let created_at = chrono::DateTime::parse_from_rfc3339(&format!("2026-01-01T00:{minute:02}:00Z")).unwrap().into();
        Comment { id: id.into(), parent_id: parent.map(Into::into), upvotes: up, downvotes: down, created_at, ..Default::default() }
    }

    #[test]
    fn every_level_is_sorted_and_replies_stay_under_their_parent() {
        // a: score 5, posted minute 1; b: score 9, minute 3; c: score 2, minute 2.
        // a's replies: a1 (score 1, minute 4) and a2 (score 3, minute 5).
        let list = vec![
            scored("a", None, 6, 1, 1),
            scored("b", None, 9, 0, 3),
            scored("c", None, 2, 0, 2),
            scored("a1", Some("a"), 1, 0, 4),
            scored("a2", Some("a"), 3, 0, 5),
        ];
        let order = |sort| {
            let mut tree = build_tree(list.clone());
            sort_tree(&mut tree, sort);
            flatten(&tree, &HashSet::new()).into_iter().map(|r| r.comment.id).collect::<Vec<_>>()
        };
        assert_eq!(order(CommentSort::Best), ["b", "a", "a2", "a1", "c"]);
        // a holds the only thread (3 comments), so it leads despite fewer upvotes than b.
        assert_eq!(order(CommentSort::Top), ["a", "a2", "a1", "b", "c"]);
        assert_eq!(order(CommentSort::New), ["b", "c", "a", "a2", "a1"]);
        assert_eq!(order(CommentSort::Old), ["a", "a1", "a2", "c", "b"]);
        assert_eq!(CommentSort::parse("old"), Some(CommentSort::Old));
        assert_eq!(CommentSort::parse("nope"), None);
    }

    #[test]
    fn top_puts_the_longest_thread_first() {
        // x: one comment with two direct replies (3 comments, depth 2).
        // y: a chain of three (3 comments, depth 3): same size, deeper, so it wins.
        // z: four comments in a chain: biggest of all.
        let list = vec![
            scored("x", None, 50, 0, 1),
            scored("x1", Some("x"), 1, 0, 2),
            scored("x2", Some("x"), 1, 0, 3),
            scored("y", None, 1, 0, 4),
            scored("y1", Some("y"), 1, 0, 5),
            scored("y2", Some("y1"), 1, 0, 6),
            scored("z", None, 0, 0, 7),
            scored("z1", Some("z"), 0, 0, 8),
            scored("z2", Some("z1"), 0, 0, 9),
            scored("z3", Some("z2"), 0, 0, 10),
            scored("lonely", None, 99, 0, 11),
        ];
        let mut tree = build_tree(list);
        sort_tree(&mut tree, CommentSort::Top);
        let roots: Vec<_> = tree.iter().map(|n| n.comment.id.as_str()).collect();
        assert_eq!(roots, ["z", "y", "x", "lonely"]);
    }

    #[test]
    fn builds_and_flattens_out_of_order_list() {
        // Children can arrive after unrelated roots, as on the live API.
        let list = vec![c("a", None, 1), c("b", None, 0), c("a1", Some("a"), 1), c("a1x", Some("a1"), 0), c("orphan", Some("missing"), 0)];
        let tree = build_tree(list);
        assert_eq!(tree.len(), 3);
        let rows = flatten(&tree, &HashSet::new());
        let got: Vec<(&str, usize)> = rows.iter().map(|r| (r.comment.id.as_str(), r.level)).collect();
        assert_eq!(got, [("a", 0), ("a1", 1), ("a1x", 2), ("b", 0), ("orphan", 0)]);

        let collapsed: HashSet<String> = ["a".to_string()].into();
        let rows = flatten(&tree, &collapsed);
        assert_eq!(rows.len(), 3);
        assert!(rows[0].collapsed);
    }

    #[test]
    fn reports_unloaded_replies() {
        let tree = build_tree(vec![c("a", None, 3), c("a1", Some("a"), 0)]);
        let rows = flatten(&tree, &HashSet::new());
        assert_eq!(rows[0].unloaded_replies, 2);
    }
}
