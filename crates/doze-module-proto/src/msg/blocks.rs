//! The console's working blocks (minor 2): a value viewer, action forms, a time chart, checks and a tree (proposal
//! `console-blocks-2026-11-24`). The module declares data and layout, the host draws. Two of them are calls a module
//! offers with a capability bit (`value`, `children`); the other three are declarations on types
//! protocol 2.0 already has (an [`Action`](super::console::Action)'s fields, a "series" [`Node`](super::console::Node),
//! resources of kind [`check::KIND`]), so an older host shows what it always showed.

use super::instance::Instance;
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

/// The layout node kinds the blocks add. A host that does not know one draws a placeholder ("This needs a newer
/// doze") in its place and the rest of the tab as usual.
pub mod kind {
    /// The value of the row selected in the table block named by the node's `from`.
    pub const VALUE: &str = "value";
    /// Lines over time: the node's `keys` columns of its panel, one line per value of its `by` column.
    pub const SERIES: &str = "series";
    /// Pass, warn or fail rows: the resources of kind [`check::KIND`](super::check::KIND), worst first.
    pub const CHECKS: &str = "checks";
    /// Folders that open as you go, each folder's children asked of the module (`children`).
    pub const TREE: &str = "tree";
}

/// A check is a resource the module already lists (`resources`): kind [`KIND`](check::KIND), its status one of [`OK`](check::OK), [`WARN`](check::WARN) or
/// [`BAD`](check::BAD), its info holding a plain-words [`DETAIL`](check::DETAIL) and, when the module can mend it, a [`FIX`](check::FIX): the id of one of its
/// actions. A "bad" check also shows on the map and in Needs you.
pub mod check {
    pub const KIND: &str = "check";
    pub const OK: &str = "ok";
    pub const WARN: &str = "warn";
    pub const BAD: &str = "bad";
    /// The info key of the result in plain words.
    pub const DETAIL: &str = "detail";
    /// The info key of the action that mends it.
    pub const FIX: &str = "fix";
}

/// One typed field of an action's form. An action with fields keeps its `input_hint`, so a host that does not know
/// forms still shows its one box.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct Field {
    /// The key its value is sent under (`RunAction::values`).
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub label: String,
    /// "text", "secret" (never echoed or logged), "number", "choice", "bool" or "pairs" ([`field`]). A host that
    /// does not know a kind shows a text box.
    #[n(2)]
    pub kind: String,
    #[n(3)]
    pub required: bool,
    /// Filled in when the form opens: a number in decimal, "true"/"false", a choice, pairs as `k=v` lines.
    #[n(4)]
    pub default: Option<String>,
    /// A "choice" field's options.
    #[n(5)]
    #[cbor(default)]
    pub choices: Vec<String>,
    /// A "number" field's bounds (whole numbers).
    #[n(6)]
    pub min: Option<i64>,
    #[n(7)]
    pub max: Option<i64>,
    /// Shown under the field.
    #[n(8)]
    pub help: Option<String>,
}

/// The kinds of [`Field`].
pub mod field {
    pub const TEXT: &str = "text";
    pub const SECRET: &str = "secret";
    pub const NUMBER: &str = "number";
    pub const CHOICE: &str = "choice";
    pub const BOOL: &str = "bool";
    pub const PAIRS: &str = "pairs";
}

/// `value` arguments: part of one value in full.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct ValueArgs {
    #[n(0)]
    pub instance: Instance,
    /// The panel the row is in (a table, a stream or a tree).
    #[n(1)]
    pub panel: String,
    /// The selected row's cells (a tree's leaf: `id`), from which the module knows the value (a topic's partition and
    /// offset, a key).
    #[n(2)]
    pub row: BTreeMap<String, String>,
    /// From which byte.
    #[n(3)]
    pub offset: u64,
    /// Bytes at most, up to [`VALUE_CHUNK`].
    #[n(4)]
    pub max: u32,
}

/// The most of a value one answer carries; "Load the rest" asks again from where it stopped.
pub const VALUE_CHUNK: u32 = 1 << 20;

/// Part of a value, and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct ValueAnswer {
    /// "json", "text", "bytes" or "pairs" ([`media`]); a host that does not know one shows bytes.
    #[n(0)]
    pub media: String,
    #[n(1)]
    #[cbor(with = "minicbor::bytes")]
    pub body: Vec<u8>,
    /// Key–value pairs beside or instead of the body (a message's headers, a hash's fields), in order.
    #[n(2)]
    #[cbor(default)]
    pub pairs: Vec<(String, String)>,
    /// The whole value's size in bytes.
    #[n(3)]
    pub size: u64,
    /// More follows `offset + body.len()`.
    #[n(4)]
    pub more: bool,
}

/// What a value is.
pub mod media {
    pub const JSON: &str = "json";
    pub const TEXT: &str = "text";
    pub const BYTES: &str = "bytes";
    pub const PAIRS: &str = "pairs";
}

impl ValueAnswer {
    /// The part of `whole` from `offset`, at most `max` bytes (and [`VALUE_CHUNK`]), with its size and whether more
    /// follows.
    pub fn slice(media: &str, whole: &[u8], offset: u64, max: u32) -> ValueAnswer {
        let max = if max == 0 {
            VALUE_CHUNK
        } else {
            max.min(VALUE_CHUNK)
        };
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(whole.len());
        let end = start.saturating_add(max as usize).min(whole.len());
        ValueAnswer {
            media: media.into(),
            body: whole.get(start..end).unwrap_or_default().to_vec(),
            pairs: Vec::new(),
            size: whole.len() as u64,
            more: end < whole.len(),
        }
    }
}

/// `children` arguments: one page of a folder's children.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct ChildrenArgs {
    #[n(0)]
    pub instance: Instance,
    /// The tree panel.
    #[n(1)]
    pub panel: String,
    /// The folder's id; empty for the top.
    #[n(2)]
    pub parent: String,
    /// What the person typed to filter by; the module searches everything, not only the open folders.
    #[n(3)]
    pub filter: String,
    /// Where the last page stopped.
    #[n(4)]
    pub cursor: Option<String>,
}

/// One page of a folder's children.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct Children {
    #[n(0)]
    #[cbor(default)]
    pub nodes: Vec<TreeNode>,
    /// Ask again with this cursor for the next page.
    #[n(1)]
    pub next: Option<String>,
}

/// A folder or a leaf of a tree. A leaf opens in the value viewer, asked with the row `{"id": id}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct TreeNode {
    /// Unique in the tree: what `children` and `value` are asked with.
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub label: String,
    #[n(2)]
    pub leaf: bool,
    /// Shown beside the label ("rows" → "12,431 rows", "bytes" → "1.2 KB").
    #[n(3)]
    #[cbor(default)]
    pub info: BTreeMap<String, String>,
}

impl Children {
    /// One page of `all` (already filtered) after `cursor`, `size` at most, with the cursor of the next: for a module
    /// that has the whole list at hand.
    pub fn page(all: Vec<TreeNode>, cursor: Option<&str>, size: usize) -> Children {
        let from = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
        let size = size.max(1);
        let total = all.len();
        let nodes: Vec<TreeNode> = all.into_iter().skip(from).take(size).collect();
        let end = from.saturating_add(nodes.len());
        Children {
            nodes,
            next: (end < total).then(|| end.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_sliced_with_its_size_and_whether_more_follows() {
        let whole = vec![7u8; 10];
        let a = ValueAnswer::slice(media::BYTES, &whole, 4, 4);
        assert_eq!((a.body.len(), a.size, a.more), (4, 10, true));
        let b = ValueAnswer::slice(media::BYTES, &whole, 8, 4);
        assert_eq!((b.body.len(), b.more), (2, false));
        let past = ValueAnswer::slice(media::BYTES, &whole, 99, 4);
        assert_eq!((past.body.len(), past.more), (0, false));
        let big = vec![0u8; VALUE_CHUNK as usize + 5];
        let c = ValueAnswer::slice(media::BYTES, &big, 0, u32::MAX);
        assert_eq!((c.body.len(), c.more), (VALUE_CHUNK as usize, true));
    }

    #[test]
    fn children_page_by_cursor() {
        let all: Vec<TreeNode> = (0..5)
            .map(|i| TreeNode {
                id: i.to_string(),
                ..TreeNode::default()
            })
            .collect();
        let p1 = Children::page(all.clone(), None, 2);
        assert_eq!(p1.next.as_deref(), Some("2"));
        let p3 = Children::page(all, Some("4"), 2);
        assert_eq!((p3.nodes.len(), p3.next), (1, None));
    }
}
