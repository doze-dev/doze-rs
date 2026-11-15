//! What runs a module instance: the engine's [`ModuleInstance`] over the catalog's pool, given the instance as its
//! module sees it (its block as the protocol's value tree, its granted main listener) and the lock's engine pins.
//!
//! The main listener's backend (PLAN.md D13) is a socket file in the instance's run folder, named as the module's
//! manifest names it (`backend_socket`, `{port}` filled in), where the system has sockets and the engine can listen on
//! one and the path fits a socket address; a loopback port chosen when the daemon starts otherwise. The daemon's front ([`crate::front`]) forwards the
//! declared address to it, waking it first.

use crate::catalog::{self, Catalog};
use doze_config::Instance;
use doze_engine::modules::{ANY_LOOPBACK, ModuleInstance};
use doze_engine::stack::{BoxFuture, PlanCtx, Planned, Planner};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::{Address, Instance as ModInstance};
use doze_module_proto::network::MAIN;
use std::collections::BTreeMap;
use std::sync::Arc;

/// The planner of module instance `i`, and the instance itself for its data, console and client (`None` with a
/// planner that says why it cannot run when its module was not found).
pub fn module(
    cat: &Arc<Catalog>,
    i: &Instance,
    target: &str,
    run_dir: &std::path::Path,
) -> (Arc<dyn Planner>, Option<Arc<ModuleInstance>>) {
    let Some(m) = cat.manifest_for(&i.ty, &i.version) else {
        let why = doze_config::modules::Modules::lookup_error(cat.as_ref(), &i.ty)
            .unwrap_or_else(|| format!("no module for {:?}", i.ty));
        return (Arc::new(NoModule { why }), None);
    };
    let addr = doze_config::instance_addr(i, cat.as_ref()).unwrap_or_default();
    let tcp_only = as_module_sees_it(cat, i).is_some_and(|mi| cat.main_is_tcp(&mi));
    let backend = backend(&addr, &i.name, &m, tcp_only, run_dir);
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
        listeners: BTreeMap::from([(MAIN.to_string(), catalog::granted(addr, backend))]),
        deps: Vec::new(),
        toolchain: None,
    };
    let (structured, supervised) = (m.caps & caps::OBJECTS != 0, m.supervised);
    let lock = cat.lock_path().to_path_buf();
    let sink: doze_engine::modules::PinSink =
        Arc::new(move |pins| catalog::record_pins(&lock, pins));
    let mi = Arc::new(
        ModuleInstance::new(cat.pool.clone(), base, cat.pins(), target, structured)
            .supervised(supervised)
            .pool_key(&cat.key(&i.ty, &i.version))
            .with_pin_sink(sink),
    );
    (mi.clone(), Some(mi))
}

/// Where the engine listens for the main listener: the declared socket itself for `listen = "unix:…"` (nothing fronts
/// it); else a socket file in its run folder, named as the manifest says, where the system has sockets, the engine can
/// listen on one and the path fits; else a loopback port chosen at each start ([`ANY_LOOPBACK`]; never the declared
/// address, which the front holds: it would dial itself).
pub fn backend(
    addr: &str,
    name: &str,
    m: &doze_module_proto::msg::hello::Manifest,
    tcp_only: bool,
    run_dir: &std::path::Path,
) -> Address {
    if let Some(p) = addr.strip_prefix("unix:") {
        return Address::Unix(p.to_string());
    }
    let port = addr
        .rsplit_once(':')
        .map(|(_, p)| p)
        .filter(|p| p.parse::<u16>().is_ok())
        .map_or_else(|| m.default_port.to_string(), str::to_string);
    let socket = m.backend_socket.as_ref().map(|file| {
        run_dir
            .join(name)
            .join(file.replace("{port}", &port))
            .to_string_lossy()
            .into_owned()
    });
    match socket {
        Some(p) if cfg!(unix) && !tcp_only && p.len() < SOCKET_PATH_MAX => Address::Unix(p),
        _ => Address::Tcp(ANY_LOOPBACK.into()),
    }
}

/// The instance as its module sees it once planned (for what it publishes), if it is a module instance.
pub fn as_module_sees_it(cat: &Catalog, i: &Instance) -> Option<ModInstance> {
    let m = cat.manifest_for(&i.ty, &i.version)?;
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

/// The longest socket path every Unix binds (macOS's `sun_path` holds 104 bytes with its nul); a longer one gets a
/// loopback port instead.
const SOCKET_PATH_MAX: usize = 104;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn manifest(socket: Option<&str>) -> doze_module_proto::msg::hello::Manifest {
        let mut m = crate::catalog::tests::manifest(&["17"]);
        m.backend_socket = socket.map(String::from);
        m
    }

    #[test]
    fn the_backend_is_a_socket_where_one_fits_else_a_loopback_port() {
        let m = manifest(Some(".s.PGSQL.{port}"));
        let short = Path::new("/tmp/r");
        let got = backend("127.0.0.1:15432", "db", &m, false, short);
        if cfg!(unix) {
            assert_eq!(got, Address::Unix("/tmp/r/db/.s.PGSQL.15432".into()));
        } else {
            assert_eq!(got, Address::Tcp(ANY_LOOPBACK.into()));
        }
        let long = Path::new("/tmp").join("x".repeat(100));
        let tcp = |a: Address| a == Address::Tcp(ANY_LOOPBACK.into());
        assert!(
            tcp(backend("127.0.0.1:15432", "db", &m, false, &long)),
            "too long for a socket address"
        );
        assert!(
            tcp(backend("127.0.0.1:15432", "db", &m, true, short)),
            "an engine that cannot listen on a socket"
        );
        assert!(
            tcp(backend(
                "127.0.0.1:15432",
                "db",
                &manifest(None),
                false,
                short
            )),
            "no socket declared"
        );
    }

    #[test]
    fn a_declared_socket_is_the_backend_and_a_port_that_is_no_port_takes_the_default() {
        let m = manifest(Some(".s.PGSQL.{port}"));
        assert_eq!(
            backend("unix:/tmp/pg.sock", "db", &m, false, Path::new("/tmp/r")),
            Address::Unix("/tmp/pg.sock".into())
        );
        if cfg!(unix) {
            assert_eq!(
                backend("db.local", "db", &m, false, Path::new("/tmp/r")),
                Address::Unix("/tmp/r/db/.s.PGSQL.5432".into())
            );
        }
    }
}
