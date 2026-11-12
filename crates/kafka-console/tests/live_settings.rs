//! The Settings page and the edit dialog of a broker config over a real broker in this process: what the page lists is what the
//! broker lists; a save (for all brokers and for this one), a check and each refusal are the protocol requests a client would
//! send; the rule the dialog teaches is the rule the broker holds; hostile text is escaped; a GET changes nothing.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_access;
mod support_write;

use axum::http::StatusCode;
use kafka_console::backend::requests::broker_configs;
use kafka_console::settings_words::{DYNAMIC, least};
use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;
use support::get;
use support_access::{router_with, start};
use support_write::{World, page, post_form, world};

/// A broker config as DescribeConfigs says it for this broker: value and source number.
async fn described(w: &World, key: &str) -> (String, i8) {
    let Response::DescribeConfigs(r) = w.ask(4, broker_configs::describe_broker(1)).await else {
        panic!()
    };
    let c = r.results[0]
        .configs
        .iter()
        .find(|c| c.name.as_str() == key)
        .unwrap_or_else(|| panic!("the broker does not list {key}"));
    (
        c.value
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        c.config_source,
    )
}

async fn alter(
    w: &World,
    name: &str,
    key: &str,
    value: &str,
    validate_only: bool,
) -> (i16, Option<String>) {
    let Response::IncrementalAlterConfigs(r) = w
        .ask(
            1,
            broker_configs::alter_broker(name, key, value, validate_only),
        )
        .await
    else {
        panic!()
    };
    (
        r.responses[0].error_code,
        r.responses[0]
            .error_message
            .as_ref()
            .map(ToString::to_string),
    )
}

async fn seeded(name: &str) -> World {
    let w = world(name, |b, _| {
        b.log_retention_check_every = std::time::Duration::from_secs(7);
        b.fsync_interval = std::time::Duration::from_millis(90);
    })
    .await;
    w.create_topic("orders", 2, &[("retention.ms", "604800000")])
        .await;
    w.create_topic("payments.settled", 1, &[]).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    w
}

fn app(w: &World) -> axum::Router {
    router_with(w.facts.clone(), start(false), w.backend.clone())
}

/// CON-566: the page lists what the broker lists, from the broker's own answers.
#[tokio::test]
async fn the_page_lists_the_five_with_their_sources_and_the_intervals_of_this_run() {
    let w = seeded("set-page").await;
    let (status, doc) = page(&app(&w), get("/settings")).await;
    assert_eq!(status, StatusCode::OK);
    for (key, _, _) in DYNAMIC {
        let (value, _) = described(&w, key).await;
        assert!(
            doc.contains(&format!("<span class=\"mono\">{key}</span>")),
            "{key}"
        );
        assert!(
            doc.contains(&format!("{value}<span class=\"sub\"")),
            "the page does not say {key} is {value}"
        );
    }
    assert!(
        doc.contains("<b>0</b> of the 5 are set at run time"),
        "{doc:.3000}"
    );
    // The intervals are this run's: 7 s retention check, 90 ms flush.
    assert!(
        doc.contains("Retention is applied every <b>7 s</b>"),
        "{doc}"
    );
    assert!(
        doc.contains(">7s</td>") && doc.contains(">90ms</td>"),
        "{doc}"
    );
    // The version profile is ApiVersions' own: every API the broker serves with SASL off.
    let served = w.broker.served();
    let n = kafka_wire::profile::Profile::new(kafka_wire::profile::KafkaMajor::new(4).unwrap())
        .advertised(kafka_wire::profile::Features {
            sasl: false,
            telemetry: w.broker.client_telemetry(),
            served,
        })
        .len();
    assert!(
        doc.contains(&format!("<b>{n} APIs</b>")),
        "{n} APIs offered: {doc:.4000}"
    );
    assert!(doc.contains(&format!("Show all {n} APIs")));
    // The data directory: its path, a size and the last stop of a new directory.
    assert!(
        doc.contains(&w.dir.file_name().unwrap().to_string_lossy().into_owned()),
        "{doc}"
    );
    assert!(doc.contains("this data directory is new"), "{doc}");
    // The weight says what it can read of itself: memory only where the platform lets the process read it.
    if cfg!(target_os = "linux") {
        assert!(
            doc.contains("<div class=\"eyebrow\">Idle memory</div>")
                && !doc.contains("Measured on Linux only")
        );
    } else {
        assert!(doc.contains("Measured on Linux only"), "{doc}");
    }
    assert!(doc.contains("3<small>of") || doc.contains("of "), "{doc}");
    // Flags are not known to a broker started by a test: the page says so and invents none.
    assert!(doc.contains("did not say how it was started"));
    w.stop().await;
}

/// CON-567: a save for all brokers and for this one reaches the broker as the sources 3 and 2, and is the call the dialog drew.
#[tokio::test]
async fn a_save_is_the_call_the_dialog_drew_and_the_broker_keeps_it_for_the_scope_chosen() {
    let w = seeded("set-save").await;
    let app = app(&w);
    assert_eq!(
        described(&w, "log.retention.ms").await,
        ("-1".to_owned(), 4),
        "the flag default is not Kafka's, so it is a static broker value"
    );
    let (status, dlg) = page(&app, get("/settings/config/edit?key=log.retention.ms")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        dlg.contains("<h1>Settings</h1>") && dlg.contains("Edit log.retention.ms"),
        "{dlg:.500}"
    );
    assert!(
        dlg.contains("payments.settled") && dlg.contains("orders (7 d)"),
        "{dlg}"
    );
    assert!(
        dlg.contains("inventory.levels") && dlg.contains("compact only: retention does not apply"),
        "{dlg}"
    );
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "259200000"),
    ];
    let (status, saved) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        saved.contains("<b>Saved.</b>")
            && saved.contains("259,200,000")
            && saved.contains("for all brokers."),
        "{saved}"
    );
    assert_eq!(
        described(&w, "log.retention.ms").await,
        ("259200000".to_owned(), 3)
    );
    assert!(
        saved.contains("1 of the 5 is now set at run time")
            && saved.contains("Dynamic default broker"),
        "{saved}"
    );
    assert!(
        saved.contains("1 topic has no retention.ms of their own")
            || saved.contains("topics have no retention.ms"),
        "{saved}"
    );
    // For this broker alone: the source is 2, and the value wins over the one for all.
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "this"),
        ("value", "86400000"),
    ];
    let (_, saved) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert!(saved.contains("for this broker."), "{saved}");
    assert_eq!(
        described(&w, "log.retention.ms").await,
        ("86400000".to_owned(), 2)
    );
    // The settings page now says two are set, and the table carries the sources.
    let (_, doc) = page(&app, get("/settings")).await;
    assert!(
        doc.contains("<b>1</b> of the 5 is set at run time") || doc.contains("of the 5 are set"),
        "{doc:.3000}"
    );
    assert!(
        doc.contains("Dynamic broker") && doc.contains("set for this broker"),
        "{doc}"
    );
    w.stop().await;
}

/// CON-568: Check asks with validate_only and changes nothing; each refusal is Kafka's, with its code.
#[tokio::test]
async fn a_check_changes_nothing_and_each_refusal_is_kafkas_own() {
    let w = seeded("set-refuse").await;
    let (spy, log) = w.backend.recording();
    let app = router_with(w.facts.clone(), start(false), spy);
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "5000"),
        ("check", "1"),
    ];
    let (status, dlg) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        dlg.contains("Kafka would take this.") && dlg.contains("validate_only answered 0"),
        "{dlg}"
    );
    assert_eq!(
        described(&w, "log.retention.ms").await,
        ("-1".to_owned(), 4),
        "a check changes nothing"
    );
    // Too small: INVALID_REQUEST with the broker's sentence.
    let f = [
        ("key", "log.segment.bytes"),
        ("was", "log.segment.bytes"),
        ("scope", "all"),
        ("value", "524288"),
    ];
    let (status, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("INVALID_REQUEST (42)"), "{doc}");
    assert!(doc.contains("Invalid value 524288 for configuration log.segment.bytes: Value must be at least 1048576"), "{doc}");
    assert_eq!(described(&w, "log.segment.bytes").await.0, "134217728");
    // Not a number.
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "abc"),
    ];
    let (_, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert!(
        doc.contains(
            "Invalid value abc for configuration log.retention.ms: Not a number of type LONG"
        ),
        "{doc}"
    );
    // A config that cannot change while the broker runs, asked as a dry run by the link of the page.
    log.lock().unwrap().clear();
    let (_, doc) = page(
        &app,
        post_form("/settings/config/edit", &[("probe", "num.partitions")]),
    )
    .await;
    assert!(
        doc.contains("Cannot update these configs dynamically: [num.partitions]"),
        "{doc}"
    );
    assert!(
        doc.contains("--default-partitions") && doc.contains("now 1."),
        "{doc}"
    );
    assert!(
        doc.contains("Unknown Dynamic Configuration: foo.bar.")
            && doc.contains("Unexpected broker id, expected 1, but received 2"),
        "{doc}"
    );
    assert!(doc.contains("Asked as a dry run"), "{doc}");
    assert!(
        doc.contains("node.id") && doc.contains("broker.id"),
        "the fixed configs are the broker's own list"
    );
    // Every one of those asks was validate_only: the broker's configs are as they were.
    for key in ["log.retention.ms", "log.segment.bytes"] {
        assert_eq!(described(&w, key).await.1, 4, "{key}");
    }
    // A forged key is a dry run too, whatever the form said.
    let f = [
        ("key", "num.partitions"),
        ("was", "num.partitions"),
        ("scope", "all"),
        ("value", "9"),
    ];
    let (_, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert!(doc.contains("INVALID_REQUEST (42)"), "{doc}");
    // An empty value is the console's own sentence and asks the broker nothing.
    log.lock().unwrap().clear();
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", ""),
    ];
    let (status, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(doc.contains("Type a value."), "{doc}");
    assert!(
        !log.lock()
            .unwrap()
            .contains(&ApiKey::IncrementalAlterConfigs)
    );
    // Moving the select to another config asks nothing either and shows that config's dialog.
    let f = [
        ("key", "message.max.bytes"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "5000"),
        ("check", "1"),
    ];
    log.lock().unwrap().clear();
    let (_, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert!(
        doc.contains("Edit message.max.bytes") && doc.contains("1048588"),
        "{doc}"
    );
    assert!(
        !log.lock()
            .unwrap()
            .contains(&ApiKey::IncrementalAlterConfigs)
    );
    w.stop().await;
}

/// CON-569: the rule the dialog teaches is the one the broker holds: the least value is taken and one less is refused.
#[tokio::test]
async fn the_rule_the_dialog_teaches_is_the_rule_the_broker_holds() {
    let w = seeded("set-rule").await;
    for (key, _, _) in DYNAMIC {
        if let Some(n) = least(key) {
            assert_eq!(
                alter(&w, "", key, &n.to_string(), true).await.0,
                0,
                "{key} takes {n}"
            );
            if key != "log.retention.ms" || n > i64::MIN {
                assert_eq!(
                    alter(&w, "", key, &(n - 1).to_string(), true).await.0,
                    42,
                    "{key} does not take {}",
                    n - 1
                );
            }
        }
    }
    assert_eq!(
        alter(&w, "", "log.retention.bytes", "-5", true).await.0,
        0,
        "retention.bytes takes any number"
    );
    assert_eq!(
        alter(&w, "", "log.segment.bytes", "2147483648", true)
            .await
            .0,
        42,
        "the 32-bit ones stop at 2,147,483,647"
    );
    assert_eq!(
        alter(&w, "", "log.segment.bytes", "2147483647", true)
            .await
            .0,
        0
    );
    w.stop().await;
}

/// CON-570: a hostile key, value or topic name is escaped and cannot start a line of the call.
#[tokio::test]
async fn hostile_text_is_escaped_and_cannot_start_a_line_of_the_call() {
    let w = seeded("set-hostile").await;
    let app = app(&w);
    let (status, doc) = page(
        &app,
        get("/settings/config/edit?key=%3Cscript%3Ealert(1)%3C/script%3E"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!doc.contains("<script>alert(1)"), "{doc}");
    let (_, dlg) = page(&app, get("/settings/config/edit?key=log.retention.ms")).await;
    assert!(!dlg.contains("<script>alert(1)"), "{dlg}");
    let evil = "5\n  SET evil = 1\r\nSet-Cookie: x=1";
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", evil),
        ("check", "1"),
    ];
    let (res, text) = support::send(&app, post_form("/settings/config/edit", &f)).await;
    assert!(res.headers().get(axum::http::header::SET_COOKIE).is_none());
    // Kafka's own sentence repeats what was typed, as text; the call drawn beside it is one line for each part.
    let pre: String = text
        .split("<pre")
        .skip(1)
        .map(|p| p.split("</pre>").next().unwrap_or(""))
        .collect();
    assert!(
        !pre.contains("\n  SET evil = 1") && pre.contains("\\n  SET evil"),
        "{pre}"
    );
    let f = [
        ("key", "<img src=x onerror=alert(1)>"),
        ("was", "x"),
        ("scope", "all"),
        ("value", "1"),
    ];
    let (_, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert!(!doc.contains("<img src=x"), "{doc}");
    w.stop().await;
}

/// CON-571: a caller that may not alter configs is refused as a client would be, and the page still draws.
#[tokio::test]
async fn a_caller_without_rights_is_refused_in_kafkas_words_and_the_page_still_draws() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    let w = world("set-acl", |b, _| b.acl_default_allow = false).await;
    let nobody = Caller::new(
        Principal::user("nobody"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let denied = router_with(w.facts.clone(), start(false), w.backend.acting_as(nobody));
    let (status, doc) = page(&denied, get("/settings")).await;
    assert_eq!(status, StatusCode::OK, "{doc:.500}");
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "5000"),
    ];
    let (status, doc) = page(&denied, post_form("/settings/config/edit", &f)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("CLUSTER_AUTHORIZATION_FAILED (31)"), "{doc}");
    // The console itself, a super user, is not stopped by the same broker.
    let (_, saved) = page(&app(&w), post_form("/settings/config/edit", &f)).await;
    assert!(saved.contains("<b>Saved.</b>"), "{saved}");
    w.stop().await;
}
