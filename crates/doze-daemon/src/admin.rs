//! `reset`, `resources` and `admin` (Go's `handler.Reset`, `Resources`, `Admin` and `runtime.ResetData`): an
//! instance's data wiped, and a module instance's resources and data actions, asked of its module. Nothing here names a
//! module: whatever a module offers comes from its manifest.

use crate::Daemon;
use doze_control::{ActionView, ResourceView, Response, fault};
use doze_engine::Cause;
use doze_engine::modules::ModuleInstance;
use std::sync::Arc;

fn failed(error: String, code: &str) -> Response {
    Response {
        ok: false,
        error,
        code: code.into(),
        ..Response::default()
    }
}

fn ok() -> Response {
    Response {
        ok: true,
        ..Response::default()
    }
}

fn declared(d: &Daemon, name: &str) -> bool {
    d.engine().get(name).is_some()
}

fn not_declared(name: &str) -> Response {
    failed(
        format!("instance {name:?} is not declared"),
        fault::NOT_FOUND,
    )
}

/// Go's `ResetData`: stops it, removes its data and run folders, and forgets a task's last run (it runs again at its
/// next trigger).
pub(crate) async fn reset(d: &Daemon, name: &str) -> Response {
    if name.is_empty() {
        return failed("reset needs an instance name".into(), "");
    }
    if !declared(d, name) {
        return not_declared(name);
    }
    let e = d.engine();
    if let Err(err) = e.stop(name, Cause::Command).await {
        return failed(format!("stopping {name:?}: {err}"), "");
    }
    let stack = e.stack();
    for (what, dir) in [
        ("data", stack.cluster_dir(name)),
        ("sockets", stack.run_dir().join(name)),
    ] {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return failed(format!("removing {what} for {name:?}: {err}"), ""),
        }
    }
    if e.is_task(name) {
        e.set_task_record(name, None);
    }
    ok()
}

/// The module instance `name` and whether it runs; `Err` is the answer to give.
fn module(d: &Daemon, name: &str) -> Result<Option<Arc<ModuleInstance>>, Box<Response>> {
    if !declared(d, name) {
        return Err(Box::new(not_declared(name)));
    }
    Ok(d.inner.modules.get(name).cloned())
}

fn running(d: &Daemon, name: &str) -> bool {
    d.engine().get(name).is_some_and(|i| i.state.running())
}

fn not_running(name: &str) -> Response {
    failed(
        format!("instance {name:?} is not running — boot it first"),
        "",
    )
}

/// Its module's data actions, as the wire shows them.
pub(crate) fn action_views(m: &doze_module_proto::msg::hello::Manifest) -> Vec<ActionView> {
    m.actions
        .iter()
        .map(|a| ActionView {
            id: a.id.clone(),
            label: a.label.clone(),
            kind: a.kind.clone(),
            destructive: a.destructive,
            input_hint: a.input_hint.clone().unwrap_or_default(),
        })
        .collect()
}

/// Go's `Resources`: a module instance's resources with a status line, and its module's data actions. Empty for an
/// instance whose module offers none (or that is not a module's); an error when it does not run.
pub(crate) async fn resources(d: &Daemon, name: &str) -> Response {
    let mi = match module(d, name) {
        Ok(Some(mi)) => mi,
        Ok(None) => return ok(),
        Err(r) => return *r,
    };
    match mi.has_admin().await {
        Ok(true) => {}
        Ok(false) => return ok(),
        Err(err) => return failed(err, ""),
    }
    if !running(d, name) {
        return not_running(name);
    }
    let ctx = d.engine().ctx_for(name);
    let res = match mi.resources(&ctx).await {
        Ok(r) => r,
        Err(err) => return failed(err, ""),
    };
    let actions = mi
        .manifest()
        .await
        .map(|m| action_views(&m))
        .unwrap_or_default();
    Response {
        ok: true,
        resources: res
            .into_iter()
            .map(|r| ResourceView {
                kind: r.kind,
                name: r.name,
                status: r.status,
                info: r.info,
            })
            .collect(),
        actions,
        ..Response::default()
    }
}

/// Go's `Admin`: runs data action `action` on `resource` of a running module instance; what the module says it did.
pub(crate) async fn admin(d: &Daemon, req: &doze_control::Request) -> Response {
    let name = req.db.as_str();
    let mi = match module(d, name) {
        Ok(mi) => mi,
        Err(r) => return *r,
    };
    let has = match &mi {
        Some(mi) => mi.has_admin().await.unwrap_or(false),
        None => false,
    };
    let Some(mi) = mi.filter(|_| has) else {
        let engine = d
            .inner
            .labels
            .get(name)
            .map(|l| l.engine.clone())
            .unwrap_or_default();
        return failed(
            format!("instance {name:?} ({engine}) has no data actions"),
            "",
        );
    };
    if !running(d, name) {
        return not_running(name);
    }
    let ctx = d.engine().ctx_for(name);
    let input = (!req.input.is_empty()).then(|| req.input.clone());
    match mi.run_action(&ctx, &req.action, &req.resource, input).await {
        Ok(result) => Response {
            ok: true,
            result,
            ..Response::default()
        },
        Err(err) => failed(err, ""),
    }
}
