//! How a service listens on its name.
//!
//! On Linux and Windows a name's address is local and the service binds it. On macOS it is not (see `pool.rs`): the
//! service listens on a private port of the daemon's interface and registers which public port that stands for, and
//! the daemon translates (`nat.rs`). [`Lease::listen`] hides the difference and is the one call doze, doze-aws and
//! doze-kafka all make, so a service is reached at `<name>:<port>` without any of them knowing which system it is on.

use crate::ports::write_ports;
use crate::{Error, Lease, Registry, in_zone};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener};

impl Lease {
    /// Whether this name's address is one the daemon translates rather than one that can be bound.
    pub fn translated(&self) -> bool {
        self.reg.net.virtual_ && self.reg.net.in_pool(self.ip)
    }

    /// Where a process should listen to serve one of this name's ports, for a server this program starts but does
    /// not run in-process: pick a free port here, have the server listen on it, and [`Lease::forward`] the public port
    /// to it. Where the address is bound directly it is the name's own, and the server can simply take the public
    /// port.
    pub fn bind_addr(&self) -> Ipv4Addr {
        if self.translated() {
            self.reg.net.gateway()
        } else {
            self.ip
        }
    }

    /// Registers that this name's `public` port is served on `private`, at [`Lease::bind_addr`], replacing whatever
    /// it was forwarded to. Nothing to do where the address is bound directly.
    pub fn forward(&self, public: u16, private: u16) -> Result<(), Error> {
        if !self.translated() {
            return Ok(());
        }
        self.set_port(public, Some(private))
    }

    /// Drops a port registered with [`Lease::forward`].
    pub fn unforward(&self, public: u16) -> Result<(), Error> {
        if !self.translated() {
            return Ok(());
        }
        self.set_port(public, None)
    }

    /// A TCP listener for one of this name's ports: clients reach it at the name's address on `port`. The listener's
    /// [`Listener::addr`] is that public address, whatever it is really bound to; dropping it unregisters the port.
    pub fn listen(&self, port: u16) -> Result<Listener, Error> {
        let public = SocketAddr::V4(SocketAddrV4::new(self.ip, port));
        if !self.translated() {
            let socket = TcpListener::bind(public)?;
            return Ok(Listener {
                socket,
                addr: public,
                forwarding: None,
            });
        }
        let socket = TcpListener::bind((self.reg.net.gateway(), 0)).map_err(|why| {
            let host = self.name.host.clone();
            Error::NetworkDown { host, why }
        })?;
        let private = socket.local_addr()?.port();
        self.forward(port, private)?;
        let forwarding = Some(Forwarding {
            lease: self.clone(),
            port,
        });
        Ok(Listener {
            socket,
            addr: public,
            forwarding,
        })
    }

    fn set_port(&self, public: u16, private: Option<u16>) -> Result<(), Error> {
        if public == 0 || private == Some(0) {
            let host = &self.name.host;
            return Err(std::io::Error::other(format!("{host}: port 0 is not a port")).into());
        }
        let (host, pid) = (self.name.host.clone(), i64::from(self.reg.pid));
        let after = self.reg.update(|m| {
            let e = m
                .get_mut(&host)
                .filter(|e| e.pid == pid)
                .ok_or_else(|| Error::NotHeld(host.clone()))?;
            match private {
                Some(p) => e.ports.insert(public.to_string(), p),
                None => e.ports.remove(&public.to_string()),
            };
            Ok(e.ports.clone())
        })?;
        write_ports(
            self.reg.path(),
            &host,
            &self.ip.to_string(),
            self.reg.pid,
            &after,
        )
    }

    /// Drops this lease's ports from the ports file: the name was released.
    pub(crate) fn forget_ports(&self) -> Result<(), Error> {
        let none = BTreeMap::new();
        write_ports(self.reg.path(), &self.name.host, "", self.reg.pid, &none)
    }
}

/// A listener on one of a name's ports. Answers for the public address; a translated one takes its registration with
/// it when dropped.
#[derive(Debug)]
pub struct Listener {
    pub socket: TcpListener,
    addr: SocketAddr,
    forwarding: Option<Forwarding>,
}

impl Listener {
    /// The address clients use: the name's, on the public port.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The socket and the registration apart, for a server that wants its own listener type (tokio's): keep the
    /// [`Forwarding`] as long as the socket serves.
    pub fn into_parts(self) -> (TcpListener, Option<Forwarding>) {
        (self.socket, self.forwarding)
    }
}

/// A registered translated port, unregistered when dropped.
#[derive(Debug)]
pub struct Forwarding {
    lease: Lease,
    port: u16,
}

impl Drop for Forwarding {
    fn drop(&mut self) {
        let _ = self.lease.unforward(self.port);
    }
}

/// Whether this machine can put names on addresses of their own: the daemon's interface is up (macOS), or loopback
/// addresses past 127.0.0.1 can be bound (Linux, Windows). Without it every service shares 127.0.0.1 and needs a port of
/// its own.
pub fn pool_usable(net: crate::Net) -> bool {
    let probe = if net.virtual_ {
        net.gateway()
    } else {
        net.at(crate::pool::DYNAMIC_BASE)
    };
    TcpListener::bind((probe, 0)).is_ok()
}

impl Registry {
    /// A listener on `addr` for whichever of this process's names has that address: [`Lease::listen`] for a caller
    /// that carries addresses about and not leases. An address that is no name's (127.0.0.1 on a machine with no
    /// setup) is simply listened on.
    pub fn listen(&self, addr: SocketAddrV4) -> Result<Listener, Error> {
        let ip = *addr.ip();
        if !self.net.virtual_ || !self.net.in_pool(ip) {
            let socket = TcpListener::bind(addr)?;
            let addr = SocketAddr::V4(addr);
            return Ok(Listener {
                socket,
                addr,
                forwarding: None,
            });
        }
        let pid = i64::from(self.pid);
        let mine = self
            .snapshot()
            .into_iter()
            .find(|(host, e)| e.pid == pid && e.addr() == Some(ip) && in_zone(host));
        let (host, e) = mine.ok_or_else(|| Error::NotMine(addr.to_string()))?;
        let lease = Lease {
            name: crate::Name {
                tier: crate::Tier::parse(&e.tier),
                host,
            },
            ip,
            reg: self.clone(),
        };
        lease.listen(addr.port())
    }
}

#[cfg(test)]
#[path = "listen_tests.rs"]
mod tests;
