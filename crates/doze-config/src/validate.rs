//! Checks across the whole stack once it is read: ports that collide, names that collide, and an
//! app's own domains (Go's `validate.go`, `network.go` and `domains.go`).
//!
//! A known gap: Go also refuses a module listener whose route name (from the module's declared
//! listeners, through `netplan`) is another block's doze name. Route names need the module's
//! listener declarations, which come through [`Modules`] once the module catalog declares them.

use crate::attrs::{instance_addr, join_host_port, split_host_port};
use crate::error::{Error, Result};
use crate::host::{Host, names_disabled};
use crate::kinds::Head;
use crate::model::{Config, Instance, Kind, ModuleBody, Spec};
use crate::modules::{Call, Endpoint, Modules};
use doze_hcl::value::quote::go_quote;
use std::collections::BTreeMap;
use std::path::Path;

fn join(errs: Vec<String>) -> Result<()> {
    if errs.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(errs.join("\n")))
    }
}

/// Go's `validatePorts`: every enabled instance needs an address, and no two share one.
pub fn ports(cfg: &Config, modules: &dyn Modules) -> Result<()> {
    let mut addrs: BTreeMap<String, String> = BTreeMap::new();
    let mut errs = Vec::new();
    for d in cfg.instances.iter().filter(|d| d.enabled) {
        let addr = match instance_addr(d, modules) {
            Ok(a) => a,
            Err(e) => {
                errs.push(e);
                continue;
            }
        };
        if addr.is_empty() || cfg.defaults.names {
            continue;
        }
        let mut uses = BTreeMap::from([(addr, go_quote(&d.name))]);
        uses.extend(extra_listeners(d, modules));
        for (a, what) in uses {
            if let Some(other) = addrs.get(&a) {
                errs.push(format!("port conflict: {other} and {what} both use {a} — give each instance a unique port (or `defaults {{ names = true }}` to share ports by name)"));
                continue;
            }
            addrs.insert(a, what);
        }
    }
    join(errs)
}

/// Go's `extraListenerAddrs`: a module's other listeners, which hold ports too.
fn extra_listeners(d: &Instance, modules: &dyn Modules) -> BTreeMap<String, String> {
    if d.ty == crate::model::COMMAND_ENGINE {
        return BTreeMap::new();
    }
    let host = split_host_port(&d.listen)
        .map(|(h, _)| h)
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "127.0.0.1".into());
    let empty = ModuleBody::default();
    let body = match &d.spec {
        Spec::Module(b) => b,
        _ => &empty,
    };
    let ep = Endpoint::Tcp(join_host_port(&host, &d.port.to_string()));
    let call = Call {
        ty: &d.ty,
        name: &d.name,
        version: &d.version,
        endpoint: &ep,
        body,
    };
    modules
        .listeners(&call, d.port)
        .into_iter()
        .map(|l| {
            (
                join_host_port(&host, &l.port.to_string()),
                format!("{} (its {} listener)", go_quote(&d.name), l.name),
            )
        })
        .collect()
}

/// Go's `validateNetworks`, without route names (see the module docs): what a module's network
/// asks for is checked by [`ports`]; nothing else is decided here yet.
pub fn networks(_cfg: &Config, _modules: &dyn Modules) -> Result<()> {
    Ok(())
}

/// Go's `validateAppDomains`: a domain is one app's, and domains need names on.
pub fn app_domains(cfg: &Config, host: &dyn Host) -> Result<()> {
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    for d in &cfg.instances {
        for dom in &d.domains {
            if let Some(other) = owner.get(dom.as_str()) {
                return Err(Error::msg(format!(
                    "domain conflict: {} and {} both declare {dom}",
                    go_quote(other),
                    go_quote(&d.name)
                )));
            }
            owner.insert(dom, &d.name);
        }
        if !d.domains.is_empty() && !cfg.defaults.names {
            if names_disabled(host) {
                return Err(Error::msg(format!(
                    "{} declares domains, but names are switched off here (DOZE_ZONE); unset it to serve them",
                    go_quote(&d.name)
                )));
            }
            return Err(Error::msg(format!(
                "{} declares domains, which need names on: add `names = true` to the defaults block",
                go_quote(&d.name)
            )));
        }
    }
    Ok(())
}

/// Go's `validateDomains`: with names on, no two blocks may get the same name.
pub fn domains(cfg: &Config) -> Result<()> {
    if !cfg.defaults.names {
        return Ok(());
    }
    let mut owner: BTreeMap<String, &str> = BTreeMap::new();
    for d in cfg.instances.iter().filter(|d| d.enabled) {
        let dom = cfg.domain_for(&d.name);
        if let Some(other) = owner.get(&dom) {
            return Err(Error::msg(format!(
                "domain conflict: {} and {} both publish {dom} — rename one so the sanitized hostnames differ",
                go_quote(other),
                go_quote(&d.name)
            )));
        }
        owner.insert(dom, &d.name);
    }
    Ok(())
}

/// An app's `domains`, `cert` and `key` as written, and the folder they are relative to.
#[derive(Debug, Clone, Copy)]
pub struct DomainDecl<'d> {
    pub domains: &'d [String],
    pub cert: &'d str,
    pub key: &'d str,
    pub base_dir: &'d str,
}

/// Go's `checkDomains`: the domains, lower case; the cert and key made absolute and checked.
pub fn check_domains(
    host: &dyn Host,
    head: &Head,
    what: &str,
    d: DomainDecl<'_>,
    shape: bool,
) -> std::result::Result<(Vec<String>, String, String), String> {
    if d.domains.is_empty() && d.cert.is_empty() && d.key.is_empty() {
        return Ok((Vec::new(), String::new(), String::new()));
    }
    if head.kind != Kind::App {
        return Err(format!(
            "{what}: `domains` is for app blocks; a {} is reached by its doze name",
            head.kind.as_str()
        ));
    }
    if d.domains.is_empty() {
        return Err(format!("{what}: `cert` and `key` go with `domains`"));
    }
    let mut out: Vec<String> = Vec::new();
    for dom in d.domains {
        host.valid_domain(dom)
            .map_err(|e| format!("{what}: domains: {e}"))?;
        let dom = crate::paths::go_lower(dom);
        let dom = dom.strip_suffix('.').unwrap_or(&dom).to_owned();
        if !registrable(&host.route_domain(&dom)) {
            return Err(format!(
                "{what}: domains: {} is a public suffix, not a domain of yours; declare a name under it",
                go_quote(&dom)
            ));
        }
        if out.contains(&dom) {
            return Err(format!(
                "{what}: domains: {} is listed twice",
                go_quote(&dom)
            ));
        }
        out.push(dom);
    }
    if d.cert.is_empty() != d.key.is_empty() {
        return Err(format!(
            "{what}: set both `cert` and `key`, or neither (doze then mints a certificate)"
        ));
    }
    let abs = |p: &str| {
        if p.is_empty() || Path::new(p).is_absolute() {
            p.to_owned()
        } else {
            crate::files::path_string(&crate::paths::clean(&Path::new(d.base_dir).join(p)))
        }
    };
    let (cert, key) = (abs(d.cert), abs(d.key));
    if !cert.is_empty() && !shape {
        for f in [&cert, &key] {
            if !host.abs(Path::new(f)).is_file() {
                return Err(format!("{what}: {f} is not a file"));
            }
        }
        for dom in &out {
            host.check_certificate(Path::new(&cert), Path::new(&key), dom)
                .map_err(|e| format!("{what}: {e}"))?;
        }
    }
    Ok((out, cert, key))
}

/// Go's `publicsuffix.EffectiveTLDPlusOne` succeeds: the name has a label of its own under its
/// public suffix.
fn registrable(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return false;
    }
    psl::domain(name.as_bytes()).is_some_and(|d| d.as_bytes().len() <= name.len())
}
