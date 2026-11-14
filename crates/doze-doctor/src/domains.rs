//! A stack's own domains (`domains = ["dev.acme.com"]` on an app): routed to doze's resolver, served over https by an
//! authority this machine trusts, and stale routes nobody declares any more (Go doctor's `domains: …` lines, and what
//! `doze trust` sets up).
//!
//! Where the domains come from: the routes record (`routes.json`, shared with Go) keeps, for each domain, the project
//! folders whose daemon declared it when the stack last came up. Until the Rust core reads doze.hcl itself (the core
//! plan's H3), that record is the stack's list, and the report says so. A stack that never came up has none yet.

use crate::{Check, FIX_DOMAINS, FIX_ROTATE, Machine, State};
use doze_names::Registry;
use doze_names::ca::{Ca, ca_dir};
use doze_names::routes::Routes;
use doze_names::setup::trust::{TrustRequest, trust_status};
use std::path::{Path, PathBuf};

/// Where a stack's list of domains was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The routes record: what the stack's daemon declared when it last came up.
    Recorded,
}

/// One stack's domains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackDomains {
    /// The project folder, absolute with links resolved: the record keys projects by it (Go `projectDirOf`).
    pub dir: String,
    /// The route domains (a wildcard is folded onto the name under it).
    pub domains: Vec<String>,
    pub source: Source,
}

impl StackDomains {
    /// What the record under `home` holds for the project in `dir`.
    pub fn recorded(home: &Path, dir: &Path) -> StackDomains {
        let dir = key(dir);
        let routes = Routes::open(home);
        let domains = routes
            .recorded()
            .into_iter()
            .filter(|d| routes.claimants(d).contains(&dir))
            .collect();
        StackDomains {
            dir,
            domains,
            source: Source::Recorded,
        }
    }
}

/// A project folder as the record keys it.
pub fn key(dir: &Path) -> String {
    let abs = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| dir.to_path_buf(), |c| c.join(dir))
    };
    abs.canonicalize()
        .unwrap_or(abs)
        .to_string_lossy()
        .into_owned()
}

/// The authority as doctor and `--check` see it: looked at, never made.
#[derive(Debug)]
pub struct Authority {
    pub ca: Option<Ca>,
    /// It is there and cannot be used (unreadable, expiring): `doze trust --rotate` replaces it.
    pub broken: Option<String>,
}

impl Authority {
    pub fn probe(home: &Path) -> Authority {
        match Ca::probe(&ca_dir(home)) {
            Ok(ca) => Authority { ca, broken: None },
            Err(e) => Authority {
                ca: None,
                broken: Some(e.to_string()),
            },
        }
    }
}

/// The request that sets the stack's domains up: its routes, the authority when there is one, and the stale routes
/// removed in the same prompt.
pub fn request(home: &Path, stack: &StackDomains, ca: Option<Ca>) -> TrustRequest {
    TrustRequest {
        domains: stack.domains.clone(),
        ca,
        prune: Routes::open(home).stale(),
        home: Some(PathBuf::from(home)),
        untrust_all: false,
    }
}

/// Doctor's lines for the stack's domains. None for a stack that declares none and a machine with no stale route.
pub fn checks(m: &Machine, stack: Option<&StackDomains>) -> Vec<Check> {
    let home = m.home();
    let empty = StackDomains {
        dir: String::new(),
        domains: Vec::new(),
        source: Source::Recorded,
    };
    let stack = stack.unwrap_or(&empty);
    let auth = Authority::probe(home);
    let uses = uses_doze_ca(home, stack);
    let mut out = Vec::new();
    if !stack.domains.is_empty() {
        let says =
            |state, detail: &str| Check::new("domains: certificate authority", state, detail);
        match (&auth.broken, uses, &auth.ca) {
            (Some(why), _, _) => out.push(
                Check::step("domains: certificate authority", false, why.clone()).fix(FIX_ROTATE),
            ),
            (None, None, _) => out.push(says(
                State::NotChecked,
                "whether a domain is served from the doze authority is known once the stack is up",
            )),
            (None, Some(true), None) => out.push(says(
                State::NotChecked,
                "none made yet: the daemon makes it when a stack serves a domain over https",
            )),
            _ => {}
        }
    }
    let ca = auth.ca.filter(|_| uses == Some(true));
    let req = request(home, stack, ca);
    if req.domains.is_empty() && req.prune.is_empty() {
        return out;
    }
    let steps = trust_status(&req);
    out.extend(steps.into_iter().map(|s| {
        let detail = if s.done {
            s.detail
        } else {
            format!("{} — run {FIX_DOMAINS} (one sudo)", s.detail)
        };
        Check::step(format!("domains: {}", s.name), s.done, detail).fix(FIX_DOMAINS)
    }));
    if !stack.domains.is_empty()
        && let Some(h) = Registry::open(home, "doze doctor")
            .ingress_tls_holder()
            .filter(|e| u32::try_from(e.pid).is_ok_and(doze_os::process::alive))
    {
        out.push(Check::new(
            "domains: https",
            State::Ok,
            format!(
                "{} (pid {}) is serving https:// for the domains",
                h.owner, h.pid
            ),
        ));
    }
    out
}

/// Whether the stack's domains need nothing done.
pub fn set_up(m: &Machine, stack: Option<&StackDomains>) -> bool {
    checks(m, stack).iter().all(|c| c.state != State::Fail)
}

/// Whether the stack serves a domain from the doze authority, so the authority must be trusted here (Go `usesDozeCA`:
/// a domain without the person's own certificate). Its daemon registers each domain in the zone with the certificate
/// the person brought, if any; `None` while none is registered (the stack is not up), when nothing says.
pub fn uses_doze_ca(home: &Path, stack: &StackDomains) -> Option<bool> {
    let zone = Registry::open(home, "doze doctor");
    let found: Vec<doze_names::Entry> = stack
        .domains
        .iter()
        .filter_map(|d| zone.lookup(d))
        .collect();
    if found.is_empty() {
        return None;
    }
    Some(found.iter().any(|e| e.cert.is_empty()))
}
