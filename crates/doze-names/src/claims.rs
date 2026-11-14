//! Claiming names and stacks: first-come, with the loser told who holds it.

use crate::pool::address_for;
use crate::registry::{Entry, Registry};
use crate::{Error, Name, Tier, label};
use std::collections::BTreeSet;
use std::net::Ipv4Addr;

/// The entry recording which project a stack name belongs to; like the other bookkeeping keys, not an in-zone name.
const STACK_PREFIX: &str = "_stack.";

/// A claimed name. [`Lease::release`] drops it at once; the serving peer would prune it anyway once this process
/// exits, so releasing is a courtesy.
#[derive(Debug, Clone)]
pub struct Lease {
    pub name: Name,
    pub ip: Ipv4Addr,
    pub(crate) reg: Registry,
}

impl Registry {
    /// Registers `name` to this process and returns the address it resolves to. A name another live process holds
    /// is refused with [`Error::Held`], naming the holder.
    pub fn claim(&self, name: &Name) -> Result<Lease, Error> {
        self.claim_inner(name, None, false)
    }

    /// Registers `name` at an address the caller picked. An address from the pool is refused when another live name
    /// resolves there (two names on one address send each other's clients to whichever bound the port first); one
    /// outside it, such as 127.0.0.1 on a machine with no setup, is shared by design and never refused.
    pub fn claim_at(&self, name: &Name, ip: Ipv4Addr) -> Result<Lease, Error> {
        self.claim_inner(name, Some(ip), false)
    }

    fn claim_inner(
        &self,
        name: &Name,
        want: Option<Ipv4Addr>,
        shared: bool,
    ) -> Result<Lease, Error> {
        let pid = i64::from(self.pid);
        self.update(|m| {
            if let Some(cur) = m.get(&name.host)
                && cur.pid != pid
            {
                return Err(Error::Held {
                    host: name.host.clone(),
                    pid: holder_pid(cur),
                    owner: cur.owner.clone(),
                });
            }
            let taken: BTreeSet<Ipv4Addr> = m
                .iter()
                .filter(|(host, _)| **host != name.host)
                .filter_map(|(_, e)| e.addr())
                .collect();
            let ip = match want {
                None => address_for(self.net, name, &taken)?,
                Some(ip) => {
                    if self.net.in_pool(ip) && taken.contains(&ip) {
                        let clash = m.iter().find(|(host, e)| {
                            **host != name.host && e.addr() == Some(ip) && !(shared && e.pid == pid)
                        });
                        if let Some((host, e)) = clash {
                            let (host, owner) = (host.clone(), e.owner.clone());
                            return Err(Error::AddrTaken {
                                ip: ip.to_string(),
                                host,
                                pid: holder_pid(e),
                                owner,
                            });
                        }
                    }
                    ip
                }
            };
            // Keep a route this process already published, so claiming again (on every topology change) does not
            // drop it from the front door; keep its ports only at the same address.
            let mut entry = Entry {
                ip: ip.to_string(),
                pid,
                owner: self.owner.clone(),
                tier: name.tier.as_str().to_string(),
                ..Entry::default()
            };
            if let Some(cur) = m.get(&name.host).filter(|c| c.pid == pid) {
                entry.target = cur.target.clone();
                entry.cert = cur.cert.clone();
                entry.key = cur.key.clone();
                if cur.ip == entry.ip {
                    entry.ports = cur.ports.clone();
                }
            }
            m.insert(name.host.clone(), entry);
            Ok(Lease {
                name: name.clone(),
                ip,
                reg: self.clone(),
            })
        })
    }

    /// Records that the stack `name` belongs to the project in `dir` while this process lives. Every name in a stack
    /// is `<service>.<name>.doze`, so two projects with one stack name would answer for each other's services: the
    /// second is refused and told who has it. The same project claiming again (a daemon restarting) takes it over.
    pub fn claim_stack(&self, name: &str, dir: &str) -> Result<StackLease, Error> {
        let key = format!("{STACK_PREFIX}{}", label(name));
        let pid = i64::from(self.pid);
        self.update(|m| {
            if let Some(cur) = m.get(&key)
                && cur.pid != pid
                && cur.target != dir
            {
                let (stack, dir) = (name.to_string(), cur.target.clone());
                return Err(Error::StackTaken {
                    stack,
                    dir,
                    pid: holder_pid(cur),
                });
            }
            let entry = Entry {
                pid,
                owner: self.owner.clone(),
                target: dir.to_string(),
                ..Entry::default()
            };
            m.insert(key.clone(), entry);
            Ok(())
        })?;
        Ok(StackLease {
            key,
            reg: self.clone(),
        })
    }

    /// Drops this process's entry under `key`, if it is this process's.
    pub(crate) fn release_key(&self, key: &str) -> Result<(), Error> {
        let pid = i64::from(self.pid);
        self.update(|m| {
            if m.get(key).is_some_and(|e| e.pid == pid) {
                m.remove(key);
            }
            Ok(())
        })
    }
}

fn holder_pid(e: &Entry) -> u32 {
    u32::try_from(e.pid).unwrap_or(0)
}

impl Lease {
    /// A second name at this name's address, for a host the same listeners answer (`sync-aws.shop.doze` beside
    /// `aws.shop.doze`). The one way two names share a pool address, and it has to be asked for.
    pub fn twin(&self, name: &Name) -> Result<Lease, Error> {
        self.reg.claim_inner(name, Some(self.ip), true)
    }

    /// Drops the claim, and its translated ports with it.
    pub fn release(&self) -> Result<(), Error> {
        self.reg.release_key(&self.name.host)?;
        if self.translated() {
            self.forget_ports()?;
        }
        Ok(())
    }

    /// The registry this lease is in.
    pub fn registry(&self) -> &Registry {
        &self.reg
    }
}

/// A claimed stack name; [`StackLease::release`] gives it up.
#[derive(Debug, Clone)]
pub struct StackLease {
    key: String,
    reg: Registry,
}

impl StackLease {
    pub fn release(&self) -> Result<(), Error> {
        self.reg.release_key(&self.key)
    }
}

impl Name {
    /// The registry name for a domain outside the zone (`dev.acme.com`).
    pub fn domain(host: &str) -> Name {
        Name {
            host: crate::normalize(host),
            tier: Tier::Domain,
        }
    }
}
