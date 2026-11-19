//! The half that touches the machine: claim, listen, route, and the grant.

use crate::plan::{Addressing, InstancePlan, ListenerPlan, Plan};
use doze_module_proto::msg::instance::{Address, Granted};
use doze_names::{Forwarding, Lease, Name, Registry, Tier};
use std::collections::BTreeMap;
use std::net::{SocketAddrV4, TcpListener};

/// What went wrong binding one instance, and which.
#[derive(Debug)]
pub struct Error {
    pub instance: String,
    pub source: doze_names::Error,
    /// The address it was listening on when that failed (`127.0.0.1:5432`, `db.shop.doze:5432`).
    pub at: Option<Box<str>>,
}

/// A failure inside one instance's binding, before it is said whose.
struct Failed {
    source: doze_names::Error,
    at: Option<Box<str>>,
}

impl From<doze_names::Error> for Failed {
    fn from(source: doze_names::Error) -> Failed {
        Failed { source, at: None }
    }
}

impl From<std::io::Error> for Failed {
    fn from(e: std::io::Error) -> Failed {
        Failed::from(doze_names::Error::from(e))
    }
}

/// `r` with the address it was listening on, for the message.
fn listening<T>(r: Result<T, impl Into<doze_names::Error>>, at: String) -> Result<T, Failed> {
    r.map_err(|e| Failed {
        source: e.into(),
        at: Some(at.into()),
    })
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
            doze_names::Error::Io(io) if self.at.is_some() => write!(
                f,
                "{} cannot listen on {}: {io}",
                self.instance,
                self.at.as_deref().unwrap_or_default()
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
    /// Where clients reach it: the name's address and the public port (the socket itself may be a private port of
    /// the network daemon's interface, where addresses are translated).
    pub public: std::net::SocketAddr,
    pub granted: Granted,
    _forwarding: Option<Forwarding>,
}

/// One instance, bound.
#[derive(Debug)]
pub struct BoundInstance {
    /// Empty when names are off.
    pub host: String,
    pub listeners: BTreeMap<String, BoundListener>,
    /// The claim on its name (none when names are off): what more names for the same listeners (an app's own
    /// domains) are twinned onto. Released with the [`Bound`] it is in.
    pub lease: Option<Lease>,
    /// Route names another program holds, said as "<host> is held by <owner> (pid <n>)": the instance runs without
    /// them (its own name held is an error instead).
    pub held: Vec<String>,
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
        let fail = |f: Failed| Error {
            instance: i.name.clone(),
            source: f.source,
            at: f.at,
        };
        let inst = match (&i.host, plan.addressing) {
            (Some(host), Addressing::Own) => {
                bind_named(i, host, reg, None, backend, &mut bound.leases).map_err(fail)?
            }
            (Some(host), Addressing::Shared) => {
                let at = Some(crate::LOOPBACK);
                bind_named(i, host, reg, at, backend, &mut bound.leases).map_err(fail)?
            }
            _ => bind_loopback(i, backend).map_err(fail)?,
        };
        bound.instances.insert(i.name.clone(), inst);
    }
    Ok(bound)
}

fn granted(i: &InstancePlan, l: &ListenerPlan, backend: Address) -> Granted {
    i.granted(l, backend)
}

fn bind_loopback(
    i: &InstancePlan,
    backend: &dyn Fn(&str, &ListenerPlan) -> Address,
) -> Result<BoundInstance, Failed> {
    let mut listeners = BTreeMap::new();
    for l in &i.listeners {
        let at = SocketAddrV4::new(crate::LOOPBACK, l.port);
        let socket = listening(TcpListener::bind(at), at.to_string())?;
        let granted = granted(i, l, backend(&i.name, l));
        let bl = BoundListener {
            public: socket.local_addr()?,
            socket,
            granted,
            _forwarding: None,
        };
        listeners.insert(l.name.clone(), bl);
    }
    let host = String::new();
    Ok(BoundInstance {
        host,
        listeners,
        lease: None,
        held: Vec::new(),
    })
}

/// Claims `host` (at `at`, or at an address of its own) and listens on it.
fn bind_named(
    i: &InstancePlan,
    host: &str,
    reg: &Registry,
    at: Option<std::net::Ipv4Addr>,
    backend: &dyn Fn(&str, &ListenerPlan) -> Address,
    leases: &mut Vec<Lease>,
) -> Result<BoundInstance, Failed> {
    let qualified = |host: &str| Name {
        host: host.to_string(),
        tier: Tier::Qualified,
    };
    let lease = match at {
        Some(ip) => reg.claim_at(&qualified(host), ip)?,
        None => reg.claim(&qualified(host))?,
    };
    leases.push(lease.clone());
    let mut listeners = BTreeMap::new();
    let mut held = Vec::new();
    for l in &i.listeners {
        let opened = listening(lease.listen(l.port), i.addr(l))?;
        let public = opened.addr();
        let (socket, forwarding) = opened.into_parts();
        // The front door dials what the proxy really listens on: the name's address where it is bound, the daemon's
        // interface and the private port where it is translated.
        let target = socket.local_addr()?.to_string();
        for r in &l.routes {
            if r.twin {
                match lease.twin(&qualified(&r.host)) {
                    Ok(twin) => {
                        twin.route(&target)?;
                        leases.push(twin);
                    }
                    Err(doze_names::Error::Held { host, pid, owner }) => {
                        held.push(format!("{host} is held by {owner} (pid {pid})"));
                    }
                    Err(e) => return Err(e.into()),
                }
            } else {
                lease.route(&target)?;
            }
        }
        let granted = granted(i, l, backend(&i.name, l));
        let bl = BoundListener {
            socket,
            public,
            granted,
            _forwarding: forwarding,
        };
        listeners.insert(l.name.clone(), bl);
    }
    let host = host.to_string();
    Ok(BoundInstance {
        host,
        listeners,
        lease: Some(lease),
        held,
    })
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
