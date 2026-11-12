use super::*;
use crate::base::Base;
use crate::fixtures_config as larder;
use crate::gather::ConfigFields;
use crate::model::{ConfigAsk, ConfigResult, ConfigShow};

fn page_of(ask: ConfigAsk) -> String {
    let p = larder::page(larder::TOPIC, ask).unwrap();
    config_page(&p, &Base::root()).into_string()
}

fn dialog_of(key: &str, op: Option<&str>, value: &str, other: Option<&str>) -> String {
    let v = larder::form(
        larder::TOPIC,
        ConfigFields {
            key,
            op,
            value: Some(value),
            other,
        },
    )
    .unwrap();
    edit_dialog(&v, &Base::root(), true).into_string()
}

#[test]
fn a_filter_keeps_the_rows_that_match_the_name_or_what_kafka_says_and_a_chip_keeps_its_group() {
    let by_name = page_of(ConfigAsk {
        q: "SEGMENT.b".into(),
        ..ConfigAsk::default()
    });
    assert!(
        by_name.contains(">segment.bytes<") && !by_name.contains(">retention.ms<"),
        "{by_name:.200}"
    );
    let by_text = page_of(ConfigAsk {
        q: "fsync".into(),
        ..ConfigAsk::default()
    });
    assert!(
        by_text.contains(">flush.ms<")
            && by_text.contains(">flush.messages<")
            && !by_text.contains(">segment.bytes<")
    );
    let none = page_of(ConfigAsk {
        q: "zzzz".into(),
        ..ConfigAsk::default()
    });
    assert!(none.contains("No config matches."));
    let changed = page_of(ConfigAsk {
        show: ConfigShow::Changed,
        ..ConfigAsk::default()
    });
    assert!(changed.contains(">cleanup.policy<") && !changed.contains(">preallocate<"));
    let compaction = page_of(ConfigAsk {
        show: ConfigShow::Compaction,
        ..ConfigAsk::default()
    });
    assert!(
        compaction.contains(">min.cleanable.dirty.ratio<") && !compaction.contains(">segment.ms<")
    );
}

#[test]
fn the_replication_group_is_shut_until_asked_for_and_says_how_many_it_holds() {
    let shut = page_of(ConfigAsk::default());
    assert!(shut.contains("9 configs Kafka lists that this broker has no use for"));
    assert!(!shut.contains(">min.insync.replicas<"));
    assert!(shut.contains("href=\"/topics/inventory.levels/config?replication=1\""));
    let open = page_of(ConfigAsk {
        replication: true,
        ..ConfigAsk::default()
    });
    assert!(open.contains(">min.insync.replicas<") && !open.contains(">Hide<"));
    assert!(open.contains("Hide 9"));
}

#[test]
fn an_expander_is_a_link_to_the_same_page_with_that_part_opened_and_the_same_link_shuts_it() {
    let shut = page_of(ConfigAsk::default());
    assert!(
        shut.contains("config?open=where%3Asegment.bytes#c-segment.bytes"),
        "{shut:.100}"
    );
    assert!(!shut.contains("Where segment.bytes comes from."));
    let open = page_of(ConfigAsk {
        open: vec![token("where", "segment.bytes")],
        ..ConfigAsk::default()
    });
    assert!(open.contains("Where segment.bytes comes from."));
    assert!(open.contains("Reset the topic value and the flag, 134217728, takes over."));
    assert!(
        open.contains("<span class=\"v\">none</span>"),
        "places with no value say none"
    );
    assert!(
        open.contains("href=\"/topics/inventory.levels/config#c-segment.bytes\""),
        "the same link shuts it"
    );
}

#[test]
fn the_places_a_value_can_come_from_are_marked_by_the_one_that_wins() {
    let open = page_of(ConfigAsk {
        open: vec![token("where", "retention.ms")],
        ..ConfigAsk::default()
    });
    let rules = &open[open.find("class=\"rules\"").unwrap()..];
    let first_ok = rules.find("class=\"r ok\"").unwrap();
    assert_eq!(
        rules[..first_ok].matches("class=\"r no\"").count(),
        3,
        "three places have no value before the flag: {rules:.900}"
    );
    assert!(rules[first_ok..].starts_with("class=\"r ok\""));
    assert!(
        rules[first_ok..first_ok + 400].contains("Static flag"),
        "the flag wins for retention.ms"
    );
    assert_eq!(rules.matches("class=\"r ok\"").count(), 1, "one place wins");
}

#[test]
fn every_edit_and_reset_is_a_plain_link_a_script_can_open_over_the_page() {
    let doc = page_of(ConfigAsk::default());
    assert!(doc.contains("<a class=\"btn sm\" href=\"/topics/inventory.levels/config/edit?key=segment.ms\" data-dialog>Edit</a>"));
    assert!(doc.contains(
        "href=\"/topics/inventory.levels/config/reset?key=cleanup.policy\" data-dialog>Reset</a>"
    ));
    assert!(
        !doc.contains("config/reset?key=segment.ms"),
        "only what the topic sets can be reset"
    );
}

#[test]
fn a_label_is_bound_to_its_input_in_every_dialog() {
    for (key, op, value, other) in [
        ("min.cleanable.dirty.ratio", None, "0.7", None),
        ("cleanup.policy", Some("append"), "delete", None),
        ("min.compaction.lag.ms", None, "1", Some("2")),
    ] {
        let doc = dialog_of(key, op, value, other);
        for part in doc.split("<label for=\"").skip(1) {
            let id = &part[..part.find('"').unwrap()];
            assert!(
                doc.contains(&format!("id=\"{id}\"")),
                "{key}: label for {id} has no input"
            );
        }
        assert!(
            doc.contains("aria-labelledby=\"config-edit-title\""),
            "{key}"
        );
        assert!(
            doc.contains("name=\"key\""),
            "{key}: the form carries which config"
        );
    }
}

#[test]
fn a_list_dialog_offers_the_three_operations_with_the_one_asked_for_chosen() {
    let doc = dialog_of("cleanup.policy", Some("subtract"), "delete", None);
    assert!(doc.contains("value=\"subtract\" checked"), "{doc:.100}");
    assert!(doc.contains("value=\"append\"") && doc.contains("value=\"set\""));
    assert!(doc.contains("Policy to take out"));
    let default = dialog_of("cleanup.policy", None, "", None);
    assert!(default.contains("value=\"append\" checked") && default.contains("Policy to append"));
    assert!(
        default.contains("<span class=\"pill mono\">compact</span>"),
        "what the list holds now"
    );
}

#[test]
fn a_dialog_shows_a_field_error_and_a_dry_runs_answer_and_nothing_when_there_is_none() {
    let plain = dialog_of("segment.bytes", None, "2048", None);
    assert!(!plain.contains("class=\"help err\"") && !plain.contains("Kafka would take this."));
    let ConfigResult::Invalid(v) = larder::save(
        larder::TOPIC,
        ConfigFields {
            key: "segment.bytes",
            op: None,
            value: Some(""),
            other: None,
        },
        false,
    ) else {
        panic!()
    };
    let doc = edit_dialog(&v, &Base::root(), true).into_string();
    assert!(
        doc.contains("<div class=\"help err\">Type a value.</div>"),
        "{doc:.1500}"
    );
    assert!(doc.contains("aria-invalid=\"true\""));
    let ConfigResult::Checked(v) = larder::save(
        larder::TOPIC,
        ConfigFields {
            key: "segment.bytes",
            op: None,
            value: Some("2048"),
            other: None,
        },
        true,
    ) else {
        panic!()
    };
    let doc = edit_dialog(&v, &Base::root(), true).into_string();
    assert!(doc.contains("Kafka would take this."));
    assert!(doc.contains("8388608 <span class=\"dim\">to</span> 2048"));
}

#[test]
fn names_and_values_from_the_broker_are_text_in_every_place_they_are_drawn() {
    let mut p = larder::page(larder::TOPIC, ConfigAsk::default()).unwrap();
    let evil = "<img src=x onerror=alert(1)>";
    let i = p
        .rows
        .iter()
        .position(|r| r.name == "segment.bytes")
        .unwrap();
    p.rows[i].name = evil.to_owned();
    p.rows[i].value = evil.to_owned();
    p.rows[i].doc = format!("{evil}. Second <b>sentence</b>.");
    p.rows[i].synonyms[0].name = evil.to_owned();
    p.ask.open = vec![token("where", evil), token("text", evil)];
    let doc = config_page(&p, &Base::root()).into_string();
    assert!(!doc.contains("<img"), "{doc:.600}");
    assert!(doc.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!doc.contains("<b>sentence</b>"));
}
