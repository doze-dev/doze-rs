use super::*;
use crate::lab::form::spec_of;
use crate::model::lab::LabKind;

fn busy() -> Seen {
    Seen {
        topics: vec!["clickstream".into(), "orders".into()],
        producing: Some(("orders".into(), "payments".into())),
        fetching: Some("clickstream".into()),
        clients: vec!["fulfilment".into(), "payments".into()],
        group: Some("fulfilment".into()),
        member: Some("fulfilment/m-1".into()),
        compacted: vec!["inventory.levels".into()],
    }
}

fn by<'a>(all: &'a [LabRecipe], id: &str) -> &'a LabRecipe {
    all.iter().find(|r| r.id == id).expect(id)
}

#[test]
fn recipes_name_the_brokers_own_topics_apps_and_groups() {
    let all = recipes(&busy());
    assert_eq!(all.len(), 9);
    assert!(
        all.iter().all(|r| r.needs.is_none()),
        "a busy broker serves every recipe"
    );
    let p = by(&all, "slow-produce");
    assert_eq!(p.title, "Slow every Produce to orders by 800 ms");
    let f = p.fields.as_ref().unwrap();
    assert_eq!(
        (
            f.api.as_str(),
            f.topic.as_str(),
            f.delay.as_str(),
            f.share.as_str(),
            f.ends.as_str()
        ),
        ("Produce", "orders", "800", "every", "10")
    );
    assert_eq!(
        by(&all, "slow-fetch").fields.as_ref().unwrap().topic,
        "clickstream"
    );
    let l = by(&all, "fail-leader").fields.clone().unwrap();
    assert_eq!((l.code.as_str(), l.share.as_str()), ("6", "5"));
    let t = by(&all, "fail-timeout");
    assert_eq!(t.title, "Time out 1 in 10 Produce from payments to orders");
    assert_eq!(t.fields.as_ref().unwrap().client, "payments");
    assert_eq!(by(&all, "drop-once").target.as_deref(), Some("payments"));
    assert_eq!(by(&all, "drop-flap").how.as_deref(), Some("1"));
    assert_eq!(
        by(&all, "expire-member").target.as_deref(),
        Some("fulfilment/m-1")
    );
    assert_eq!(
        by(&all, "rebalance-group").target.as_deref(),
        Some("fulfilment")
    );
    assert_eq!(
        by(&all, "compact-now").target.as_deref(),
        Some("inventory.levels")
    );
}

#[test]
fn every_rule_recipe_makes_a_rule_the_form_accepts() {
    for r in recipes(&busy()) {
        let Some(f) = &r.fields else { continue };
        let kind = if r.tab == "slow" {
            LabKind::Slow
        } else {
            LabKind::Fail
        };
        assert!(
            spec_of(kind, f).is_ok(),
            "{}: {:?}",
            r.id,
            spec_of(kind, f).err()
        );
    }
}

#[test]
fn an_empty_broker_says_what_each_recipe_needs() {
    let all = recipes(&Seen::default());
    for r in &all {
        assert!(
            r.needs.as_deref().is_some_and(|n| n.starts_with("Needs ")),
            "{}",
            r.id
        );
        assert!(
            r.target.as_deref().is_none_or(|t| !t.is_empty()),
            "{}: no empty target",
            r.id
        );
    }
    assert_eq!(
        by(&all, "slow-produce").title,
        "Slow every Produce to a topic by 800 ms"
    );
    assert_eq!(
        by(&all, "drop-once").needs.as_deref(),
        Some("Needs an app with a connection open.")
    );
    // A topic and no app: the topic recipes serve, the app ones say so.
    let some = recipes(&Seen {
        topics: vec!["orders".into()],
        ..Seen::default()
    });
    assert!(by(&some, "slow-produce").needs.is_none());
    assert!(by(&some, "fail-timeout").needs.is_some());
}
