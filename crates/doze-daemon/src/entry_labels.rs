//! What the views say of each instance, and where doze listens for the ones it sits in front of.

use crate::Labels;
use crate::catalog::Catalog;
use crate::front::Fronted;
use crate::planners::Grant;
use doze_config::{Instance, Kind, Spec};
use std::collections::BTreeMap;
use std::path::Path;

/// What the config says about each instance, for the views; a module instance's variables, URL and client as its
/// module answers them.
pub(super) fn labels(
    cfg: &doze_config::Config,
    cat: &Catalog,
    grants: &BTreeMap<String, Grant>,
) -> BTreeMap<String, Labels> {
    cfg.instances
        .iter()
        .map(|i| (i.name.clone(), label(i, cat, grants.get(&i.name))))
        .collect()
}

/// What the config says about one instance, and what the zone granted it (its name, its listeners).
pub(crate) fn label(i: &Instance, cat: &Catalog, grant: Option<&Grant>) -> Labels {
    let run = match &i.spec {
        Spec::Command(c) => c.run.clone(),
        _ => String::new(),
    };
    let host = grant.map(|g| g.host.clone()).unwrap_or_default();
    let named = !host.is_empty();
    let mut l = Labels {
        kind: match i.kind() {
            Kind::Module => String::new(),
            k => k.as_str().into(),
        },
        engine: i.ty.clone(),
        version: i.version.clone(),
        run,
        needs: i.deps.iter().map(|d| d.name.clone()).collect(),
        endpoint: doze_config::instance_addr(i, cat).unwrap_or_default(),
        domain: host,
        paused: !i.enabled,
        ..Labels::default()
    };
    if named && let Some(first) = i.domains.first() {
        l.domains = i.domains.clone();
        l.https = format!("https://{}", doze_names::domains::route_domain(first));
    }
    let main = grant.and_then(|g| g.listeners.get(doze_module_proto::network::MAIN));
    if i.kind() != Kind::Module
        && let Some(m) = main
    {
        l.url = if m.url.is_empty() {
            format!("http://{}", m.addr)
        } else {
            m.url.clone()
        };
    }
    if i.kind() == Kind::Module
        && let Some(p) = crate::planners::as_module_sees_it_granted(cat, i, grant)
            .and_then(|mi| cat.published(&mi))
    {
        l.connect = cat
            .manifest_for(&i.ty, &i.version)
            .and_then(|m| m.client)
            .map(|c| crate::client::view(&c, &l.endpoint, &p.url, &p.extras));
        (l.url, l.env_var, l.env, l.client) = (p.url, p.env_var, p.env, p.client);
    }
    if i.kind() == Kind::Module
        && let Some(spec) = cat.manifest_for(&i.ty, &i.version).and_then(|m| m.console)
        && let Some(url) = spec.web_url.as_deref().and_then(|t| web_url(t, grant))
    {
        l.web_url = url;
        l.console_front = spec.front.unwrap_or_default();
    }
    l
}

/// A module's `web_url` template with its address filled in: `{address}` is the instance's main listener,
/// `{address:ui}` the listener named `ui`, each as doze granted it (the instance's name and port). Empty when
/// the listener it names was not granted, so no link points at nothing.
fn web_url(template: &str, grant: Option<&Grant>) -> Option<String> {
    let grant = grant?;
    let mut out = template.to_owned();
    while let Some(at) = out.find("{address") {
        let end = at + out[at..].find('}')?;
        let name = match out[at + "{address".len()..end].strip_prefix(':') {
            Some(name) => name,
            None => doze_module_proto::network::MAIN,
        };
        let addr = &grant.listeners.get(name)?.addr;
        out.replace_range(at..=end, addr);
    }
    Some(out)
}

/// Where doze listens for `i`, if it sits in front of it: every module that is not supervised, and every app or
/// service with a port it does not bind itself. Its listeners are what its module declares (`network`); an app's is
/// one HTTP listener, routed through the front door by its name (Go's ingress: `http://api.shop.doze`, no port).
pub(crate) fn fronted_one(
    i: &Instance,
    cat: &Catalog,
    supervised: bool,
    run_dir: &Path,
) -> Option<Fronted> {
    let app = matches!(&i.spec, Spec::Command(c) if i.port > 0 && !c.direct);
    let module = i.kind() == Kind::Module && !supervised;
    if !i.enabled || !(app || module) {
        return None;
    }
    let addr = doze_config::instance_addr(i, cat).ok()?;
    if addr.is_empty() || addr.starts_with("unix:") {
        return None;
    }
    let port = addr
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse::<u16>().ok())
        .filter(|p| *p > 0);
    let any =
        doze_module_proto::msg::instance::Address::Tcp(doze_engine::modules::ANY_LOOPBACK.into());
    let (listeners, default_port, main_backend) = if app {
        let main = doze_module_proto::msg::instance::Listener {
            name: doze_module_proto::network::MAIN.into(),
            port: 0,
            http: true,
            routes: vec!["@".into()],
            tcp: false,
        };
        (vec![main], 0, any)
    } else {
        let m = cat.manifest_for(&i.ty, &i.version)?;
        let mi = crate::planners::as_module_sees_it(cat, i)?;
        let tcp_only = cat.main_is_tcp(&mi);
        let backend = crate::planners::backend(&addr, &i.name, &m, tcp_only, run_dir);
        (cat.network(&mi), m.default_port, backend)
    };
    Some(Fronted {
        name: i.name.clone(),
        decl: doze_bind::Decl {
            name: i.name.clone(),
            port,
            routes: !i.no_routes,
            listeners,
            default_port,
        },
        app,
        domains: i.domains.clone(),
        cert: i.cert.clone(),
        key: i.key.clone(),
        main_backend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use doze_module_proto::msg::instance::{Address, Granted};

    fn grant(listeners: &[(&str, &str)]) -> Grant {
        Grant {
            host: "local.shop.doze".into(),
            listeners: listeners
                .iter()
                .map(|(n, a)| {
                    (
                        (*n).to_owned(),
                        Granted {
                            addr: (*a).into(),
                            url: String::new(),
                            hosts: vec![],
                            backend: Address::Tcp("127.0.0.1:1".into()),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn a_web_url_takes_the_address_doze_granted_the_listener_it_names() {
        let g = grant(&[
            ("main", "local.shop.doze:4566"),
            ("console", "local.shop.doze:4567"),
        ]);
        let url = |t: &str| web_url(t, Some(&g));
        assert_eq!(
            url("http://{address:console}").as_deref(),
            Some("http://local.shop.doze:4567")
        );
        assert_eq!(
            url("http://{address}/_console").as_deref(),
            Some("http://local.shop.doze:4566/_console")
        );
        // No link to a listener that was not granted, nor when nothing was.
        assert_eq!(url("http://{address:ui}"), None);
        assert_eq!(web_url("http://{address:console}", None), None);
    }
}
