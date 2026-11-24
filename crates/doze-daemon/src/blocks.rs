//! `value` and `children` (Rust daemon, additive): a module console's value viewer and tree, asked of the instance's
//! module. Nothing here names a module: what each offers comes from its manifest. A sleeping instance is woken only when
//! the request says the person opened a value (`wake`); a tree never wakes one.

use crate::Daemon;
use doze_control::{ChildrenView, PairView, Response, TreeNodeView, ValueView, fault};
use doze_engine::modules::{Block, ModuleInstance};
use doze_engine::{Instance, State};
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
    awake(d, name, wake).await?;
    Ok(mi)
}

/// The instance up, or why not in its own words: only one truly asleep is "asleep", and only an explicit press
/// (`wake`) wakes it (or waits for a start already under way). One that is off, failed or crashing is not woken.
async fn awake(d: &Daemon, name: &str, wake: bool) -> Result<(), Box<Response>> {
    let Some(i) = d.engine().get(name) else {
        let why = format!("instance {name:?} is not declared");
        return Err(refuse(why, fault::NOT_FOUND));
    };
    let off = i.off || d.label(name).is_some_and(|l| l.paused);
    match refusal(name, &i, off, wake) {
        Some(r) => Err(r),
        None if i.state.running() => Ok(()),
        None => crate::unary::wake(d, name)
            .await
            .map_err(|e| refuse(e.to_string(), fault::FAILED)),
    }
}

/// Why a block cannot be read from instance `i` now; `None` when it runs, or may be woken (`wake`) or waited for.
fn refusal(name: &str, i: &Instance, off: bool, wake: bool) -> Option<Box<Response>> {
    let why = if i.last_error.is_empty() {
        String::new()
    } else {
        format!(": {}", i.last_error)
    };
    let (text, code) = match i.state {
        s if s.running() => return None,
        _ if off => (
            format!(
                "{name} is off: `doze on {name}` (or `enabled = true` in the config) brings it back."
            ),
            fault::OFF,
        ),
        State::Failed => (
            format!("{name} failed{why}. `doze restart {name}` tries again."),
            fault::FAILED,
        ),
        State::Crashing => (
            format!(
                "{name} crashed and is restarting (attempt {} of {}){why}.",
                i.restart_count, i.restart_max
            ),
            fault::FAILED,
        ),
        _ if wake => return None,
        State::Booting => (
            format!("{name} is starting. Try again in a moment."),
            fault::STARTING,
        ),
        _ => (
            format!("{name} is asleep. Opening a value wakes it."),
            fault::ASLEEP,
        ),
    };
    Some(refuse(text, code))
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn said(state: State, off: bool, wake: bool) -> Option<(String, String)> {
        let i = Instance {
            state,
            last_error: if state == State::Failed {
                "exit status 1".into()
            } else {
                String::new()
            },
            restart_count: 2,
            restart_max: 5,
            ..Instance::default()
        };
        refusal("db", &i, off, wake).map(|r| (r.error.clone(), r.code.clone()))
    }

    #[test]
    fn only_an_instance_truly_asleep_is_called_asleep() {
        assert_eq!(said(State::Idle, false, false), None);
        assert_eq!(
            said(State::Reaped, false, true),
            None,
            "an explicit open wakes it"
        );
        assert_eq!(
            said(State::Booting, false, true),
            None,
            "and waits for a start under way"
        );
        let code = |s, off, wake| said(s, off, wake).unwrap().1;
        assert_eq!(code(State::Reaped, false, false), fault::ASLEEP);
        assert_eq!(code(State::Booting, false, false), fault::STARTING);
        assert_eq!(
            code(State::Reaped, true, true),
            fault::OFF,
            "never woken when off"
        );
        let (failed, c) = said(State::Failed, false, true).unwrap();
        assert_eq!(
            (failed.as_str(), c.as_str()),
            (
                "db failed: exit status 1. `doze restart db` tries again.",
                fault::FAILED
            )
        );
        let (crashing, c) = said(State::Crashing, false, true).unwrap();
        assert_eq!(
            (crashing.as_str(), c.as_str()),
            (
                "db crashed and is restarting (attempt 2 of 5).",
                fault::FAILED
            )
        );
    }
}
