//! The half that touches the machine: claim, listen, route, and the grant.

use crate::plan::{InstancePlan, ListenerPlan, Plan};
use doze_module_proto::msg::instance::{Address, Granted};
use doze_names::{Forwarding, Lease, Name, Registry, Tier};
use std::collections::BTreeMap;
use std::net::{SocketAddrV4, TcpListener};

/// What went wrong binding one instance, and which.
#[derive(Debug)]
pub struct Error {
    pub instance: String,
    pub source: doze_names::Error,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            doze_names::Error::Held { host, pid, owner } => write!(
                f,
                "{}: {host} is already in use on this machine by {owner} (pid {pid}): stop it, or rename this stack \
                 or service",
                self.instance
            ),
            e => write!(f, "{}: {e}", self.instance),
        }
    }
}

impl std::error::Error for Error {}

/// One listener, bound: the socket the core's proxy accepts on, and what the module is told.
#[derive(Debug)]
pub struct BoundListener {
    pub socket: TcpListener,
    pub granted: Granted,
    _forwarding: Option<Forwarding>,
}

/// One instance, bound.
#[derive(Debug)]
pub struct BoundInstance {
    /// Empty when names are off.
    pub host: String,
    pub listeners: BTreeMap<String, BoundListener>,
}

impl BoundInstance {
    /// The grant, as protocol 2's `Instance::listeners` carries it.
    pub fn grants(&self) -> BTreeMap<String, Granted> {
        self.listeners
            .iter()
            .map(|(k, l)| (k.clone(), l.granted.clone()))
            .collect()
    }
}

/// A stack, bound. Dropping it closes every port and releases every name (twins first).
#[derive(Debug)]
pub struct Bound {
    pub instances: BTreeMap<String, BoundInstance>,
    leases: Vec<Lease>,
}

impl Drop for Bound {
    fn drop(&mut self) {
        self.instances.clear();
        for lease in self.leases.iter().rev() {
            let _ = lease.release();
        }
    }
}

/// Carries out `plan` against `reg`. `backend` says where the engine itself listens for each listener (a socket in
/// the instance's directory, or a loopback port): the proxy's other end, which this crate does not choose.
pub fn bind(
    plan: &Plan,
    reg: &Registry,
    backend: &dyn Fn(&str, &ListenerPlan) -> Address,
) -> Result<Bound, Error> {
    let mut bound = Bound {
        instances: BTreeMap::new(),
        leases: Vec::new(),
    };
    for i in &plan.instances {
        let fail = |source| Error {
            instance: i.name.clone(),
            source,
        };
        let inst = match &i.host {
            Some(host) => bind_named(i, host, reg, backend, &mut bound.leases).map_err(fail)?,
            None => bind_loopback(i, backend).map_err(fail)?,
        };
        bound.instances.insert(i.name.clone(), inst);
    }
    Ok(bound)
}

fn granted(i: &InstancePlan, l: &ListenerPlan, backend: Address) -> Granted {
    Granted {
        addr: i.addr(l),
        url: i.url(l),
        hosts: l.routes.iter().map(|r| r.host.clone()).collect(),
        backend,
    }
}

fn bind_loopback(
    i: &InstancePlan,
    backend: &dyn Fn(&str, &ListenerPlan) -> Address,
) -> Result<BoundInstance, doze_names::Error> {
    let mut listeners = BTreeMap::new();
    for l in &i.listeners {
        let socket = TcpListener::bind(SocketAddrV4::new(crate::LOOPBACK, l.port))?;
        let granted = granted(i, l, backend(&i.name, l));
        let bl = BoundListener {
            socket,
            granted,
            _forwarding: None,
        };
        listeners.insert(l.name.clone(), bl);
    }
    let host = String::new();
    Ok(BoundInstance { host, listeners })
}

fn bind_named(
    i: &InstancePlan,
    host: &str,
    reg: &Registry,
    backend: &dyn Fn(&str, &ListenerPlan) -> Address,
    leases: &mut Vec<Lease>,
) -> Result<BoundInstance, doze_names::Error> {
    let qualified = |host: &str| Name {
        host: host.to_string(),
        tier: Tier::Qualified,
    };
    let lease = reg.claim(&qualified(host))?;
    leases.push(lease.clone());
    let mut listeners = BTreeMap::new();
    for l in &i.listeners {
        let (socket, forwarding) = lease.listen(l.port)?.into_parts();
        // The front door dials what the proxy really listens on: the name's address where it is bound, the daemon's
        // interface and the private port where it is translated.
        let target = socket.local_addr()?.to_string();
        for r in &l.routes {
            if r.twin {
                let twin = lease.twin(&qualified(&r.host))?;
                twin.route(&target)?;
                leases.push(twin);
            } else {
                lease.route(&target)?;
            }
        }
        let granted = granted(i, l, backend(&i.name, l));
        let bl = BoundListener {
            socket,
            granted,
            _forwarding: forwarding,
        };
        listeners.insert(l.name.clone(), bl);
    }
    let host = host.to_string();
    Ok(BoundInstance { host, listeners })
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
