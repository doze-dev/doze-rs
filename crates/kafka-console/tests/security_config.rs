//! Hostile strings in every field of every Config view-model come out inert: the same rule as `tests/security.rs`, for the
//! pages of slice 9d (CON-261).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use kafka_console::base::Base;
use kafka_console::fixtures_config as larder;
use kafka_console::gather::ConfigFields;
use kafka_console::model::{ConfigAsk, ConfigResult, ConfigSynonym};
use kafka_console::pages;

const PAYLOADS: [&str; 4] = [
    "<script>alert(1)</script>",
    "\"><img src=x onerror=alert(1)>",
    "' onmouseover='alert(1)",
    "</textarea><svg/onload=alert(1)>",
];

fn inert(doc: &str, what: &str) {
    for p in PAYLOADS {
        assert!(
            !p.contains('<') || !doc.contains(p),
            "{what}: {p} reached the page raw"
        );
    }
    for bad in ["<script", "<img", "<svg/"] {
        assert!(!doc.to_lowercase().contains(bad), "{what}: {bad} is markup");
    }
}

#[test]
fn hostile_text_in_every_field_of_every_config_page_comes_out_inert() {
    let base = Base::root();
    for p in PAYLOADS {
        let mut page = larder::page(larder::TOPIC, ConfigAsk::default()).unwrap();
        for r in &mut page.rows {
            r.value = p.to_owned();
            r.doc = format!("{p}. {p}");
            r.allowed = p.to_owned();
            r.synonyms.push(ConfigSynonym {
                name: p.to_owned(),
                value: p.to_owned(),
                source: kafka_console::model::ConfigSource::StaticFlag,
            });
        }
        page.head.name = p.to_owned();
        page.ask.q = p.to_owned();
        page.ask.open = page
            .rows
            .iter()
            .flat_map(|r| {
                [
                    pages::config_token("where", &r.name),
                    pages::config_token("text", &r.name),
                ]
            })
            .take(8)
            .collect();
        page.ask.replication = true;
        inert(
            &pages::config_page(
                &page,
                Some(&kafka_console::fixtures_topics::head("orders").unwrap()),
                &base,
            )
            .into_string(),
            "the page",
        );

        let f = ConfigFields {
            key: "cleanup.policy",
            op: Some("append"),
            value: Some(p),
            other: Some(p),
        };
        let ConfigResult::Refused(mut r) = larder::save(larder::TOPIC, f, false) else {
            panic!("the larder refuses a policy that is not one")
        };
        r.refusal.message = p.to_owned();
        r.refusal.name = p.to_owned();
        r.others.push((p.to_owned(), p.to_owned()));
        r.pass[0].lines.push(p.to_owned());
        r.pass[0].result = Some(p.to_owned());
        r.head.name = p.to_owned();
        inert(
            &pages::config_refused(&r, &base).into_string(),
            "the refusal",
        );

        let f = ConfigFields {
            key: "leader.replication.throttled.replicas",
            op: Some("set"),
            value: Some(p),
            other: None,
        };
        let ConfigResult::Saved(mut s) = larder::save(larder::TOPIC, f, false) else {
            panic!("the larder takes any text for a throttled list")
        };
        s.head.name = p.to_owned();
        s.changes[0].name = p.to_owned();
        inert(
            &pages::config_saved(&s, &base).into_string(),
            "the saved page",
        );

        let mut v = larder::form(
            larder::TOPIC,
            ConfigFields {
                key: "min.compaction.lag.ms",
                op: None,
                value: Some(p),
                other: Some(p),
            },
        )
        .unwrap();
        v.error = Some(p.to_owned());
        v.topic = p.to_owned();
        inert(
            &pages::config_edit_dialog(&v, &base).into_string(),
            "the dialog",
        );

        let mut reset = larder::reset_form(larder::TOPIC, "segment.bytes").unwrap();
        reset.topic = p.to_owned();
        reset.row.value = p.to_owned();
        inert(
            &pages::config_reset_dialog(&reset, &base).into_string(),
            "the reset dialog",
        );
    }
}
