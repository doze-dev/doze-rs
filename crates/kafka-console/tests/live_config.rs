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
mod support_write;

use std::path::PathBuf;

use axum::http::StatusCode;
use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_console::backend::requests::forms::topic_configs;
use kafka_console::config_effects::{Moment, moment};
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::ApiKey;
use support::get;
use support_write::{World, page, post_form, router_of, world};

const TOPIC: &str = "inventory.levels";

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff-live")
}

async fn seeded(name: &str) -> World {
    let w = world(name, |_, _| {}).await;
    w.create_topic(
        TOPIC,
        1,
        &[
            ("cleanup.policy", "compact"),
            ("segment.bytes", "8388608"),
            ("min.compaction.lag.ms", "0"),
        ],
    )
    .await;
    w.create_topic("orders", 2, &[]).await;
    w
}

/// A config of the topic as DescribeConfigs says it: value and source number.
async fn described(w: &World, key: &str) -> Option<(String, i8)> {
    let Response::DescribeConfigs(r) = w.ask(4, topic_configs(TOPIC, &[key])).await else {
        panic!()
    };
    r.results[0]
        .configs
        .iter()
        .find(|c| c.name.as_str() == key)
        .map(|c| {
            (
                c.value
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                c.config_source,
            )
        })
}

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

/// CON-257: a reset is IncrementalAlterConfigs DELETE: the topic follows the broker again.
#[tokio::test]
async fn a_reset_removes_the_topics_value_and_the_broker_takes_over() {
    let w = seeded("cfg-reset").await;
    let app = w.router();
    let (_, dialog) = page(
        &app,
        get(&format!("/topics/{TOPIC}/config/reset?key=segment.bytes")),
    )
    .await;
    assert!(
        dialog.contains("Reset segment.bytes to default?"),
        "{dialog:.2500}"
    );
    assert!(
        dialog.contains("<dt>Now</dt>")
            && dialog.contains("8388608")
            && dialog.contains("segment.bytes  DELETE")
    );
    assert_eq!(described(&w, "segment.bytes").await.unwrap().1, 1);
    let res = {
        use tower::ServiceExt;
        app.clone()
            .oneshot(post_form(
                &format!("/topics/{TOPIC}/config/reset"),
                &[("key", "segment.bytes")],
            ))
            .await
            .unwrap()
    };
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()["location"], format!("/topics/{TOPIC}/config"));
    let (value, source) = described(&w, "segment.bytes").await.unwrap();
    assert_ne!(source, 1, "no longer set on the topic");
    assert_ne!(value, "8388608");
    w.stop().await;
}

/// CON-258: Check asks the broker with validate_only: what it accepts is said so, and nothing changes either way.
#[tokio::test]
async fn a_check_is_the_same_call_with_validate_only_and_changes_nothing() {
    let w = seeded("cfg-check").await;
    let app = w.router();
    let path = format!("/topics/{TOPIC}/config/edit");
    let (status, ok) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "cleanup.policy"),
                ("op", "append"),
                ("value", "delete"),
                ("check", "1"),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(ok.contains("Kafka would take this."), "{ok:.3000}");
    assert!(
        ok.contains("<dd class=\"mono\">compact <span class=\"dim\">to</span> compact,delete</dd>")
    );
    assert_eq!(
        described(&w, "cleanup.policy").await,
        Some(("compact".to_owned(), 1))
    );
    let (_, bad) = page(
        &app,
        post_form(
            &path,
            &[("key", "segment.bytes"), ("value", "12"), ("check", "1")],
        ),
    )
    .await;
    assert!(
        bad.contains(
            "Invalid value 12 for configuration segment.bytes: Value must be at least 1024"
        ),
        "{bad:.2500}"
    );
    assert_eq!(
        described(&w, "segment.bytes").await,
        Some(("8388608".to_owned(), 1))
    );
    w.stop().await;
}

/// CON-259: an empty form is the dialog again with its error, not a call.
#[tokio::test]
async fn an_empty_value_is_the_dialog_again_with_the_error_and_no_call() {
    let w = seeded("cfg-empty").await;
    let (spy, log) = w.backend.recording();
    let app = router_of(&w, spy);
    let (status, doc) = page(
        &app,
        post_form(
            &format!("/topics/{TOPIC}/config/edit"),
            &[("key", "retention.ms"), ("value", "  ")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(doc.contains("Type a value."), "{doc:.2500}");
    assert!(
        !log.lock()
            .unwrap()
            .contains(&ApiKey::IncrementalAlterConfigs)
    );
    w.stop().await;
}

fn grant(op: i8) -> kafka_wire::messages::create_acls_request::AclCreation {
    let mut a = kafka_wire::messages::create_acls_request::AclCreation::default();
    a.resource_type = 2;
    a.resource_name = StrBytes::from_static_str("orders");
    a.resource_pattern_type = 3;
    a.principal = StrBytes::from_static_str("User:ANONYMOUS");
    a.host = StrBytes::from_static_str("*");
    a.operation = op;
    a.permission_type = 3;
    a
}

/// CON-260: a caller that may describe the configs and not alter them is refused in Kafka's words; one that may not
/// describe them sees no topic.
#[tokio::test]
async fn on_a_broker_with_acls_the_page_reads_and_the_save_is_refused_as_a_client_would_be() {
    let w = world("cfg-acl", |b, _| b.acl_default_allow = false).await;
    w.create_topic("orders", 1, &[]).await;
    w.create_topic("secret", 1, &[]).await;
    let mut creation = kafka_wire::messages::CreateAclsRequest::default();
    creation.creations = vec![grant(10)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(creation)).await else {
        panic!()
    };
    assert_eq!(r.results[0].error_code, 0, "{r:?}");
    let someone = Caller::new(Principal::anonymous(), Some("127.0.0.1".parse().unwrap()));
    let app = router_of(&w, w.backend.acting_as(someone));
    let (status, doc) = page(&app, get("/topics/orders/config")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("<b>33</b> configs."), "{doc:.1500}");
    assert!(
        !doc.contains("within 15 s"),
        "the pace is the cluster's to describe, and this caller may not"
    );
    let (_, refused) = page(
        &app,
        post_form(
            "/topics/orders/config/edit",
            &[("key", "retention.ms"), ("value", "1000")],
        ),
    )
    .await;
    assert!(
        refused.contains("TOPIC_AUTHORIZATION_FAILED (29)"),
        "{refused:.2500}"
    );
    assert!(refused.contains("Topic authorization failed."));
    assert_ne!(
        {
            let Response::DescribeConfigs(r) =
                w.ask(4, topic_configs("orders", &["retention.ms"])).await
            else {
                panic!()
            };
            r.results[0].configs[0]
                .value
                .as_ref()
                .map(ToString::to_string)
        },
        Some("1000".to_owned())
    );
    let (status, hidden) = page(&app, get("/topics/secret/config")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(hidden.contains("There is no topic called"));
    w.stop().await;
}

/// CON-261: hostile text is escaped wherever it is shown, and a newline typed cannot make a line of the call.
#[tokio::test]
async fn hostile_text_is_escaped_and_cannot_forge_a_line_of_the_call() {
    let w = seeded("cfg-hostile").await;
    let app = w.router();
    let path = format!("/topics/{TOPIC}/config/edit");
    let evil = "<script>alert(1)</script>\"'&\n  min.compaction.lag.ms  SET  1";
    let (status, saved) = page(
        &app,
        post_form(
            &path,
            &[
                ("key", "leader.replication.throttled.replicas"),
                ("op", "set"),
                ("value", evil),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!saved.contains("<script>alert(1)"), "{saved:.3000}");
    assert!(saved.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    let (_, key) = page(
        &app,
        get(&format!(
            "/topics/{TOPIC}/config/edit?key=%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E"
        )),
    )
    .await;
    assert!(!key.contains("<img src=x"), "{key:.2000}");
    assert!(key.contains("has no config called"));
    let (status, unknown) = page(&app, get("/topics/%3Cb%3Ex%3C%2Fb%3E/config")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!unknown.contains("<b>x</b>"));
    // A dialog opened with a hostile value in the address shows it as text in the field and in the call.
    let (_, dialog) = page(&app, get(&format!("/topics/{TOPIC}/config/edit?key=retention.ms&value=%22%3E%3Cscript%3Ex%3C%2Fscript%3E%0Afake%20%20SET"))).await;
    assert!(!dialog.contains("<script>x"), "{dialog:.3000}");
    assert!(
        dialog.contains("\\n"),
        "the newline is written out in the call"
    );
    // Hostile open tokens and filters are text too.
    let (_, listed) = page(
        &app,
        get(&format!(
            "/topics/{TOPIC}/config?q=%3Cscript%3E&open=%3Cscript%3E,where:segment.bytes"
        )),
    )
    .await;
    assert!(!listed.contains("<script>"), "{listed:.600}");
    w.stop().await;
}

/// CON-262: a GET of any Config address only reads, and a POST shows its write to the spy.
#[tokio::test]
async fn a_get_of_the_config_pages_never_changes_the_broker() {
    let w = seeded("cfg-spy").await;
    let (spy, log) = w.backend.recording();
    let app = router_of(&w, spy);
    let before = described(&w, "segment.bytes").await;
    for path in [
        format!("/topics/{TOPIC}/config"),
        format!(
            "/topics/{TOPIC}/config?show=changed&q=seg&open=where:segment.bytes,text:retention.ms&replication=1"
        ),
        format!("/topics/{TOPIC}/config/edit?key=cleanup.policy&op=append&value=delete"),
        format!("/topics/{TOPIC}/config/edit?key=min.compaction.lag.ms&value=1&other=2"),
        format!("/topics/{TOPIC}/config/reset?key=segment.bytes"),
        "/topics/nope/config".to_owned(),
    ] {
        let (status, _) = page(&app, get(&path)).await;
        assert!(status.as_u16() < 500, "{path} answered {status}");
    }
    let calls = log.lock().unwrap().clone();
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|c| *c == ApiKey::DescribeConfigs),
        "{calls:?}"
    );
    assert_eq!(described(&w, "segment.bytes").await, before);
    page(
        &app,
        post_form(
            &format!("/topics/{TOPIC}/config/edit"),
            &[("key", "retention.ms"), ("value", "1000")],
        ),
    )
    .await;
    assert!(
        log.lock()
            .unwrap()
            .contains(&ApiKey::IncrementalAlterConfigs)
    );
    w.stop().await;
}

/// CON-263: the eight boards rendered from the real broker, for the structure gate.
#[tokio::test]
async fn the_eight_boards_are_rendered_from_a_real_broker() {
    let w = seeded("cfg-boards").await;
    let app = w.router();
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let t = format!("/topics/{TOPIC}/config");
    let edit = format!("{t}/edit");
    let boards: Vec<(&str, bool, String, Vec<(&str, &str)>)> = vec![
        (
            "topic-config",
            false,
            format!("{t}?open=where:segment.bytes,text:min.cleanable.dirty.ratio"),
            vec![],
        ),
        (
            "config-edit",
            false,
            format!("{edit}?key=min.cleanable.dirty.ratio&value=0.7"),
            vec![],
        ),
        (
            "config-edit-list",
            false,
            format!("{edit}?key=cleanup.policy&op=append&value=compacted"),
            vec![],
        ),
        (
            "config-edit-lag",
            false,
            format!("{edit}?key=min.compaction.lag.ms&value=7200000&other=3600000"),
            vec![],
        ),
        (
            "config-saved",
            true,
            edit.clone(),
            vec![("key", "min.cleanable.dirty.ratio"), ("value", "0.7")],
        ),
        (
            "config-refused",
            true,
            edit.clone(),
            vec![
                ("key", "min.compaction.lag.ms"),
                ("value", "7200000"),
                ("other", "3600000"),
            ],
        ),
        (
            "config-refused-value",
            true,
            edit.clone(),
            vec![
                ("key", "cleanup.policy"),
                ("op", "append"),
                ("value", "compacted"),
            ],
        ),
        (
            "config-reset",
            false,
            format!("{t}/reset?key=segment.bytes"),
            vec![],
        ),
    ];
    for (id, is_post, path, fields) in boards {
        let req = if is_post {
            post_form(&path, &fields)
        } else {
            get(&path)
        };
        let (status, doc) = page(&app, req).await;
        assert_eq!(status, StatusCode::OK, "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    w.stop().await;
}
