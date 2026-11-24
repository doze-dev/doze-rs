//! Registering the console's working blocks (protocol minor 2): a value viewer and a tree. Each sets its capability
//! bit, so a host offers the block only for a module that answers it. The SDK holds the module to the bytes the host
//! asked for.

use crate::caps::{Tree, Values};
use crate::module::Module;
use crate::route;
use crate::route::args;
use crate::server::{Route, Server};
use crate::with_cx;
use doze_module_proto::method::Method;
use doze_module_proto::msg::blocks::{ChildrenArgs, VALUE_CHUNK, ValueAnswer, ValueArgs};
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
                with_cx!(m, a.instance, call, peer, |cx| m
                    .children(&cx, &a.panel, &a.parent, &a.filter, a.cursor.as_deref())
                    .await);
            }),
        )
    }
}

/// A value's part held to `max` bytes, its size at least what was sent.
fn cut_to(mut v: ValueAnswer, offset: u64, max: u32) -> ValueAnswer {
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
    }
}
