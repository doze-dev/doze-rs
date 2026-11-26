//! The members of an input or an answer a summary reads: a count of a list, a number, a string. Two sources, one reading: the
//! decoded tree of a call (`TreeFields`) and the JSON of a REST answer (`serde_json::Value`).

use aws_schema::arena::{Arena, NIL, Val};
use aws_schema::tables::Tables;
use serde_json::Value;

/// A structure's top-level members, by name (case aside: Logs writes `logEvents`, Kinesis `Records`).
pub trait Fields {
    /// How many items the list `member` holds; `None` when it is not there (or not a list).
    fn count(&self, member: &str) -> Option<u32>;
    /// The whole number `member` holds.
    fn number(&self, member: &str) -> Option<u64>;
    /// The text `member` holds.
    fn text(&self, member: &str) -> Option<&str>;
    /// The text `member` of the structure `parent` holds (IAM `PolicyVersion.VersionId`).
    fn text_in(&self, parent: &str, member: &str) -> Option<&str>;
    /// The boolean `member` holds (`IsTruncated`).
    fn flag(&self, member: &str) -> Option<bool>;
    /// Whether `member` is there and not null.
    fn present(&self, member: &str) -> bool;
}

/// The decoded tree of a call's input or output.
#[derive(Debug, Clone, Copy)]
pub struct TreeFields<'a> {
    pub tables: &'static Tables,
    pub arena: &'a Arena,
    pub body: &'a str,
    pub root: u32,
}

impl TreeFields<'_> {
    fn find(&self, member: &str) -> Option<Val> {
        if self.root == NIL {
            return None;
        }
        let Val::Obj(first) = self.arena.node(self.root)?.val else {
            return None;
        };
        self.find_from(first, member)
    }

    /// The member among the siblings that begin at `first`.
    fn find_from(&self, first: u32, member: &str) -> Option<Val> {
        let mut at = first;
        while at != NIL {
            let node = self.arena.node(at)?;
            if self
                .tables
                .member(node.member)
                .is_some_and(|m| m.name.eq_ignore_ascii_case(member))
            {
                return Some(node.val);
            }
            at = node.next;
        }
        None
    }
}

impl Fields for TreeFields<'_> {
    fn count(&self, member: &str) -> Option<u32> {
        let Val::Seq(first) = self.find(member)? else {
            return None;
        };
        let (mut n, mut at) = (0u32, first);
        while at != NIL {
            n = n.saturating_add(1);
            at = self.arena.node(at)?.next;
        }
        Some(n)
    }

    fn number(&self, member: &str) -> Option<u64> {
        match self.find(member)? {
            Val::Int(i) => u64::try_from(i).ok(),
            _ => None,
        }
    }

    fn text(&self, member: &str) -> Option<&str> {
        self.arena.str_of(self.body, self.find(member)?)
    }

    fn text_in(&self, parent: &str, member: &str) -> Option<&str> {
        let Val::Obj(first) = self.find(parent)? else {
            return None;
        };
        self.arena.str_of(self.body, self.find_from(first, member)?)
    }

    fn flag(&self, member: &str) -> Option<bool> {
        match self.find(member)? {
            Val::Bool(b) => Some(b),
            _ => None,
        }
    }

    fn present(&self, member: &str) -> bool {
        self.find(member).is_some_and(|v| !matches!(v, Val::Null))
    }
}

fn get<'v>(v: &'v Value, member: &str) -> Option<&'v Value> {
    let o = v.as_object()?;
    o.get(member).or_else(|| {
        o.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(member))
            .map(|(_, v)| v)
    })
}

impl Fields for Value {
    fn count(&self, member: &str) -> Option<u32> {
        let n = get(self, member)?.as_array()?.len();
        Some(u32::try_from(n).unwrap_or(u32::MAX))
    }

    fn number(&self, member: &str) -> Option<u64> {
        let v = get(self, member)?;
        v.as_u64().or_else(|| v.as_str()?.parse().ok())
    }

    fn text(&self, member: &str) -> Option<&str> {
        get(self, member)?.as_str()
    }

    fn text_in(&self, parent: &str, member: &str) -> Option<&str> {
        get(get(self, parent)?, member)?.as_str()
    }

    fn flag(&self, member: &str) -> Option<bool> {
        get(self, member)?.as_bool()
    }

    fn present(&self, member: &str) -> bool {
        get(self, member).is_some_and(|v| !v.is_null())
    }
}
