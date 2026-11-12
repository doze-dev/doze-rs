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

use axum::http::StatusCode;
use kafka_console::backend::requests::forms::topic_configs;
use kafka_console::config_effects::{Moment, moment};
use kafka_wire::api::Response;
use support::get;
use support_config::{TOPIC, described, seeded};
use support_write::{page, post_form};

/// CON-251: the page lists the configs the broker lists, each with its source, Kafka's text and a sentence on when it counts.
#[tokio::test]
async fn the_page_lists_what_the_broker_lists_with_its_source_and_when_a_change_counts() {
    let w = seeded("cfg-page").await;
    let app = w.router();
    let (status, doc) = page(&app, get(&format!("/topics/{TOPIC}/config?replication=1"))).await;
    assert_eq!(status, StatusCode::OK);
    let Response::DescribeConfigs(r) = w.ask(4, topic_configs(TOPIC, &[])).await else {
        panic!()
    };
    let listed: Vec<String> = r.results[0]
        .configs
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(listed.len(), 33, "{listed:?}");
    for key in &listed {
        assert!(
            doc.contains(&format!("<span class=\"mono bright\">{key}</span>")),
            "the page does not list {key}"
        );
    }
    assert!(
        doc.contains("<b>33</b> configs. <b>3</b> are set on this topic"),
        "{doc:.3000}"
    );
    assert!(doc.contains("<span class=\"st ok\">Compacted</span>"));
    assert!(doc.contains("<span class=\"tag\">Dynamic topic</span>"));
    assert!(doc.contains("<span class=\"tag dim\">Default</span>"));
    assert!(
        doc.contains("within 15 s"),
        "the cleaner's pace is the broker's own"
    );
    assert!(doc.contains("within 5 m"), "and so is retention's");
    // Kafka's text, as the broker gives it.
    assert!(doc.contains("This config designates the retention policy to use on log segments."));
    w.stop().await;
}

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

/// CON-253: a single value goes through IncrementalAlterConfigs SET, and the page shows what the broker has before and after.
#[tokio::test]
async fn a_value_is_set_with_the_call_a_client_sends_and_read_back_from_the_broker() {
    let w = seeded("cfg-set").await;
    let app = w.router();
    assert_eq!(
        described(&w, "min.cleanable.dirty.ratio").await,
        Some(("0.5".to_owned(), 5))
    );
    let (status, doc) = page(
        &app,
        post_form(
            &format!("/topics/{TOPIC}/config/edit"),
            &[("key", "min.cleanable.dirty.ratio"), ("value", "0.7")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        doc.contains("<b>Saved.</b> IncrementalAlterConfigs answered"),
        "{doc:.2500}"
    );
    assert!(doc.contains("<b>min.cleanable.dirty.ratio</b> is now <b>0.7</b>, set on this topic."));
    assert_eq!(
        described(&w, "min.cleanable.dirty.ratio").await,
        Some(("0.7".to_owned(), 1))
    );
    w.stop().await;
}

/// CON-254: a list is appended to and subtracted from, element by element.
#[tokio::test]
async fn a_list_is_appended_to_and_subtracted_from() {
    let w = seeded("cfg-list").await;
    let app = w.router();
    let path = format!("/topics/{TOPIC}/config/edit");
    let (_, doc) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "cleanup.policy"),
                ("op", "append"),
                ("value", "delete"),
            ],
        ),
    )
    .await;
    assert!(doc.contains("<b>Saved.</b>"), "{doc:.2500}");
    assert_eq!(
        described(&w, "cleanup.policy").await,
        Some(("compact,delete".to_owned(), 1))
    );
    let (_, doc) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "cleanup.policy"),
                ("op", "subtract"),
                ("value", "delete"),
            ],
        ),
    )
    .await;
    assert!(doc.contains("<b>Saved.</b>"), "{doc:.2500}");
    assert_eq!(
        described(&w, "cleanup.policy").await,
        Some(("compact".to_owned(), 1))
    );
    // Set replaces it all.
    page(
        &app,
        post_form(
            &path,
            &[
                ("key", "cleanup.policy"),
                ("op", "set"),
                ("value", "delete"),
            ],
        ),
    )
    .await;
    assert_eq!(
        described(&w, "cleanup.policy").await,
        Some(("delete".to_owned(), 1))
    );
    w.stop().await;
}

/// CON-255: the two compaction lags are one request; Kafka's refusal of the pair names both and changes neither.
#[tokio::test]
async fn the_two_lags_are_saved_together_and_a_crossed_pair_is_refused_whole() {
    let w = seeded("cfg-lags").await;
    let app = w.router();
    let path = format!("/topics/{TOPIC}/config/edit");
    let (_, ok) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "min.compaction.lag.ms"),
                ("value", "3600000"),
                ("other", "7200000"),
            ],
        ),
    )
    .await;
    assert!(ok.contains("<b>min.compaction.lag.ms</b> and <b>max.compaction.lag.ms</b> are now <b>3600000</b> and <b>7200000</b>"), "{ok:.2500}");
    assert_eq!(
        described(&w, "max.compaction.lag.ms").await,
        Some(("7200000".to_owned(), 1))
    );
    let (status, doc) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "min.compaction.lag.ms"),
                ("value", "9000000"),
                ("other", "100"),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("INVALID_CONFIG (40)"), "{doc:.2500}");
    assert!(doc.contains("conflict topic config setting min.compaction.lag.ms (9000000) &gt; max.compaction.lag.ms (100)"));
    // What would pass was asked of the broker, not worked out here.
    assert!(
        doc.contains("<dt>min</dt><dd class=\"mono\">100"),
        "{doc:.4000}"
    );
    assert!(doc.contains("<dt>max</dt><dd class=\"mono\">9000000"));
    assert_eq!(
        described(&w, "min.compaction.lag.ms").await,
        Some(("3600000".to_owned(), 1)),
        "nothing was changed"
    );
    assert_eq!(
        described(&w, "max.compaction.lag.ms").await,
        Some(("7200000".to_owned(), 1))
    );
    w.stop().await;
}

/// CON-256: a value Kafka would not take shows its code and words; what would pass and the other ranges are the broker's answers.
#[tokio::test]
async fn a_refused_value_shows_kafkas_words_and_only_what_the_broker_accepted_as_what_would_pass() {
    let w = seeded("cfg-value").await;
    let app = w.router();
    let path = format!("/topics/{TOPIC}/config/edit");
    let (status, doc) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "cleanup.policy"),
                ("op", "append"),
                ("value", "compacted"),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("Invalid value compact,compacted for configuration cleanup.policy: String must be one of: compact, delete"), "{doc:.2500}");
    assert!(
        doc.contains("cleanup.policy  APPEND  delete"),
        "the broker took delete"
    );
    assert!(doc.contains("-&gt; compact,delete"));
    assert!(doc.contains(
        "Invalid value 512 for configuration segment.bytes: Value must be at least 1024"
    ));
    assert!(
        doc.contains("Invalid value abc for configuration segment.bytes: Not a number of type INT")
    );
    assert_eq!(
        described(&w, "cleanup.policy").await,
        Some(("compact".to_owned(), 1))
    );
    // A bound the broker names is tried: segment.bytes below its minimum.
    let (_, low) = page(
        &app,
        post_form(&path, &[("key", "segment.bytes"), ("value", "512")]),
    )
    .await;
    assert!(low.contains("segment.bytes  SET  1024"), "{low:.3000}");
    assert!(
        !low.contains("<td class=\"mono\">segment.bytes 512</td>"),
        "the value just sent is not an 'other' range"
    );
    w.stop().await;
}
