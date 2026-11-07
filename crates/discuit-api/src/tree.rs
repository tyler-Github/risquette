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
