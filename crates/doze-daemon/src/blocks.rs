//! `value` and `children` (Rust daemon, additive): a module console's value viewer and tree, asked of the instance's
//! module. Nothing here names a module: what each offers comes from its manifest. A sleeping instance is woken only when
//! the request says the person opened a value (`wake`); a tree never wakes one.

use crate::Daemon;
use doze_control::{ChildrenView, PairView, Response, TreeNodeView, ValueView, fault};
use doze_engine::modules::{Block, ModuleInstance};
use doze_module_proto::msg::blocks::{self as proto, VALUE_CHUNK};
use std::sync::Arc;

fn failed(error: String, code: &str) -> Response {
    Response {
        ok: false,
        error,
        code: code.into(),
        ..Response::default()
    }
}

fn refuse(error: String, code: &str) -> Box<Response> {
    Box::new(failed(error, code))
}

/// The module instance `name`, awake, offering `block`; or the answer to give instead.
async fn ready(
    d: &Daemon,
    name: &str,
    block: Block,
    wake: bool,
) -> Result<Arc<ModuleInstance>, Box<Response>> {
    if d.engine().get(name).is_none() {
        let why = format!("instance {name:?} is not declared");
        return Err(refuse(why, fault::NOT_FOUND));
    }
    let has = match d.module(name) {
        Some(mi) => mi.has_block(block).await.map(|h| h.then_some(mi)),
        None => Ok(None),
    };
    let mi = match has {
        Ok(Some(mi)) => mi,
        Ok(None) => {
            let engine = d.label(name).map(|l| l.engine).unwrap_or_default();
            let what = match block {
                Block::Value => "value viewer",
                Block::Tree => "tree",
            };
            let why = format!("instance {name:?} ({engine}) has no {what}");
            return Err(refuse(why, ""));
        }
        Err(e) => return Err(refuse(e, "")),
    };
    if !d.engine().get(name).is_some_and(|i| i.state.running()) {
        if !wake {
            return Err(refuse(
                format!("{name} is asleep. Opening a value wakes it."),
                fault::ASLEEP,
            ));
        }
        crate::unary::wake(d, name)
            .await
            .map_err(|e| refuse(e.to_string(), ""))?;
    }
    Ok(mi)
}

/// The `value` op.
pub(crate) async fn value(d: &Daemon, req: &doze_control::Request) -> Response {
    let name = req.db.as_str();
    let mi = match ready(d, name, Block::Value, req.wake).await {
        Ok(mi) => mi,
        Err(r) => return *r,
    };
    let ctx = d.engine().ctx_for(name);
    let row = req.row.clone();
    match mi
        .value(&ctx, &req.panel, row, req.offset, VALUE_CHUNK)
        .await
    {
        Ok(a) => Response {
            ok: true,
            value: Some(value_view(a, req.offset)),
            ..Response::default()
        },
        Err(e) => failed(e, ""),
    }
}

pub(crate) fn value_view(a: proto::ValueAnswer, offset: u64) -> ValueView {
    let mut v = ValueView {
        media: a.media,
        pairs: a
            .pairs
            .into_iter()
            .map(|(key, value)| PairView { key, value })
            .collect(),
        size: a.size,
        offset,
        more: a.more,
        ..ValueView::default()
    };
    v.set_body(&a.body);
    v
}

/// The `children` op. A tree is read only while the instance is awake.
pub(crate) async fn children(d: &Daemon, req: &doze_control::Request) -> Response {
    let name = req.db.as_str();
    let mi = match ready(d, name, Block::Tree, false).await {
        Ok(mi) => mi,
        Err(r) => return *r,
    };
    let ctx = d.engine().ctx_for(name);
    let cursor = (!req.cursor.is_empty()).then(|| req.cursor.clone());
    match mi
        .children(&ctx, &req.panel, &req.parent, &req.filter, cursor)
        .await
    {
        Ok(c) => Response {
            ok: true,
            children: Some(children_view(c)),
            ..Response::default()
        },
        Err(e) => failed(e, ""),
    }
}

pub(crate) fn children_view(c: proto::Children) -> ChildrenView {
    ChildrenView {
        nodes: c
            .nodes
            .into_iter()
            .map(|n| TreeNodeView {
                id: n.id,
                label: n.label,
                leaf: n.leaf,
                info: n.info,
            })
            .collect(),
        next: c.next.unwrap_or_default(),
    }
}
