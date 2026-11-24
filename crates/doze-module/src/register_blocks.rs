//! Registering the console's working blocks (protocol minor 2): a value viewer and a tree. Each sets its capability
//! bit, so a host offers the block only for a module that answers it. The SDK holds the module to the bytes the host
//! asked for (the pairs ride with the first part only), and a tree page to [`CHILDREN_PAGE`] nodes.

use crate::caps::{Tree, Values};
use crate::module::Module;
use crate::route;
use crate::route::args;
use crate::server::{Route, Server};
use crate::with_cx;
use doze_module_proto::method::Method;
use doze_module_proto::msg::blocks::{
    CHILDREN_PAGE, Children, ChildrenArgs, VALUE_CHUNK, ValueAnswer, ValueArgs,
};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::peer::Incoming;
use std::sync::Arc;

impl<M: Module> Server<M> {
    /// The value viewer: the module gives one value in full, a part at a time.
    pub fn values(self) -> Self
    where
        M: Values,
    {
        self.add(
            caps::VALUE,
            Method::Value,
            route!(|m, call, peer| {
                let Some((a, call)) = args::<ValueArgs>(call) else {
                    return;
                };
                let max = if a.max == 0 {
                    VALUE_CHUNK
                } else {
                    a.max.min(VALUE_CHUNK)
                };
                with_cx!(m, a.instance, call, peer, |cx| m
                    .value(&cx, &a.panel, &a.row, a.offset, max)
                    .await
                    .map(|v| cut_to(v, a.offset, max)));
            }),
        )
    }

    /// The tree: the module gives a folder's children as it opens.
    pub fn tree(self) -> Self
    where
        M: Tree,
    {
        self.add(
            caps::TREE,
            Method::Children,
            route!(|m, call, peer| {
                let Some((a, call)) = args::<ChildrenArgs>(call) else {
                    return;
                };
                // A cursor of the SDK's own continues a page it cut: ask the module for that page again and skip what
                // was sent.
                let (skip, cursor) = unwrap_cursor(a.cursor);
                with_cx!(m, a.instance, call, peer, |cx| m
                    .children(&cx, &a.panel, &a.parent, &a.filter, cursor.as_deref())
                    .await
                    .map(|c| paged(c, skip, cursor, CHILDREN_PAGE)));
            }),
        )
    }
}

/// What marks a cursor the SDK made when it cut a page: `<mark><skip>:<the module's cursor>` (`-` for none).
const MARK: &str = "doze-sdk-page:";

/// The nodes already sent of the module's page, and the module's own cursor for that page.
fn unwrap_cursor(cursor: Option<String>) -> (usize, Option<String>) {
    let Some(rest) = cursor.as_deref().and_then(|c| c.strip_prefix(MARK)) else {
        return (0, cursor);
    };
    match rest.split_once(':') {
        Some((skip, inner)) => {
            let inner = inner.strip_prefix('=').map(String::from);
            (skip.parse().unwrap_or(0), inner)
        }
        None => (0, None),
    }
}

/// The module's page from `skip`, at most `max` nodes; the rest continue under a cursor of the SDK's.
fn paged(mut c: Children, skip: usize, cursor: Option<String>, max: usize) -> Children {
    c.nodes.drain(..skip.min(c.nodes.len()));
    if c.nodes.len() > max {
        c.nodes.truncate(max);
        let inner = cursor.map_or_else(|| "-".to_string(), |c| format!("={c}"));
        c.next = Some(format!("{MARK}{}:{inner}", skip + max));
    }
    c
}

/// A value's part held to `max` bytes, its size at least what was sent; its pairs only with the first part.
fn cut_to(mut v: ValueAnswer, offset: u64, max: u32) -> ValueAnswer {
    if offset > 0 {
        v.pairs.clear();
    }
    if v.body.len() > max as usize {
        v.body.truncate(max as usize);
        v.more = true;
    }
    let reached = offset.saturating_add(v.body.len() as u64);
    if v.size < reached {
        v.size = reached;
    }
    v
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_module_is_held_to_the_bytes_asked() {
        let v = ValueAnswer {
            body: vec![1; 10],
            ..ValueAnswer::default()
        };
        let cut = cut_to(v, 100, 4);
        assert_eq!((cut.body.len(), cut.more, cut.size), (4, true, 104));
        let first = ValueAnswer {
            pairs: vec![("k".into(), "v".into())],
            ..ValueAnswer::default()
        };
        assert_eq!(cut_to(first.clone(), 0, 4).pairs.len(), 1);
        assert!(
            cut_to(first, 4, 4).pairs.is_empty(),
            "the pairs came with the first part"
        );
    }

    /// A module that answers a whole folder at once: the SDK sends it a page at a time and walks the module's own
    /// pages after.
    #[test]
    fn a_long_page_is_cut_and_continued() {
        let node = |i: usize| doze_module_proto::msg::blocks::TreeNode {
            id: i.to_string(),
            ..Default::default()
        };
        // The module's first page is 7 nodes with its own cursor "p2"; the SDK sends 3 at a time.
        let module = |cursor: Option<&str>| match cursor {
            None => Children {
                nodes: (0..7).map(node).collect(),
                next: Some("p2".into()),
            },
            Some("p2") => Children {
                nodes: (7..9).map(node).collect(),
                next: None,
            },
            Some(other) => panic!("{other}"),
        };
        let mut seen = Vec::new();
        let mut asked: Option<String> = None;
        loop {
            let (skip, cursor) = unwrap_cursor(asked.clone());
            let page = paged(module(cursor.as_deref()), skip, cursor, 3);
            assert!(page.nodes.len() <= 3);
            seen.extend(page.nodes.iter().map(|n| n.id.clone()));
            match page.next {
                Some(n) => asked = Some(n),
                None => break,
            }
        }
        let want: Vec<String> = (0..9).map(|i| i.to_string()).collect();
        assert_eq!(seen, want);
    }
}
