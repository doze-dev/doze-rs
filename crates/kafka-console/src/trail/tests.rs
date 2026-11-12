use super::*;
use crate::routes::{Method, ROUTES};

fn labels(p: Place<'_>) -> Vec<String> {
    p.steps().into_iter().map(|(l, _)| l).collect()
}

/// Whether a path matches a route pattern (`{x}` is one segment).
fn matches(pattern: &str, path: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(p, s)| (p.starts_with('{') && p.ends_with('}')) || p == s)
}

#[test]
fn a_place_is_its_entity_path_and_nothing_else() {
    let table: [(Place<'_>, &[&str]); 12] = [
        (Place::Home, &["Home"]),
        (Place::Connect, &["Home", "Connect an app"]),
        (Place::Topics, &["Resources", "Topics"]),
        (Place::NewTopic, &["Resources", "Topics", "New topic"]),
        (Place::Topic("orders"), &["Resources", "Topics", "orders"]),
        (
            Place::TopicPage("orders", TopicPage::Partitions),
            &["Resources", "Topics", "orders", "Partitions"],
        ),
        (
            Place::TopicPage("orders", TopicPage::Consumers),
            &["Resources", "Topics", "orders", "Consumers"],
        ),
        (Place::Groups, &["Resources", "Groups"]),
        (
            Place::Group("fulfilment"),
            &["Resources", "Groups", "fulfilment"],
        ),
        (
            Place::GroupPage("fulfilment", "Members"),
            &["Resources", "Groups", "fulfilment", "Members"],
        ),
        (Place::Live, &["Live"]),
        (Place::Settings, &["Settings"]),
    ];
    for (place, want) in table {
        assert_eq!(labels(place), want, "{place:?}");
    }
}

#[test]
fn the_last_step_has_no_link_and_every_other_step_links() {
    let places = [
        Place::Home,
        Place::Connect,
        Place::Advertised,
        Place::FirstRequest,
        Place::Topics,
        Place::NewTopic,
        Place::Topic("a"),
        Place::TopicPage("a", TopicPage::Config),
        Place::NoTopic,
        Place::Groups,
        Place::Group("g"),
        Place::GroupPage("g", "Lag"),
        Place::NoGroup,
        Place::Live,
        Place::Access,
        Place::Settings,
        Place::Lab,
        Place::NotFound,
        Place::NotAllowed,
    ];
    for p in places {
        let steps = p.steps();
        let (last, rest) = steps.split_last().unwrap();
        assert!(last.1.is_none(), "{p:?}: the last step is the page");
        assert!(rest.iter().all(|s| s.1.is_some()), "{p:?}: {rest:?}");
    }
}

#[test]
fn every_link_of_a_trail_is_a_page_the_console_serves() {
    let get: Vec<&str> = ROUTES
        .iter()
        .filter(|r| r.method == Method::Get)
        .map(|r| r.path)
        .collect();
    let places = [
        Place::Connect,
        Place::Advertised,
        Place::NewTopic,
        Place::Topic("orders"),
        Place::TopicPage("orders", TopicPage::Producers),
        Place::TopicPage("a b/c", TopicPage::Wire),
        Place::GroupPage("fulfilment", "Members"),
        Place::NoTopic,
        Place::NoGroup,
        Place::NotFound,
    ];
    for p in places {
        for (label, path) in p.steps() {
            let Some(path) = path else { continue };
            assert!(
                get.iter().any(|r| matches(r, &path)),
                "{p:?}: the step {label} links to {path}, which no route serves"
            );
        }
    }
}

#[test]
fn a_name_is_shown_cut_at_sixty_characters_and_escaped_by_the_shell_not_here() {
    let long = "x".repeat(300);
    let steps = Place::Topic(&long).steps();
    let name = &steps[2].0;
    assert_eq!(name.chars().count(), 60);
    assert!(name.ends_with('…'));
    let hostile = "<script>alert(1)</script>";
    assert_eq!(Place::Topic(hostile).steps()[2].0, hostile);
    // The link to a topic with a slash or a space in its name is encoded.
    let steps = Place::TopicPage("a b/c", TopicPage::Config).steps();
    assert_eq!(steps[2].1.as_deref(), Some("/topics/a%20b%2Fc"));
}

#[test]
fn no_place_takes_a_filter_so_a_filter_cannot_be_in_a_trail() {
    // The same topic is the same trail whatever the page is showing of it: there is nothing to pass.
    assert_eq!(
        Place::Topic("orders").steps(),
        Place::Topic("orders").steps()
    );
    for word in [
        "read_committed",
        "read_uncommitted",
        "Paused",
        "A busy minute",
        "orders-0",
    ] {
        for p in [Place::Topic("orders"), Place::Live, Place::Groups] {
            assert!(!labels(p).iter().any(|l| l == word), "{p:?} says {word}");
        }
    }
}
