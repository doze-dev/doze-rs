use kafka_auth::Principal;
use kafka_wire::messages::ApiKey;

use super::super::rule::{Matcher, Pattern};
use super::super::testing::{ask, fetch, head, heartbeat, metadata, produce};

fn pat(s: &str) -> Pattern {
    Pattern::new(s).unwrap()
}

#[test]
fn an_empty_matcher_fits_everything_but_the_console() {
    let m = Matcher::default();
    let (h, r) = (
        head(ApiKey::Produce, 9, "orders"),
        produce("orders", &[0], 1),
    );
    let anyone = Principal::user("alice");
    assert!(m.fits(&ask(Some(1), &anyone, &h, Some(&r))));
    let console = Principal::console();
    assert!(!m.fits(&ask(Some(1), &console, &h, Some(&r))));
}

#[test]
fn the_console_is_touched_only_by_a_rule_that_names_it() {
    let m = Matcher {
        principal: Some("User:__console".into()),
        ..Matcher::default()
    };
    let (h, r) = (
        head(ApiKey::Produce, 9, "console"),
        produce("orders", &[0], 1),
    );
    let console = Principal::console();
    assert!(m.fits(&ask(None, &console, &h, Some(&r))));
    let other = Principal::user("alice");
    assert!(!m.fits(&ask(None, &other, &h, Some(&r))));
}

#[test]
fn each_field_must_fit_and_a_field_left_empty_fits_anything() {
    let who = Principal::anonymous();
    let (h, r) = (
        head(ApiKey::Produce, 9, "orders-go"),
        produce("orders", &[0, 2], 1),
    );
    let a = ask(None, &who, &h, Some(&r));
    let fits = |m: Matcher| m.fits(&a);
    assert!(fits(Matcher {
        client: Some(pat("orders-*")),
        ..Matcher::default()
    }));
    assert!(!fits(Matcher {
        client: Some(pat("payments*")),
        ..Matcher::default()
    }));
    assert!(fits(Matcher {
        apis: vec![ApiKey::Fetch, ApiKey::Produce],
        ..Matcher::default()
    }));
    assert!(!fits(Matcher {
        apis: vec![ApiKey::Fetch],
        ..Matcher::default()
    }));
    assert!(fits(Matcher {
        topics: vec![pat("pay*"), pat("ord*")],
        ..Matcher::default()
    }));
    assert!(!fits(Matcher {
        topics: vec![pat("pay*")],
        ..Matcher::default()
    }));
    assert!(fits(Matcher {
        topics: vec![pat("orders")],
        partitions: vec![2],
        ..Matcher::default()
    }));
    assert!(!fits(Matcher {
        topics: vec![pat("orders")],
        partitions: vec![1],
        ..Matcher::default()
    }));
    assert!(fits(Matcher {
        principal: Some("User:ANONYMOUS".into()),
        ..Matcher::default()
    }));
    assert!(!fits(Matcher {
        principal: Some("User:alice".into()),
        ..Matcher::default()
    }));
}

#[test]
fn a_partition_belongs_to_the_topic_it_is_named_under() {
    // Partition 1 of orders and partition 0 of payments are asked for: a rule for payments-1 does not fit.
    let who = Principal::anonymous();
    let h = head(ApiKey::Fetch, 12, "c");
    let r = fetch("orders", &[1]);
    let m = Matcher {
        topics: vec![pat("payments")],
        partitions: vec![1],
        ..Matcher::default()
    };
    assert!(!m.fits(&ask(None, &who, &h, Some(&r))));
    let m = Matcher {
        topics: vec![pat("orders")],
        partitions: vec![1],
        ..Matcher::default()
    };
    assert!(m.fits(&ask(None, &who, &h, Some(&r))));
}

#[test]
fn a_topic_rule_does_not_fit_a_request_that_names_none() {
    let who = Principal::anonymous();
    let h = head(ApiKey::Heartbeat, 4, "c");
    let r = heartbeat("g");
    let m = Matcher {
        topics: vec![pat("orders")],
        ..Matcher::default()
    };
    assert!(!m.fits(&ask(None, &who, &h, Some(&r))));
    assert!(!m.fits(&ask(None, &who, &h, None)));
}

#[test]
fn a_group_rule_fits_group_apis_by_their_group() {
    let who = Principal::anonymous();
    let h = head(ApiKey::Heartbeat, 4, "c");
    let r = heartbeat("payments-workers");
    let m = |g: &str| Matcher {
        group: Some(pat(g)),
        ..Matcher::default()
    };
    assert!(m("payments-*").fits(&ask(None, &who, &h, Some(&r))));
    assert!(!m("fulfilment").fits(&ask(None, &who, &h, Some(&r))));
    let (hp, rp) = (head(ApiKey::Produce, 9, "c"), produce("t", &[0], 1));
    assert!(!m("payments-*").fits(&ask(None, &who, &hp, Some(&rp))));
}

#[test]
fn metadata_names_topics_and_a_connection_limit_keeps_to_the_connections_that_were_open() {
    let who = Principal::anonymous();
    let h = head(ApiKey::Metadata, 12, "catalog");
    let r = metadata(&["inventory.levels"]);
    let m = Matcher {
        topics: vec![pat("inventory.*")],
        up_to_conn: Some(5),
        ..Matcher::default()
    };
    assert!(m.fits(&ask(Some(5), &who, &h, Some(&r))));
    assert!(m.fits(&ask(Some(1), &who, &h, Some(&r))));
    assert!(!m.fits(&ask(Some(6), &who, &h, Some(&r))));
    // A server that numbers no connections cannot say which were open: the limit never fits.
    assert!(!m.fits(&ask(None, &who, &h, Some(&r))));
}

#[test]
fn a_request_with_no_body_fits_only_a_matcher_that_names_nothing_in_a_body() {
    let who = Principal::anonymous();
    let h = head(ApiKey::ApiVersions, 3, "catalog");
    let by_client = Matcher {
        client: Some(pat("catalog")),
        ..Matcher::default()
    };
    assert!(by_client.fits(&ask(None, &who, &h, None)));
    let by_topic = Matcher {
        topics: vec![pat("orders")],
        ..Matcher::default()
    };
    assert!(!by_topic.fits(&ask(None, &who, &h, None)));
}
