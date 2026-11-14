//! What runs a module instance: the engine's [`ModuleInstance`] over the catalog's pool, given the instance as its
//! module sees it (its block as the protocol's value tree, its granted main listener) and the lock's engine pins.
//!
//! Until D2's listeners, the main listener's backend is a loopback port chosen when the daemon starts: the engine
//! listens there and the daemon's front ([`crate::front`]) forwards the declared address to it, waking it first.

use crate::catalog::{self, Catalog};
use doze_config::Instance;
use doze_engine::modules::ModuleInstance;
use doze_engine::stack::{BoxFuture, PlanCtx, Planned, Planner};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::{Address, Instance as ModInstance};
use doze_module_proto::network::MAIN;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The planner of module instance `i`; one that says why it cannot run when its module was not found.
pub fn module(cat: &Arc<Catalog>, i: &Instance, target: &str) -> Arc<dyn Planner> {
    let Some(m) = cat.manifest(&i.ty) else {
        let why = doze_config::modules::Modules::lookup_error(cat.as_ref(), &i.ty)
            .unwrap_or_else(|| format!("no module for {:?}", i.ty));
        return Arc::new(NoModule { why });
    };
    let addr = doze_config::instance_addr(i, cat.as_ref()).unwrap_or_default();
    let backend = free_loopback().map_or_else(|| addr.clone(), |p| format!("127.0.0.1:{p}"));
    let spec = match &i.spec {
        doze_config::Spec::Module(b) => crate::spec_tree::tree(b, &m.schema),
        _ => crate::spec_tree::tree(&Default::default(), &m.schema),
    };
    let base = ModInstance {
        name: i.name.clone(),
        engine: i.ty.clone(),
        version: i.version.clone(),
        data_dir: String::new(),
        socket_dir: String::new(),
        spec,
        host: String::new(),
        listeners: BTreeMap::from([(
            MAIN.to_string(),
            catalog::granted(addr, Address::Tcp(backend)),
        )]),
        deps: Vec::new(),
        toolchain: None,
    };
    let structured = m.caps & caps::OBJECTS != 0;
    Arc::new(ModuleInstance::new(
        cat.pool.clone(),
        base,
        cat.pins(&i.ty),
        target,
        structured,
    ))
}

/// The instance as its module sees it once planned (for what it publishes), if it is a module instance.
pub fn as_module_sees_it(cat: &Catalog, i: &Instance) -> Option<ModInstance> {
    let m = cat.manifest(&i.ty)?;
    let addr = doze_config::instance_addr(i, cat).ok()?;
    let ep = match addr.strip_prefix("unix:") {
        Some(p) => doze_config::modules::Endpoint::Unix(p.to_string()),
        None => doze_config::modules::Endpoint::Tcp(addr.clone()),
    };
    let empty = doze_config::model::ModuleBody::default();
    let body = match &i.spec {
        doze_config::Spec::Module(b) => b,
        _ => &empty,
    };
    let call = doze_config::modules::Call {
        ty: &i.ty,
        name: &i.name,
        version: &i.version,
        endpoint: &ep,
        body,
    };
    Some(catalog::instance_for(&call, &m))
}

/// A loopback port no one holds right now.
fn free_loopback() -> Option<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    l.local_addr().ok().map(|a| a.port())
}

/// A module instance this daemon cannot run: its module could not be had.
#[derive(Debug)]
pub struct NoModule {
    pub why: String,
}

impl Planner for NoModule {
    fn plan<'a>(&'a self, _ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>> {
        let why = self.why.clone();
        Box::pin(async move { Err(why) })
    }
}
