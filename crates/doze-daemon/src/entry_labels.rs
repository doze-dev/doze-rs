//! What the views say of each instance, and where doze listens for the ones it sits in front of.

use crate::Labels;
use crate::catalog::Catalog;
use crate::front::Fronted;
use crate::planners::Grant;
use doze_config::{Instance, Kind, Spec};
use doze_module_proto::msg::console::ConsoleSpec;
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
        // A module's instance says so (`module`), as Go's daemon did: the app opens a module's console, and `connect`
        // and `sync` tell a module from a service, by it.
        kind: String::new(),
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
    if let Some((url, front)) = console_of(i, cat, grant) {
        (l.web_url, l.console_front) = (url, front);
    }
    let described = cat
        .manifest_for(&i.ty, &i.version)
        .is_some_and(|m| m.console.is_some());
    (l.kind, l.has_console) = kind_and_console(i.kind(), described);
    l
}

/// What an instance is (`module`, `service`, `task`, …: `connect`, `sync` and the app read it), and whether it has a
/// console the app opens: a module's whose manifest describes one (tables, streams, the working blocks).
fn kind_and_console(kind: Kind, described: bool) -> (String, bool) {
    (kind.as_str().into(), kind == Kind::Module && described)
}

/// The console link of module instance `i` and the tab it opens on, from what the zone granted it.
pub(crate) fn console_of(
    i: &Instance,
    cat: &Catalog,
    grant: Option<&Grant>,
) -> Option<(String, String)> {
    if i.kind() != Kind::Module {
        return None;
    }
    let spec = cat
        .manifest_for(&i.ty, &i.version)
        .and_then(|m| m.console)?;
    console_link(&spec, grant)
}

/// `grant` with each listener at the port it was really bound to. A grant is made from the plan, before anything is
/// bound, and a routed listener whose port was taken is bound on another one (`doze_bind`: "reached by its name
/// only"): a link that names the planned port points at someone else's listener. `ports` is each bound listener's
/// public port; a listener it does not name, or names as 0, keeps the planned address.
pub(crate) fn at_ports(grant: &Grant, ports: &BTreeMap<String, u16>) -> Grant {
    let mut out = grant.clone();
    for (name, g) in &mut out.listeners {
        let Some(port) = ports.get(name).copied().filter(|p| *p > 0) else {
            continue;
        };
        if let Some((host, _)) = doze_config::split_host_port(&g.addr) {
            g.addr = doze_config::join_host_port(&host, &port.to_string());
        }
    }
    out
}

/// Each listener of a bound instance and the public port it got.
pub(crate) fn bound_ports(b: &doze_bind::BoundInstance) -> BTreeMap<String, u16> {
    b.listeners
        .iter()
        .map(|(n, l)| (n.clone(), l.public.port()))
        .collect()
}

/// Where a module's web console is and the tab it opens on, when it declares one (a lean build declares none) and
/// the listener it names was granted.
fn console_link(spec: &ConsoleSpec, grant: Option<&Grant>) -> Option<(String, String)> {
    let url = web_url(spec.web_url.as_deref()?, grant)?;
    Some((url, spec.front.clone().unwrap_or_default()))
}

/// A module's `web_url` template with its address filled in: `{address}` is the instance's main listener,
/// `{address:ui}` the listener named `ui`, each as a person reaches it: the name the instance is known by and the
/// port the listener was granted (`http://events.shop.doze:9093`), never the backend the engine itself listens on
/// (a console turns away a Host it was not told) and never the main listener's port for another's. Empty when the
/// listener it names was not granted, so no link points at nothing.
fn web_url(template: &str, grant: Option<&Grant>) -> Option<String> {
    let grant = grant?;
    let mut out = template.to_owned();
    while let Some(at) = out.find("{address") {
        let end = at + out[at..].find('}')?;
        let name = match out[at + "{address".len()..end].strip_prefix(':') {
            Some(name) => name,
            None => doze_module_proto::network::MAIN,
        };
        let addr = public_addr(grant.listeners.get(name)?)?;
        out.replace_range(at..=end, &addr);
    }
    Some(out)
}

/// `host:port` of a granted listener as a person types it: the host its route gives (the instance's own name for
/// `@`, `registry.events.shop.doze` for a child), else the host of the address it was granted (the instance's name,
/// or 127.0.0.1 with names off), and that address's port. `None` for a listener with no port to name.
fn public_addr(g: &doze_module_proto::msg::instance::Granted) -> Option<String> {
    let (host, port) = doze_config::split_host_port(&g.addr)?;
    if port.parse::<u16>().ok()? == 0 {
        return None;
    }
    let host = g.hosts.first().map_or(host.as_str(), String::as_str);
    Some(doze_config::join_host_port(host, &port))
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

    #[test]
    fn a_module_says_it_is_one_and_has_the_console_its_manifest_describes() {
        assert_eq!(
            kind_and_console(Kind::Module, true),
            ("module".to_owned(), true)
        );
        assert_eq!(
            kind_and_console(Kind::Module, false),
            ("module".to_owned(), false),
            "a module without a console has none to open"
        );
        let (kind, console) = kind_and_console(Kind::Service, true);
        assert!(kind != "module" && !kind.is_empty(), "{kind}");
        assert!(!console, "only a module's page is customised");
    }
    use doze_module_proto::msg::instance::{Address, Granted};

    /// One listener as the zone grants it: where clients connect, and the hosts routed to it.
    fn granted(addr: &str, hosts: &[&str]) -> Granted {
        Granted {
            addr: addr.into(),
            url: String::new(),
            hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
            // Where the engine itself listens: never what a person is sent to.
            backend: Address::Tcp("127.0.0.1:41999".into()),
        }
    }

    fn grant(listeners: &[(&str, Granted)]) -> Grant {
        Grant {
            host: "events.shop.doze".into(),
            listeners: listeners
                .iter()
                .map(|(n, g)| ((*n).to_owned(), g.clone()))
                .collect(),
        }
    }

    /// Kafka's: the broker (TCP, no route) on 9092, the console (HTTP, routed to the instance's name) on 9093.
    fn kafka() -> Grant {
        grant(&[
            ("main", granted("events.shop.doze:9092", &[])),
            (
                "console",
                granted("events.shop.doze:9093", &["events.shop.doze"]),
            ),
        ])
    }

    const KAFKA: &str = "http://{address:console}";

    #[test]
    fn kafkas_console_link_is_its_name_and_the_console_port_not_the_brokers() {
        assert_eq!(
            web_url(KAFKA, Some(&kafka())).as_deref(),
            Some("http://events.shop.doze:9093")
        );
        // The main listener is the broker's, 9092, only when a template asks for it.
        assert_eq!(
            web_url("http://{address}", Some(&kafka())).as_deref(),
            Some("http://events.shop.doze:9092")
        );
    }

    #[test]
    fn awss_console_link_is_its_name_and_port_4567() {
        let g = grant(&[
            ("main", granted("local.shop.doze:4566", &[])),
            (
                "console",
                granted("local.shop.doze:4567", &["local.shop.doze"]),
            ),
        ]);
        assert_eq!(
            web_url(KAFKA, Some(&g)).as_deref(),
            Some("http://local.shop.doze:4567")
        );
        assert_eq!(
            web_url("http://{address:console}/ui", Some(&g)).as_deref(),
            Some("http://local.shop.doze:4567/ui")
        );
    }

    #[test]
    fn a_listener_that_was_bound_elsewhere_is_linked_where_it_is() {
        // The console's port was taken, so doze bound it on another: the link names that one, and the main listener, which got
        // its planned port, is untouched.
        let moved = at_ports(&kafka(), &BTreeMap::from([("console".to_owned(), 51234)]));
        assert_eq!(
            web_url(KAFKA, Some(&moved)).as_deref(),
            Some("http://events.shop.doze:51234")
        );
        assert_eq!(
            web_url("http://{address}", Some(&moved)).as_deref(),
            Some("http://events.shop.doze:9092")
        );
        // The host it is reached by stays the name's (a routed child keeps its own).
        let child = grant(&[(
            "registry",
            granted("events.shop.doze:8081", &["registry.events.shop.doze"]),
        )]);
        let moved = at_ports(&child, &BTreeMap::from([("registry".to_owned(), 40001)]));
        assert_eq!(
            web_url("http://{address:registry}", Some(&moved)).as_deref(),
            Some("http://registry.events.shop.doze:40001")
        );
    }

    #[test]
    fn a_listener_nothing_says_a_port_for_keeps_the_planned_one() {
        let same = at_ports(
            &kafka(),
            &BTreeMap::from([("console".to_owned(), 0), ("elsewhere".to_owned(), 7)]),
        );
        assert_eq!(same, kafka());
        assert_eq!(at_ports(&kafka(), &BTreeMap::new()), kafka());
    }

    #[test]
    fn a_link_never_names_the_backend_the_engine_listens_on() {
        let url = web_url(KAFKA, Some(&kafka())).unwrap();
        assert!(
            !url.contains("41999") && !url.contains("127.0.0.1"),
            "{url}"
        );
    }

    #[test]
    fn a_routed_child_is_reached_by_its_own_name_and_the_granted_port() {
        let g = grant(&[
            ("main", granted("events.shop.doze:9092", &[])),
            (
                "registry",
                granted("events.shop.doze:8081", &["registry.events.shop.doze"]),
            ),
        ]);
        assert_eq!(
            web_url("http://{address:registry}", Some(&g)).as_deref(),
            Some("http://registry.events.shop.doze:8081")
        );
    }

    #[test]
    fn with_names_off_the_link_is_the_address_the_zone_granted() {
        let g = Grant {
            host: String::new(),
            listeners: BTreeMap::from([("console".to_owned(), granted("127.0.0.1:9093", &[]))]),
        };
        assert_eq!(
            web_url(KAFKA, Some(&g)).as_deref(),
            Some("http://127.0.0.1:9093")
        );
    }

    fn spec(web_url: Option<&str>) -> ConsoleSpec {
        ConsoleSpec {
            panels: vec![],
            tabs: vec![],
            web_url: web_url.map(str::to_owned),
            front: Some("web".into()),
            extend: vec![],
        }
    }

    #[test]
    fn no_console_declared_gives_no_link() {
        // A lean kafka build declares no `web_url`: whatever the zone granted, there is no link.
        assert_eq!(console_link(&spec(None), Some(&kafka())), None);
        // One that declares it gets the link and the tab it opens on.
        assert_eq!(
            console_link(&spec(Some(KAFKA)), Some(&kafka())),
            Some(("http://events.shop.doze:9093".into(), "web".into()))
        );
    }

    #[test]
    fn a_console_declared_but_not_granted_gives_no_link() {
        let broker_only = grant(&[("main", granted("events.shop.doze:9092", &[]))]);
        assert_eq!(web_url(KAFKA, Some(&broker_only)), None);
        assert_eq!(console_link(&spec(Some(KAFKA)), Some(&broker_only)), None);
        assert_eq!(web_url(KAFKA, Some(&Grant::default())), None);
        assert_eq!(web_url(KAFKA, None), None);
        // A listener without a port is nowhere a person can go.
        let unbound = grant(&[("console", granted("127.0.0.1:0", &[]))]);
        assert_eq!(web_url(KAFKA, Some(&unbound)), None);
    }
}
