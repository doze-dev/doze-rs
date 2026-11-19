//! `add` and `remove` (Go's `daemon/dynamic.go`): an instance block wired into the running stack, and one taken out.
//!
//! A block is read alone through doze-config, as Go read it (references to other instances do not resolve, and the
//! error says to pass the value instead), and becomes what a block in doze.hcl becomes: the engine's service (module
//! instances planned through the catalog), its labels, its front. Something doze does not front and that runs by
//! itself (an app without a port, a supervised module) is started at once; a refused start undoes the add. A remove
//! is refused while another instance needs it (Go left the need dangling); `wipe` also deletes its data, sockets and
//! logs. Nothing here names a module.

use crate::catalog::Catalog;
use crate::front::Front;
use crate::{Daemon, entry};
use doze_config::{Instance, Kind, Spec};
use doze_control::{Response, fault};
use doze_engine::Cause;
use doze_engine::modules::ModuleInstance;
use doze_engine::stack::Service;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

/// What a daemon needs to add instances: the catalog that loaded its stack, where doze.hcl is, and its front.
#[derive(Debug)]
pub struct Live {
    catalog: Arc<Catalog>,
    config: String,
    version: String,
    env: BTreeMap<String, String>,
    front: Arc<Front>,
}

impl Live {
    /// `env` is read before this process's environment, as [`entry::assemble`]'s.
    pub fn new(
        catalog: Arc<Catalog>,
        config: &Path,
        version: &str,
        env: BTreeMap<String, String>,
        front: Arc<Front>,
    ) -> Live {
        Live {
            catalog,
            config: config.to_string_lossy().into_owned(),
            version: version.into(),
            env,
            front,
        }
    }
}

/// A block made ready to add.
struct Prepared {
    inst: Instance,
    service: Service,
    labels: crate::Labels,
    module: Option<Arc<ModuleInstance>>,
    supervised: bool,
    /// Where doze listens for it, if it fronts it.
    fronted: Option<crate::front::Fronted>,
}

fn failed(error: String, code: &str) -> Response {
    Response {
        ok: false,
        error,
        code: code.into(),
        ..Response::default()
    }
}

/// Go's `looksLikeUnresolvedRef`: a block read alone names an instance that is not in it.
fn unresolved_ref(err: &str) -> bool {
    [
        "Unknown variable",
        "Variables not allowed",
        "no such variable",
        "There is no variable named",
        "reference to undeclared",
    ]
    .iter()
    .any(|s| err.contains(s))
}

/// Reads `block` and builds what the engine and the views need (on a blocking thread: the catalog asks modules).
fn prepare(
    live: &Live,
    block: &str,
    declared: &[String],
    run_dir: &Path,
) -> Result<Prepared, String> {
    let cat = &live.catalog;
    cat.retry_failed();
    let cfg = entry::parse_block(block, &live.config, &live.version, cat.as_ref(), live.env.clone())
        .map_err(|e| {
            if unresolved_ref(&e) {
                format!("a live-added service can't reference other services yet — pass the literal value instead (look it up with the client's Instance/Status): {e}")
            } else {
                format!("decoding block: {e}")
            }
        })?;
    let [inst] = <[Instance; 1]>::try_from(cfg.instances).map_err(|all| {
        format!(
            "expected exactly one module or app block, got {}",
            all.len()
        )
    })?;
    if declared.contains(&inst.name) {
        return Err(format!("an instance named {:?} already exists", inst.name));
    }
    let supervised = inst.kind() == Kind::Module
        && cat
            .manifest_for(&inst.ty, &inst.version)
            .is_some_and(|m| m.supervised);
    let fronted = entry::fronted_one(&inst, cat, supervised, run_dir);
    let zone = live.front.zone();
    let grant = match &fronted {
        Some(f) => {
            let plan = zone.plan(std::slice::from_ref(f))?;
            Some(zone.grant(&plan, f, run_dir))
        }
        None => None,
    };
    let module = std::cell::RefCell::new(None);
    let planner = |i: &Instance| {
        let (p, mi) =
            crate::planners::module(cat, i, &doze_fetch::triple(), run_dir, grant.as_ref());
        *module.borrow_mut() = mi;
        p
    };
    let wiring = doze_config::stack::Wiring {
        planner: &planner,
        extra_env: BTreeMap::new(),
    };
    let names: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
    let service = doze_config::stack::service(&inst, &wiring, &names);
    Ok(Prepared {
        labels: entry::label(&inst, cat, grant.as_ref()),
        inst,
        service,
        module: module.into_inner(),
        supervised,
        fronted,
    })
}

/// The `add` op: the new instance's view, or why it was not added.
pub(crate) async fn add(d: &Daemon, block: &str) -> Response {
    let Some(live) = d.inner.live.as_ref() else {
        return failed("this daemon cannot add instances".into(), "");
    };
    if block.trim().is_empty() {
        return failed("add needs a block".into(), "");
    }
    let e = d.engine();
    let stack = e.stack();
    // The engine's instances and the config's disabled ones (a name doze.hcl uses is taken either way).
    let mut declared: Vec<String> = stack.services.iter().map(|s| s.name.clone()).collect();
    declared.extend(d.labels().into_keys());
    declared.sort();
    declared.dedup();
    let run_dir = stack.run_dir();
    let (owned, text) = (d.clone(), block.to_owned());
    let prepared = tokio::task::spawn_blocking(move || {
        let live = owned.inner.live.as_ref().ok_or_else(String::new)?;
        prepare(live, &text, &declared, &run_dir)
    })
    .await
    .map_err(|e| e.to_string());
    let p = match prepared.and_then(|r| r) {
        Ok(p) => p,
        Err(err) => return failed(err, ""),
    };
    let name = p.inst.name.clone();
    let _held = match d.claim(&name) {
        Ok(c) => c,
        Err(err) => return failed(err, ""),
    };
    if let Err(err) = e.add(p.service) {
        return failed(err.to_string(), "");
    }
    crate::write(&d.inner.labels).insert(name.clone(), p.labels.clone());
    if let Some(mi) = p.module {
        crate::write(&d.inner.modules).insert(name.clone(), mi);
    }
    if let Err(err) = settle(d, &p.inst, &p.labels, p.supervised).await {
        undo(d, &name).await;
        return failed(err, "");
    }
    if let Some(f) = &p.fronted {
        if let Err(err) = live.front.add(f, &stack.run_dir()).await {
            undo(d, &name).await;
            return failed(err, "");
        }
        if let Some(l) = crate::write(&d.inner.labels).get_mut(&name) {
            if let Some(ep) = live.front.endpoint(&name).filter(|e| !e.is_empty()) {
                l.bind = ep.clone();
                l.endpoint = ep;
            }
            l.name_only = live.front.name_only(&name);
            l.name_held = live.front.zone().held(&name).unwrap_or_default();
        }
    } else if starts_by_itself(&p.inst, p.supervised)
        && let Err(err) = e.wake(&name, Cause::Command).await
    {
        undo(d, &name).await;
        return failed(format!("booting {name:?}: {err}"), "");
    }
    let labels = d.label(&name);
    let view = e
        .get(&name)
        .map(|i| crate::view::view(e, &i, labels.as_ref()));
    Response {
        ok: true,
        instances: view.into_iter().collect(),
        ..Response::default()
    }
}

/// What runs once added: enabled, not a task, not fronted (that is the caller's), and an app or service, or a module
/// its module supervises (Go's `Supervised`).
fn starts_by_itself(i: &Instance, supervised: bool) -> bool {
    i.enabled
        && match &i.spec {
            Spec::Command(_) => i.kind() != Kind::Task,
            _ => supervised,
        }
}

/// What a block in doze.hcl would be: a disabled one is out of the run (Go's config skipped it; here it is shown and
/// woken by nothing), and a module doze must front needs an address (Go: "needs a client-facing port").
async fn settle(
    d: &Daemon,
    inst: &Instance,
    labels: &crate::Labels,
    supervised: bool,
) -> Result<(), String> {
    if !inst.enabled {
        d.engine()
            .set_off(&inst.name, true)
            .await
            .map_err(|e| e.to_string())?;
    }
    let fronted_module = inst.kind() == Kind::Module && !supervised;
    if inst.enabled && fronted_module && labels.endpoint.is_empty() {
        return Err(format!(
            "engine {:?} needs a client-facing port — set one on the block",
            inst.ty
        ));
    }
    Ok(())
}

/// Takes back an add that could not finish; a remove the engine refuses is said in the daemon's log.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
async fn undo(d: &Daemon, name: &str) {
    if let Err(err) = d.engine().remove(name).await {
        eprintln!("doze daemon: taking back the add of {name:?}: {err}");
    }
    forget(d, name);
}

fn forget(d: &Daemon, name: &str) {
    if let Some(live) = d.inner.live.as_ref() {
        live.front.remove(name);
    }
    crate::write(&d.inner.labels).remove(name);
    crate::write(&d.inner.modules).remove(name);
}

/// The `remove` op: stops `name`, closes its front, forgets it; with `wipe`, deletes its data, sockets and logs.
pub(crate) async fn remove(d: &Daemon, name: &str, wipe: bool) -> Response {
    if name.is_empty() {
        return failed("remove needs an instance name".into(), "");
    }
    let e = d.engine();
    if e.get(name).is_none() {
        return failed(format!("no instance named {name:?}"), fault::NOT_FOUND);
    }
    let _held = match d.claim(name) {
        Ok(c) => c,
        Err(err) => return failed(err, ""),
    };
    if let Err(err) = e.remove(name).await {
        return failed(err.to_string(), "");
    }
    forget(d, name);
    if wipe {
        let stack = e.stack();
        for dir in [stack.cluster_dir(name), stack.run_dir().join(name)] {
            match std::fs::remove_dir_all(&dir) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    return failed(format!("removing {}: {err}", dir.display()), "");
                }
                _ => {}
            }
        }
        if let Err(err) = e.logs().remove(name) {
            return failed(format!("removing the logs of {name:?}: {err}"), "");
        }
    }
    Response {
        ok: true,
        ..Response::default()
    }
}

impl Live {
    pub(crate) fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The stack's doze.hcl.
    pub(crate) fn config(&self) -> &str {
        &self.config
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }
}
