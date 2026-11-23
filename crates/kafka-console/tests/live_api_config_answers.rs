//! J127: what the Config tab's answers carry, value by value, over a real broker in this process: a topic's cleanup and
//! retention on the topics list and the topic, the rows' allowed values, the edit dialog's input, pace and teaching, what a
//! form misses, a check's before and after, a save's before and now, and a refusal's ways out (what would pass, and what
//! Kafka says to the other samples). The mutation run over `gather/config*.rs` found each of these unchecked.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_api;
mod support_log;
mod support_write;

use serde_json::{Value, json};
use support_api::{api, get, post};
use support_write::world;

fn row<'a>(rows: &'a Value, name: &str) -> &'a Value {
    rows.as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no row {name} in {rows}"))
}

fn pass_of(v: &Value) -> Vec<(Value, Value)> {
    v["detail"]["refused"]["pass"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|p| (p["pairs"].clone(), p["result"].clone()))
        .collect()
}

fn others_of(v: &Value) -> Vec<String> {
    v["detail"]["refused"]["others"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|o| o[0].as_str().unwrap().to_owned())
        .collect()
}

/// The topics list and the topic say a topic's cleanup and its own retention; a topic with none set says no retention of its own.
/// The Config tab's head says the cleanup too, each row what Kafka allows, and the edit dialog the cleaner's and retention's
/// pace, the value it starts from (a single value its own, a list nothing) and, for cleanup.policy, the topic's retention.
#[tokio::test]
async fn the_pages_say_the_topics_cleanup_retention_allowed_values_and_pace() {
    let w = world("api-config-answers-pages", |_, _| {}).await;
    w.create_topic(
        "c",
        1,
        &[("cleanup.policy", "compact"), ("retention.ms", "3600000")],
    )
    .await;
    w.create_topic("d", 1, &[]).await;
    let app = api(&w);
    let (_, v) = get(&app, "/api/v1/topics").await;
    let rows = &v["view"]["rows"];
    assert_eq!(
        (&row(rows, "c")["cleanup"], &row(rows, "c")["retention_ms"]),
        (&json!("compact"), &json!(3_600_000)),
        "{v}"
    );
    assert_eq!(
        (&row(rows, "d")["cleanup"], &row(rows, "d")["retention_ms"]),
        (&json!("delete"), &Value::Null),
        "{v}"
    );
    let (_, v) = get(&app, "/api/v1/topics/c").await;
    assert_eq!(v["head"]["cleanup"], "compact", "{v}");
    assert_eq!(v["head"]["retention_ms"], 3_600_000, "{v}");
    assert_eq!(v["retention"], "compact · 1 h");

    let (_, v) = get(&app, "/api/v1/topics/c/config").await;
    assert_eq!(v["cleanup"], "compact", "{v}");
    assert_eq!(
        row(&v["rows"], "compression.type")["allowed"],
        "one of uncompressed, zstd, lz4, snappy, gzip, producer"
    );
    assert_eq!(
        row(&v["rows"], "segment.ms")["allowed"],
        "a whole number, at least 1"
    );

    let (_, v) = get(&app, "/api/v1/topics/c/config/edit?key=segment.ms").await;
    let view = &v["view"];
    assert_eq!(view["input"]["value"], "604800000", "{v}");
    assert_eq!(view["pace"]["cleaner_secs"], 15, "{v}");
    assert_eq!(view["pace"]["retention_secs"], 300, "{v}");
    let (_, v) = get(&app, "/api/v1/topics/c/config/edit?key=cleanup.policy").await;
    let view = &v["view"];
    assert_eq!(view["input"]["value"], "", "{v}");
    let here: String = view["teach"]["here"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["text"].as_str().unwrap())
        .collect();
    assert!(
        here.contains("makes retention (1 h) trims this topic too"),
        "{here}"
    );
    w.stop().await;
}

/// What a form misses is said in its own words; a check says each key's value now and after; a save says each key's value
/// before and now.
#[tokio::test]
async fn a_form_says_what_it_misses_a_check_says_after_and_a_save_says_before_and_now() {
    let w = world("api-config-answers-forms", |_, _| {}).await;
    w.create_topic("d", 1, &[]).await;
    let app = api(&w);
    let edit = "/api/v1/topics/d/config/edit";
    for (fields, error) in [
        (
            vec![("key", "retention.ms"), ("op", "set"), ("value", " ")],
            "Type a value.",
        ),
        (
            vec![("key", "cleanup.policy"), ("op", "append"), ("value", "")],
            "Type the policy to use.",
        ),
        (
            vec![
                ("key", "min.compaction.lag.ms"),
                ("value", "100"),
                ("other", ""),
            ],
            "Type both lags.",
        ),
        (
            vec![
                ("key", "min.compaction.lag.ms"),
                ("value", ""),
                ("other", "100"),
            ],
            "Type both lags.",
        ),
    ] {
        let (_, v) = post(&app, edit, &fields).await;
        assert_eq!(v["outcome"], "invalid", "{fields:?}: {v}");
        assert_eq!(v["detail"]["view"]["error"], error, "{fields:?}: {v}");
    }
    let (_, v) = post(
        &app,
        edit,
        &[
            ("key", "cleanup.policy"),
            ("op", "append"),
            ("value", "compact"),
            ("check", "1"),
        ],
    )
    .await;
    assert_eq!(v["kind"], "checked", "{v}");
    let checked = v["form"]["view"]["checked"]["after"].clone();
    assert_eq!(
        checked,
        json!([["cleanup.policy", "delete", "delete,compact"]]),
        "{v}"
    );
    assert_eq!(
        w.config("d", "cleanup.policy").await.as_deref(),
        Some("delete")
    );
    let (_, v) = post(
        &app,
        edit,
        &[
            ("key", "cleanup.policy"),
            ("op", "append"),
            ("value", "compact"),
        ],
    )
    .await;
    assert_eq!(v["outcome"], "done", "{v}");
    assert_eq!(v["answer"]["saved"]["head"]["cleanup"], "both", "{v}");
    let change = &v["answer"]["saved"]["changes"][0];
    assert_eq!(change["name"], "cleanup.policy", "{v}");
    assert_eq!(change["before"]["value"], "delete", "{v}");
    assert_eq!(change["now"]["value"], "delete,compact", "{v}");
    assert_eq!(change["now"]["source"], "dynamic_topic", "{v}");
    w.stop().await;
}

/// A refused value offers what the broker takes in its place: for a list, the elements the refusal names that change it
/// (with the list after), not the one already there; for a single value, the value in place (no list after); for the two
/// lags, the two swapped. The other samples Kafka refuses are said in its words, leaving out the one just sent. A lag
/// refused for its value (not the two together) offers nothing.
#[tokio::test]
async fn a_refusal_offers_what_the_broker_takes_and_says_the_other_samples() {
    let w = world("api-config-answers-refused", |_, _| {}).await;
    w.create_topic("d", 1, &[]).await;
    let app = api(&w);
    let edit = "/api/v1/topics/d/config/edit";
    let all = [
        "segment.bytes 512",
        "min.cleanable.dirty.ratio 1.5",
        "segment.bytes abc",
    ];

    let (_, v) = post(
        &app,
        edit,
        &[
            ("key", "cleanup.policy"),
            ("op", "append"),
            ("value", "bogus"),
        ],
    )
    .await;
    assert_eq!(v["outcome"], "refused", "{v}");
    assert_eq!(v["detail"]["refused"]["kind"], "value");
    assert_eq!(v["detail"]["refused"]["head"]["cleanup"], "delete");
    assert_eq!(
        pass_of(&v),
        vec![(
            json!([["cleanup.policy", "compact"]]),
            json!("delete,compact")
        )],
        "{v}"
    );
    assert_eq!(others_of(&v), all, "{v}");

    let (_, v) = post(
        &app,
        edit,
        &[("key", "min.cleanable.dirty.ratio"), ("value", "7")],
    )
    .await;
    assert_eq!(
        pass_of(&v),
        vec![(json!([["min.cleanable.dirty.ratio", "1"]]), Value::Null)],
        "{v}"
    );
    assert_eq!(others_of(&v), all, "{v}");

    // A sample's own value is not said again; the other value of the same key is.
    let (_, v) = post(&app, edit, &[("key", "segment.bytes"), ("value", "512")]).await;
    assert_eq!(v["outcome"], "refused", "{v}");
    assert_eq!(
        others_of(&v),
        ["min.cleanable.dirty.ratio 1.5", "segment.bytes abc"],
        "{v}"
    );

    let (_, v) = post(
        &app,
        edit,
        &[
            ("key", "min.compaction.lag.ms"),
            ("value", "50000"),
            ("other", "10000"),
        ],
    )
    .await;
    assert_eq!(v["detail"]["refused"]["kind"], "lags", "{v}");
    assert_eq!(
        pass_of(&v),
        vec![(
            json!([
                ["min.compaction.lag.ms", "10000"],
                ["max.compaction.lag.ms", "50000"]
            ]),
            Value::Null
        )],
        "{v}"
    );

    let (_, v) = post(
        &app,
        edit,
        &[
            ("key", "min.compaction.lag.ms"),
            ("value", "-5"),
            ("other", "100"),
        ],
    )
    .await;
    assert_eq!(v["outcome"], "refused", "{v}");
    assert_eq!(v["detail"]["refused"]["kind"], "value", "{v}");
    assert!(pass_of(&v).is_empty(), "{v}");
    w.stop().await;
}
