//! The rules for a module's listeners and their routes (the Go SDK's `ValidateNetwork` and `RouteHost`, which decide
//! the names doze gives an instance). Shared by both sides: the SDK checks a module's own answer, the core checks what
//! it receives.
//!
//! A route is `@` (the instance's own name), a DNS label such as `registry` (a child: `registry.events.shop.doze`), or
//! a prefix followed by `@` such as `sync-@` (a sibling: `sync-aws.shop.doze`). Only HTTP listeners have routes.

use crate::msg::instance::Listener;
use std::collections::{BTreeMap, BTreeSet};

/// The listener the host's proxy serves; every module has one.
pub const MAIN: &str = "main";

enum Route<'a> {
    Self_,
    Child(&'a str),
    Sibling(&'a str),
}

fn label_chars(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_label(s: &str) -> bool {
    label_chars(s) && s.len() <= 63 && !s.starts_with('-') && !s.ends_with('-')
}

fn parse(route: &str) -> Result<Route<'_>, String> {
    let bad = |why: &str| {
        Err(format!(
            "route {route:?}: {why}; a route is \"@\" (the instance's name), a label such as \"registry\" (a child), or a prefix followed by @ such as \"sync-@\" (a sibling)"
        ))
    };
    if route == "@" {
        return Ok(Route::Self_);
    }
    if let Some(prefix) = route.strip_suffix('@') {
        if !label_chars(prefix) {
            return bad("the prefix must be letters, digits and hyphens");
        }
        return Ok(Route::Sibling(prefix));
    }
    if route.contains('@') {
        return bad("@ may only end it");
    }
    if !is_label(route) {
        return bad("a child is one DNS label");
    }
    Ok(Route::Child(route))
}

/// The host name a route gives under the instance's own name `host`.
pub fn route_host(route: &str, host: &str) -> Result<String, String> {
    Ok(match parse(route)? {
        Route::Self_ => host.to_string(),
        Route::Child(c) => format!("{c}.{host}"),
        Route::Sibling(p) => format!("{p}{host}"),
    })
}

/// Every problem with a set of listeners (empty: valid).
pub fn validate(listeners: &[Listener]) -> Vec<String> {
    let mut errs = Vec::new();
    let mut names = BTreeSet::new();
    let mut ports: BTreeMap<u16, &str> = BTreeMap::new();
    let mut routes = BTreeSet::new();
    for l in listeners {
        if !is_label(&l.name) {
            errs.push(format!(
                "listener name {:?}: a listener's name is one DNS label, such as \"console\"",
                l.name
            ));
        }
        if !names.insert(l.name.as_str()) {
            errs.push(format!("two listeners are named {:?}", l.name));
        }
        if l.port == 0 && l.name != MAIN {
            errs.push(format!(
                "listener {:?}: port 0 is not a port (1 to 65535)",
                l.name
            ));
        } else if l.port != 0
            && let Some(other) = ports.insert(l.port, &l.name)
        {
            errs.push(format!(
                "listeners {other:?} and {:?} both use port {}",
                l.name, l.port
            ));
        }
        if !l.http && !l.routes.is_empty() {
            errs.push(format!("listener {:?} is tcp: only an http listener has routes (plain TCP carries no host to route on)", l.name));
        }
        for r in &l.routes {
            if let Err(e) = parse(r) {
                errs.push(format!("listener {:?}: {e}", l.name));
            } else if !routes.insert(r.as_str()) {
                errs.push(format!("route {r:?} is given twice"));
            }
        }
    }
    if !names.contains(MAIN) {
        errs.push(format!(
            "no listener named {MAIN:?}: it is the one the host's proxy serves"
        ));
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(name: &str, port: u16, http: bool, routes: &[&str]) -> Listener {
        Listener {
            name: name.into(),
            port,
            http,
            routes: routes.iter().map(|r| r.to_string()).collect(),
            tcp: false,
        }
    }

    /// The Go SDK's cases (engine/network_test.go), less "an unknown protocol", which a bool cannot express.
    fn go_cases() -> Vec<(&'static str, Vec<Listener>, &'static str)> {
        let main = l(MAIN, 0, false, &[]);
        let http = |n: &str, p: u16, r: &[&str]| l(n, p, true, r);
        vec![
            ("main with no port", vec![main.clone()], ""),
            (
                "kafka",
                vec![
                    main.clone(),
                    http("console", 9093, &["@"]),
                    http("registry", 8081, &["registry"]),
                ],
                "",
            ),
            ("aws", vec![l(MAIN, 0, true, &["@", "sync-@"])], ""),
            (
                "an http listener reached on its port only",
                vec![main.clone(), http("ui", 8025, &[])],
                "",
            ),
        ]
    }

    fn go_refusals() -> Vec<(&'static str, Vec<Listener>, &'static str)> {
        let main = l(MAIN, 0, false, &[]);
        let http = |n: &str, p: u16, r: &[&str]| l(n, p, true, r);
        vec![
            ("empty", vec![], "no listener named \"main\""),
            (
                "no main",
                vec![http("ui", 80, &["@"])],
                "no listener named \"main\"",
            ),
            (
                "two with one name",
                vec![main.clone(), main.clone()],
                "two listeners are named \"main\"",
            ),
            (
                "two on one port",
                vec![main.clone(), http("ui", 9093, &[]), http("api", 9093, &[])],
                "both use port 9093",
            ),
            (
                "another listener with no port",
                vec![main.clone(), http("ui", 0, &[])],
                "port 0",
            ),
            (
                "routes on tcp",
                vec![l(MAIN, 0, false, &["@"])],
                "only an http listener has routes",
            ),
            (
                "a bad listener name",
                vec![main.clone(), http("Web UI", 8080, &[])],
                "listener name \"Web UI\"",
            ),
            (
                "a route that is a full name",
                vec![main.clone(), http("ui", 80, &["ui.shop.doze"])],
                "route \"ui.shop.doze\"",
            ),
            (
                "a wildcard route",
                vec![main.clone(), http("ui", 80, &["*"])],
                "route \"*\"",
            ),
            (
                "an empty prefix",
                vec![main.clone(), http("ui", 80, &["@@"])],
                "route \"@@\"",
            ),
            (
                "a suffix after the name",
                vec![main.clone(), http("ui", 80, &["@-x"])],
                "route \"@-x\"",
            ),
            (
                "one route twice",
                vec![
                    main.clone(),
                    http("ui", 80, &["@"]),
                    http("api", 81, &["@"]),
                ],
                "route \"@\" is given twice",
            ),
        ]
    }

    #[test]
    fn the_go_cases() {
        for (what, ls, want) in go_cases().into_iter().chain(go_refusals()) {
            let errs = validate(&ls).join("; ");
            if want.is_empty() {
                assert!(errs.is_empty(), "{what}: {errs}");
            } else {
                assert!(errs.contains(want), "{what}: got {errs:?}, want {want:?}");
            }
        }
    }

    #[test]
    fn route_hosts() {
        assert_eq!(
            route_host("@", "aws.shop.doze").as_deref(),
            Ok("aws.shop.doze")
        );
        assert_eq!(
            route_host("registry", "events.shop.doze").as_deref(),
            Ok("registry.events.shop.doze")
        );
        assert_eq!(
            route_host("sync-@", "aws.shop.doze").as_deref(),
            Ok("sync-aws.shop.doze")
        );
        assert!(route_host("*", "aws.shop.doze").is_err());
    }
}
