//! The four cases module-names.md says matter, and every mistake the planner must name.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::*;

pub(crate) fn listener(name: &str, port: u16, http: bool, routes: &[&str]) -> Listener {
    Listener {
        name: name.into(),
        port,
        http,
        routes: routes.iter().map(|r| (*r).to_string()).collect(),
        tcp: false,
    }
}

pub(crate) fn decl(
    name: &str,
    port: Option<u16>,
    default_port: u16,
    listeners: Vec<Listener>,
) -> Decl {
    Decl {
        name: name.into(),
        port,
        routes: true,
        listeners,
        default_port,
    }
}

pub(crate) fn kafka() -> Decl {
    decl(
        "events",
        None,
        9092,
        vec![
            listener("main", 0, false, &[]),
            listener("registry", 8081, true, &["registry"]),
            listener("console", 9093, true, &["@"]),
        ],
    )
}

pub(crate) fn aws(name: &str) -> Decl {
    decl(
        name,
        None,
        4566,
        vec![listener("main", 0, true, &["@", "sync-@"])],
    )
}

fn only(p: &Plan, name: &str) -> InstancePlan {
    p.instances
        .iter()
        .find(|i| i.name == name)
        .cloned()
        .unwrap()
}

fn listener_of<'a>(i: &'a InstancePlan, name: &str) -> &'a ListenerPlan {
    i.listeners.iter().find(|l| l.name == name).unwrap()
}

#[test]
fn a_module_that_says_nothing_gets_its_name_and_its_port() {
    let p = plan("shop", &[decl("db", None, 5432, vec![])], true).unwrap();
    let db = only(&p, "db");
    assert_eq!(db.host.as_deref(), Some("db.shop.doze"));
    let main = listener_of(&db, "main");
    assert_eq!(
        (db.addr(main), db.url(main), main.routes.len()),
        ("db.shop.doze:5432".into(), String::new(), 0)
    );
}

#[test]
fn kafka_gets_a_broker_a_routed_registry_and_a_portless_console() {
    let p = plan("shop", &[kafka()], true).unwrap();
    let k = only(&p, "events");
    assert_eq!(k.addr(listener_of(&k, "main")), "events.shop.doze:9092");
    let registry = listener_of(&k, "registry");
    assert_eq!(
        registry.routes,
        vec![RouteHost {
            host: "registry.events.shop.doze".into(),
            twin: true
        }]
    );
    assert_eq!(k.url(registry), "http://registry.events.shop.doze");
    assert_eq!(k.url(listener_of(&k, "console")), "http://events.shop.doze");
}

#[test]
fn aws_gets_its_name_every_host_under_it_and_the_sync_sibling() {
    let p = plan("shop", &[aws("aws")], true).unwrap();
    let a = only(&p, "aws");
    let main = listener_of(&a, "main");
    let hosts: Vec<_> = main
        .routes
        .iter()
        .map(|r| (r.host.as_str(), r.twin))
        .collect();
    assert_eq!(
        hosts,
        [("aws.shop.doze", false), ("sync-aws.shop.doze", true)]
    );
    assert_eq!(a.url(main), "http://aws.shop.doze");
}

#[test]
fn the_person_can_turn_routes_off() {
    let mut d = aws("aws");
    d.routes = false;
    let p = plan("shop", &[d], true).unwrap();
    let a = only(&p, "aws");
    assert_eq!(a.url(listener_of(&a, "main")), "http://aws.shop.doze:4566");
}

#[test]
fn without_names_everything_is_on_loopback_and_ports_must_differ() {
    let p = plan("shop", &[aws("aws")], false).unwrap();
    let a = only(&p, "aws");
    assert_eq!(
        (a.host.clone(), a.url(listener_of(&a, "main"))),
        (None, "http://127.0.0.1:4566".into())
    );
    let errs = plan(
        "shop",
        &[
            decl("db", None, 5432, vec![]),
            decl("replica", None, 5432, vec![]),
        ],
        false,
    )
    .unwrap_err();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("\"db\" and \"replica\" both use port 5432"),
        "{}",
        errs[0]
    );
    assert!(
        errs[0].contains("doze dns-setup"),
        "says how to fix it: {}",
        errs[0]
    );
    // With names the same two are fine: each has an address of its own.
    plan(
        "shop",
        &[
            decl("db", None, 5432, vec![]),
            decl("replica", None, 5432, vec![]),
        ],
        true,
    )
    .unwrap();
    let errs = plan(
        "shop",
        &[kafka(), decl("schema", Some(8081), 0, vec![])],
        false,
    )
    .unwrap_err();
    assert!(
        errs[0].contains("\"events\" (its registry listener)"),
        "names the listener: {}",
        errs[0]
    );
}

#[test]
fn a_sibling_route_that_is_another_instances_name_is_refused_naming_both() {
    let errs = plan(
        "shop",
        &[aws("cloud"), decl("sync-cloud", None, 80, vec![])],
        true,
    )
    .unwrap_err();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("\"sync-cloud\"") && errs[0].contains("\"cloud\"'s route"),
        "{}",
        errs[0]
    );
}

#[test]
fn the_persons_port_cannot_land_on_another_listener() {
    let mut k = kafka();
    k.port = Some(8081);
    let errs = plan("shop", &[k], true).unwrap_err();
    assert!(
        errs[0].starts_with("events: listeners \"main\" and \"registry\" both use port 8081"),
        "{}",
        errs[0]
    );
}

#[test]
fn a_main_listener_with_no_port_anywhere_is_refused() {
    let errs = plan("shop", &[decl("worker", None, 0, vec![])], true).unwrap_err();
    assert_eq!(
        errs,
        ["worker: the main listener has no port: set `port = …`"]
    );
}

#[test]
fn a_modules_bad_listeners_are_all_reported_with_the_instance() {
    let bad = decl(
        "x",
        None,
        1,
        vec![
            listener("main", 0, false, &["@"]),
            listener("Console", 0, true, &[]),
        ],
    );
    let errs = plan("shop", &[bad, decl("y", None, 0, vec![])], true).unwrap_err();
    assert!(errs.len() >= 3, "{errs:?}");
    assert!(
        errs.iter()
            .all(|e| e.starts_with("x: ") || e.starts_with("y: ")),
        "{errs:?}"
    );
}

#[test]
fn the_plan_is_a_function_of_the_config() {
    let a = plan("shop", &[kafka(), aws("aws")], true).unwrap();
    assert_eq!(a, plan("shop", &[kafka(), aws("aws")], true).unwrap());
}
