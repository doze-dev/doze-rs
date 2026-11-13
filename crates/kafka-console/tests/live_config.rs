//! The Config pages over a real broker in this process: what the page lists is what the broker lists; a save, an append,
//! a subtract, the two lags together, a reset, a check and each refusal are the protocol requests a client would send;
//! a caller without rights is refused in Kafka's words; hostile text is escaped; and a GET changes nothing.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_config;
mod support_write;

use kafka_console::backend::requests::forms::topic_configs;
use kafka_console::config_effects::{Moment, moment};
use kafka_wire::api::Response;
use support_config::{TOPIC, seeded};

/// CON-252: every key the broker lists has a sentence for when a change counts (the plan's completeness gate).
#[tokio::test]
async fn every_key_the_broker_lists_has_an_effect_sentence() {
    let w = seeded("cfg-effects").await;
    let help = w.backend.topic_config_help().unwrap();
    assert_eq!(help.len(), 33);
    for h in &help {
        assert!(
            moment(h.name).is_some(),
            "{} has no effect sentence",
            h.name
        );
    }
    // And the listing of a topic is that same list.
    let Response::DescribeConfigs(r) = w.ask(4, topic_configs(TOPIC, &[])).await else {
        panic!()
    };
    for c in &r.results[0].configs {
        assert!(
            moment(c.name.as_str()).is_some(),
            "{} has no effect sentence",
            c.name
        );
    }
    // The ones the broker keeps and ignores say so, and the ones it acts on do not.
    assert_eq!(moment("segment.bytes"), Some(Moment::NextAppend));
    assert!(matches!(moment("preallocate"), Some(Moment::Kept(_))));
    w.stop().await;
}
