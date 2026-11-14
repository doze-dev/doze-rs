//! The pure half: from the stack's declarations to where everything will be.

use doze_module_proto::msg::instance::Listener;
use doze_module_proto::network::{self, MAIN};
use doze_names::Name;
use std::collections::BTreeMap;

/// One instance as the configuration has it.
#[derive(Debug, Clone)]
pub struct Decl {
    /// The instance's name in the stack (`db`, `events`).
    pub name: String,
    /// The person's `port = …`; `None` takes the main listener's conventional port.
    pub port: Option<u16>,
    /// `routes = false` turns every route off for this instance (principle 7: the person can always say no).
    pub routes: bool,
    /// What the module declared (protocol 2's `network`); empty means one TCP listener, `main`.
    pub listeners: Vec<Listener>,
    /// The main listener's conventional port, from the module, when its `main` says 0.
    pub default_port: u16,
}

/// A host a route gives, and whether it is the instance's own name or a twin claimed beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteHost {
    pub host: String,
    /// A child (`registry.events.shop.doze`) or sibling (`sync-aws.shop.doze`): claimed as a twin at the same address.
    pub twin: bool,
}

/// One listener, placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerPlan {
    pub name: String,
    /// The port clients use, on the instance's address.
    pub port: u16,
    pub http: bool,
    /// The backend must be a loopback port, not a socket (D13).
    pub tcp_backend: bool,
    /// Hosts the front door sends to this listener; none when names are off or no route was asked for.
    pub routes: Vec<RouteHost>,
}

/// One instance, placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstancePlan {
    pub name: String,
    /// `<instance>.<stack>.doze`; `None` when names are off.
    pub host: Option<String>,
    pub listeners: Vec<ListenerPlan>,
}

impl InstancePlan {
    /// The URL a person or a client uses for a listener, as the grant will say it, before anything is bound: fronted
    /// HTTP is port-less, other HTTP carries its port, TCP has none (its address is [`InstancePlan::addr`]).
    pub fn url(&self, l: &ListenerPlan) -> String {
        if !l.http {
            return String::new();
        }
        if let Some(r) = l.routes.first() {
            return format!("http://{}", r.host);
        }
        let host = self
            .host
            .clone()
            .unwrap_or_else(|| crate::LOOPBACK.to_string());
        format!("http://{host}:{}", l.port)
    }

    /// `host:port` for a listener: what a client connects to.
    pub fn addr(&self, l: &ListenerPlan) -> String {
        let host = self
            .host
            .clone()
            .unwrap_or_else(|| crate::LOOPBACK.to_string());
        format!("{host}:{}", l.port)
    }
}

/// The whole stack, placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub stack: String,
    /// Each instance on an address of its own (names on and the machine set up); else all on 127.0.0.1.
    pub names: bool,
    pub instances: Vec<InstancePlan>,
}

/// Places every instance of `stack`. `names` is whether names are on and the machine can give addresses (the caller
/// asks [`doze_names::pool_usable`]). Every problem is returned at once, each naming what is involved.
pub fn plan(stack: &str, decls: &[Decl], names: bool) -> Result<Plan, Vec<String>> {
    let mut errs = Vec::new();
    let mut instances = Vec::new();
    for d in decls {
        match place(stack, d, names) {
            Ok(p) => instances.push(p),
            Err(e) => errs.extend(e.into_iter().map(|e| format!("{}: {e}", d.name))),
        }
    }
    errs.extend(collisions(&instances, names));
    if errs.is_empty() {
        let stack = stack.to_string();
        Ok(Plan {
            stack,
            names,
            instances,
        })
    } else {
        Err(errs)
    }
}

fn main_only() -> Vec<Listener> {
    let main = Listener {
        name: MAIN.to_string(),
        port: 0,
        http: false,
        routes: Vec::new(),
        tcp: false,
    };
    vec![main]
}

fn place(stack: &str, d: &Decl, names: bool) -> Result<InstancePlan, Vec<String>> {
    let listeners = if d.listeners.is_empty() {
        main_only()
    } else {
        d.listeners.clone()
    };
    let errs = network::validate(&listeners);
    if !errs.is_empty() {
        return Err(errs);
    }
    let host = names.then(|| Name::qualified(&d.name, stack).host);
    let mut out = Vec::new();
    for l in &listeners {
        let port = match (l.name == MAIN, d.port) {
            (true, Some(p)) => p,
            (true, None) if l.port == 0 => d.default_port,
            _ => l.port,
        };
        if port == 0 {
            return Err(vec![
                "the main listener has no port: set `port = …`".to_string(),
            ]);
        }
        let routes = match (&host, d.routes) {
            (Some(h), true) => route_hosts(l, h)?,
            _ => Vec::new(),
        };
        let name = l.name.clone();
        out.push(ListenerPlan {
            name,
            port,
            http: l.http,
            tcp_backend: l.tcp,
            routes,
        });
    }
    // The person's port can land on another listener's conventional one.
    let mut seen = BTreeMap::new();
    for l in &out {
        if let Some(other) = seen.insert(l.port, l.name.as_str()) {
            let (a, b, p) = (other, l.name.as_str(), l.port);
            return Err(vec![format!("listeners {a:?} and {b:?} both use port {p}")]);
        }
    }
    let name = d.name.clone();
    Ok(InstancePlan {
        name,
        host,
        listeners: out,
    })
}

fn route_hosts(l: &Listener, host: &str) -> Result<Vec<RouteHost>, Vec<String>> {
    l.routes
        .iter()
        .map(|r| {
            let h = network::route_host(r, host).map_err(|e| vec![e])?;
            Ok(RouteHost {
                twin: h != host,
                host: h,
            })
        })
        .collect()
}

/// What two instances of one stack can both want: a host (a sibling route `sync-@` of `cloud` is `sync-cloud`'s own
/// name), or, without names, a port on 127.0.0.1.
fn collisions(instances: &[InstancePlan], names: bool) -> Vec<String> {
    let mut errs = Vec::new();
    let mut hosts: BTreeMap<String, String> = BTreeMap::new();
    for i in instances {
        if let Some(h) = &i.host {
            hosts.insert(h.clone(), format!("{:?}", i.name));
        }
    }
    let mut ports: BTreeMap<u16, String> = BTreeMap::new();
    for i in instances {
        for l in &i.listeners {
            for r in l.routes.iter().filter(|r| r.twin) {
                let what = format!("{:?}'s route to {}", i.name, r.host);
                if let Some(other) = hosts.insert(r.host.clone(), what.clone()) {
                    errs.push(format!(
                        "{other} and {what} are the same name: rename one of them"
                    ));
                }
            }
            if !names {
                let what = describe(i, l);
                if let Some(other) = ports.insert(l.port, what.clone()) {
                    errs.push(format!(
                        "{other} and {what} both use port {}, but services are not on addresses of their own here \
                         (names are off, or the machine is not set up: `doze dns-setup`); give them distinct ports",
                        l.port
                    ));
                }
            }
        }
    }
    errs
}

fn describe(i: &InstancePlan, l: &ListenerPlan) -> String {
    if l.name == MAIN {
        format!("{:?}", i.name)
    } else {
        format!("{:?} (its {} listener)", i.name, l.name)
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
pub(crate) mod tests;
