//! The contract model's own rules.

use super::*;

fn rule(name: &str, mode: RuleMode) -> Rule {
    Rule {
        name: Some(name.into()),
        doc: None,
        kind: RuleKind::Condition,
        mode,
        rule_type: Some("CEL".into()),
        tags: BTreeSet::new(),
        params: BTreeMap::new(),
        expr: Some("true".into()),
        on_success: None,
        on_failure: None,
        disabled: false,
    }
}

fn props(kv: &[(&str, &str)]) -> Metadata {
    Metadata {
        properties: kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
        ..Metadata::default()
    }
}

// Recorded from cp-schema-registry 7.7.1 and 8.3.2 (tools/registry-diff, data-contracts/config-defaults-and-overrides).
#[test]
fn defaults_lie_under_and_overrides_over_what_a_registration_gives() {
    let cfg = ContractConfig {
        default_metadata: Some(props(&[("team", "x"), ("owner", "dflt")])),
        override_metadata: Some(Metadata {
            sensitive: ["k".to_owned()].into(),
            ..props(&[("owner", "forced")])
        }),
        ..ContractConfig::default()
    };
    let own = Contract {
        metadata: Some(props(&[("owner", "mine"), ("other", "y")])),
        rule_set: None,
    };
    let got = cfg.apply(&own, None, None, false);
    assert_eq!(
        json::metadata_text(got.metadata.as_ref().unwrap()),
        r#"{"properties":{"other":"y","owner":"forced","team":"x"},"sensitive":["k"]}"#
    );
    // Nothing of its own: the previous version's.
    let prev = Contract {
        metadata: Some(props(&[("p", "1")])),
        rule_set: None,
    };
    let got = ContractConfig::default().apply(&Contract::default(), Some(&prev), Some(4), true);
    assert_eq!(
        got.metadata.unwrap().properties,
        BTreeMap::from([
            ("confluent:version".into(), "4".into()),
            ("p".into(), "1".into())
        ])
    );
}

#[test]
fn a_rule_of_the_same_name_replaces_the_older_one() {
    let old = RuleSet {
        domain: vec![rule("d1", RuleMode::Write), rule("shared", RuleMode::Write)],
        ..RuleSet::default()
    };
    let new = RuleSet {
        domain: vec![rule("shared", RuleMode::Read)],
        ..RuleSet::default()
    };
    let m = merge_rule_sets(Some(&old), Some(&new)).unwrap();
    let names: Vec<_> = m
        .domain
        .iter()
        .map(|r| (r.name.clone().unwrap(), r.mode))
        .collect();
    assert_eq!(
        names,
        [
            ("d1".into(), RuleMode::Write),
            ("shared".into(), RuleMode::Read)
        ]
    );
}

#[test]
fn rule_sets_are_checked_as_confluent_checks_them() {
    let bad = |r: RuleSet| r.validate().unwrap_err();
    let one = |r: Rule| RuleSet {
        domain: vec![r],
        ..RuleSet::default()
    };
    assert_eq!(
        bad(RuleSet {
            domain: vec![rule("a", RuleMode::Write), rule("a", RuleMode::Write)],
            ..RuleSet::default()
        }),
        "Found rule with duplicate name 'a'"
    );
    assert_eq!(bad(one(rule("", RuleMode::Write))), "Empty rule name");
    assert_eq!(
        bad(one(rule("1a", RuleMode::Write))),
        "Illegal initial character in rule name: 1a"
    );
    assert_eq!(
        bad(one(rule("a.b", RuleMode::Write))),
        "Illegal character in rule name: a.b"
    );
    assert_eq!(
        bad(one(rule(&"x".repeat(65), RuleMode::Write))),
        "Rule name too long"
    );
    assert!(
        one(rule(&"x".repeat(64), RuleMode::Write))
            .validate()
            .is_ok()
    );
    assert_eq!(
        bad(one(Rule {
            rule_type: None,
            ..rule("t", RuleMode::Write)
        })),
        "Missing rule type"
    );
    assert_eq!(
        bad(one(Rule {
            on_failure: Some("ERROR,NONE".into()),
            ..rule("m", RuleMode::Write)
        })),
        "Multiple actions only valid with WRITEREAD and UPDOWN"
    );
    assert_eq!(
        bad(one(rule("u", RuleMode::Upgrade))),
        "Domain rules can only be WRITE, READ, WRITEREAD"
    );
    assert_eq!(
        bad(RuleSet {
            migration: vec![rule("w", RuleMode::Write)],
            ..RuleSet::default()
        }),
        "Migration rules can only be UPGRADE, DOWNGRADE, UPDOWN"
    );
}
