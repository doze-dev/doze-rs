//! The stack as doze.hcl declares it, for when no daemon runs it: the instances in the config's order, each asleep,
//! with the address it will be reached at (Go `statusData` and `endpointValue` with no live view). The rows draw the
//! way the daemon's do, so `status` has one renderer whether the daemon is up or not.

use doze_config::{Config, Instance, Kind, Spec};
use doze_control::InstanceView;

/// Every declared instance, asleep.
pub(crate) fn views(cfg: &Config) -> Vec<InstanceView> {
    cfg.instances.iter().map(|i| view(cfg, i)).collect()
}

/// The names doze.hcl declares, in its order.
pub(crate) fn names(cfg: &Config) -> Vec<String> {
    cfg.instances.iter().map(|i| i.name.clone()).collect()
}

fn view(cfg: &Config, i: &Instance) -> InstanceView {
    let (endpoint, domain) = address(cfg, i);
    InstanceView {
        name: i.name.clone(),
        kind: i.kind().as_str().to_owned(),
        engine: i.ty.clone(),
        version: i.version.clone(),
        state: String::new(),
        endpoint,
        domain,
        needs: i.deps.iter().map(|d| d.name.clone()).collect(),
        declared: true,
        paused: !i.enabled,
        ..InstanceView::default()
    }
}

/// Where a client will reach it, and its name in the zone (Go `statusData`): an app doze fronts by name is reached
/// at the name alone; otherwise the declared address, named when the stack has names on (never for a socket).
fn address(cfg: &Config, i: &Instance) -> (String, String) {
    let names = cfg.defaults.names;
    if names && forwarded(i) {
        return (cfg.domain_for(&i.name), cfg.domain_for(&i.name));
    }
    let endpoint = doze_config::instance_addr(i, &doze_config::NoModules).unwrap_or_default();
    let domain = if names && !endpoint.is_empty() && !endpoint.starts_with("unix:") {
        cfg.domain_for(&i.name)
    } else {
        String::new()
    };
    (endpoint, domain)
}

/// An app (or service, or task) that declares a port and is not `direct`: doze serves it on port 80 of its name
/// (Go's `ForwardPort`).
fn forwarded(i: &Instance) -> bool {
    matches!(&i.spec, Spec::Command(c) if c.proxied()) && i.kind() != Kind::Module
}
