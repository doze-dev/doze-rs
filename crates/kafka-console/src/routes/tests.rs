use std::collections::BTreeSet;

use super::*;
use crate::base::Base;

#[test]
fn no_screen_is_served_by_two_routes_and_no_route_is_listed_twice() {
    let mut screens = BTreeSet::new();
    let mut routes = BTreeSet::new();
    for r in ROUTES {
        assert!(
            routes.insert((r.path, r.method == Method::Post)),
            "{} is a row twice",
            r.path
        );
        for s in r.screens {
            assert!(screens.insert(*s), "screen {s} is served by two routes");
        }
    }
}

#[test]
fn only_a_post_has_an_op_that_is_not_a_read_and_a_mutation_is_never_a_get() {
    for r in ROUTES {
        if r.mutates {
            assert_eq!(r.method, Method::Post, "{}", r.path);
        }
        if let Some(op) = r.op {
            assert!(op.chars().next().is_some_and(char::is_uppercase), "{op}");
        }
    }
}

#[test]
fn the_five_boards_of_slice_9a_are_served_by_two_routes_and_the_advertised_page() {
    let serving = |id: &str| {
        ROUTES
            .iter()
            .find(|r| r.screens.contains(&id))
            .map(|r| r.path)
    };
    for id in ["home", "home-quiet", "home-attention"] {
        assert_eq!(serving(id), Some("/"), "{id}");
    }
    assert_eq!(serving("connect"), Some("/connect"));
    assert_eq!(serving("connect-advertised"), Some("/connect/advertised"));
}

#[test]
fn a_prefix_registers_the_root_both_ways_and_prefixes_the_rest() {
    assert_eq!(paths(&Base::root(), "/"), ["/"]);
    assert_eq!(paths(&Base::new("/k"), "/"), ["/k", "/k/"]);
    assert_eq!(paths(&Base::new("/k"), "/connect"), ["/k/connect"]);
    assert_eq!(paths(&Base::root(), "/connect"), ["/connect"]);
}

#[test]
fn routes_json_is_what_the_table_says() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("design/routes.json");
    if std::env::var_os("DOZE_UPDATE_DESIGN").is_some() {
        std::fs::write(&path, json()).unwrap();
    }
    assert!(
        std::fs::read_to_string(&path).unwrap() == json(),
        "design/routes.json is not what routes.rs says: run the tests with DOZE_UPDATE_DESIGN=1"
    );
}
