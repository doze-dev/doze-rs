//! Hostile keys, values, headers, names and queries through every page of the Topics section (slice 9b, reading half).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use super::{HOSTILE, app, assert_inert, get, send, shell};
use kafka_console::base::Base;

use kafka_console::fixtures_topics as world;
use kafka_console::gather::decode::shown;
use kafka_console::model::{
    HeaderRow, MessageQuery, Reading, RecordTxn, RecordView, Shown, TopicRow,
};
use kafka_console::pages::topics as topic_pages;

/// Strings a record, a header or a name may carry: markup, attribute break-outs, bidirectional controls, a NUL, a very long
/// line, an entity.
fn nasty() -> Vec<String> {
    let mut all: Vec<String> = HOSTILE.iter().map(|s| (*s).to_owned()).collect();
    all.push("a\u{202e}evil\u{2066}b\u{0}c\u{7f}d\u{85}e\u{feff}f\u{2028}g".to_owned());
    all.push(format!(
        "<img src=x onerror=alert(1)>{}",
        "A".repeat(200_000)
    ));
    all.push("\"><script>alert(1)</script>\n'<svg/onload=alert(1)>".to_owned());
    all
}

/// A text as the console shows a value that held it: through the decoder.
fn value(s: &str) -> Shown {
    shown(Some(s.as_bytes()), s.len() as u64)
}

fn hostile_messages(s: &str, query: MessageQuery) -> kafka_console::model::MessagesView {
    let mut v = world::messages("orders", query, vec![s.to_owned()]).unwrap();
    v.head.name = s.to_owned();
    for r in &mut v.rows {
        r.key = value(s);
        r.value = value(s);
    }
    for p in &mut v.head.parts {
        p.offline = Some(s.to_owned());
    }
    for t in &mut v.head.txns {
        t.id = s.to_owned();
    }
    for h in &mut v.holds {
        h.txn = Some((s.to_owned(), 5));
    }
    v
}

fn hostile_record(s: &str) -> RecordView {
    let mut r = world::record("orders", 1, 16_050).unwrap();
    r.topic = s.to_owned();
    r.key = value(s);
    r.value = value(s);
    r.headers = vec![
        HeaderRow {
            key: s.to_owned(),
            value: value(s),
        },
        HeaderRow {
            key: "x".to_owned(),
            value: shown(Some(&[0xff, 0xfe, 0x00]), 3),
        },
    ];
    r.txn = Some(RecordTxn {
        id: Some(s.to_owned()),
        state: Some(s.to_owned()),
        since_ms: Some(0),
        timeout_ms: Some(1),
    });
    r.codec = s.to_owned();
    r
}

#[test]
fn hostile_keys_values_headers_and_names_are_inert_on_every_topic_page() {
    let base = Base::root();
    for payload in nasty() {
        let p = payload.as_str();
        let queries = [
            MessageQuery::default(),
            MessageQuery {
                partition: Some(0),
                offset: Some(16_098),
                ..MessageQuery::default()
            },
            MessageQuery {
                reading: Reading::Committed,
                ..MessageQuery::default()
            },
            MessageQuery {
                key: Some(p.chars().take(100).collect()),
                ..MessageQuery::default()
            },
        ];
        for q in queries {
            let v = hostile_messages(p, q);
            let main = topic_pages::messages_page(&v, &base, p, &v.query.to_query());
            assert_inert(&shell(p, main, &base), p, "messages");
            assert_inert(
                &shell(p, topic_pages::partitions_page(&v.head, &base), &base),
                p,
                "partitions",
            );
            let frag = format!(
                "<script src=a></script>{}",
                topic_pages::messages_fragment(&v, &base, "").into_string()
            );
            assert_inert(&frag, p, "messages fragment");
            let frag = format!(
                "<script src=a></script>{}",
                topic_pages::topic_sentence_fragment(&v.head, &base).into_string()
            );
            assert_inert(&frag, p, "topic sentence");
        }
    }
}

#[test]
fn hostile_names_and_values_are_inert_on_the_offline_empty_record_and_unknown_pages() {
    let base = Base::root();
    for payload in nasty() {
        let p = payload.as_str();
        let offline = hostile_messages(p, MessageQuery::default());
        assert!(!offline.head.offline().is_empty());
        assert_inert(
            &shell(p, topic_pages::messages_page(&offline, &base, p, ""), &base),
            p,
            "offline",
        );
        let mut empty =
            world::messages("orders.dlq", MessageQuery::default(), vec![p.to_owned()]).unwrap();
        empty.head.name = p.to_owned();
        assert_inert(
            &shell(p, topic_pages::messages_page(&empty, &base, p, ""), &base),
            p,
            "empty topic",
        );
        assert_inert(
            &shell(
                p,
                topic_pages::record_drawer(&hostile_record(p), &base, ""),
                &base,
            ),
            p,
            "record",
        );
        assert_inert(
            &shell(p, topic_pages::no_record(p, 1, 2, &base), &base),
            p,
            "no record",
        );
        assert_inert(
            &shell(p, topic_pages::unknown(p, &base), &base),
            p,
            "unknown topic",
        );
    }
}

#[test]
fn hostile_topic_names_in_the_list_come_out_inert() {
    let base = Base::root();
    for payload in nasty() {
        let mut v = world::topics("", true, None);
        for r in &mut v.rows {
            r.name.clone_from(&payload);
            r.read_by = vec![payload.clone()];
        }
        v.filter.clone_from(&payload);
        v.newest = Some((payload.clone(), 0));
        let page = topic_pages::list_page(&v, &base, &payload, "").into_string();
        assert_inert(
            &shell(&payload, maud::PreEscaped(page), &base),
            &payload,
            "list",
        );
        let mut none = world::topics("", false, Some("none"));
        none.filter.clone_from(&payload);
        assert_inert(
            &shell(
                &payload,
                topic_pages::list_page(&none, &base, &payload, ""),
                &base,
            ),
            &payload,
            "none",
        );
        let mut filtered = world::topics("zzz", false, None);
        filtered.filter.clone_from(&payload);
        assert_inert(
            &shell(
                &payload,
                topic_pages::list_page(&filtered, &base, &payload, ""),
                &base,
            ),
            &payload,
            "filtered",
        );
        let frag = format!(
            "<script src=a></script>{}{}",
            topic_pages::list_fragment(&v, &base, "").into_string(),
            topic_pages::sentence_fragment(&v, &base, "").into_string()
        );
        assert_inert(&frag, &payload, "list fragments");
    }
    // A row that is only a name: the link to it is encoded.
    let mut v = world::topics("", false, None);
    v.rows[0] = TopicRow {
        name: "a/b?c=d\"e<f>".to_owned(),
        ..v.rows[0].clone()
    };
    let page = topic_pages::list_page(&v, &base, "x", "").into_string();
    assert!(
        page.contains("href=\"/topics/a%2Fb%3Fc%3Dd%22e%3Cf%3E\""),
        "{page:.3000}"
    );
}

#[test]
fn what_could_act_on_a_page_is_written_out_in_a_value_and_the_size_stays_true() {
    let s = "x\u{202e}y\u{0}z";
    let v = value(s);
    assert!(!v.text.contains('\u{202e}') && !v.text.contains('\u{0}'));
    assert!(v.text.contains("⟨U+202E⟩") && v.text.contains("⟨U+0000⟩"));
    assert_eq!(v.bytes, s.len() as u64);
}

#[tokio::test]
async fn a_hostile_query_on_a_topic_page_is_a_sentence_never_markup_never_a_5xx() {
    let (app, _) = app("/", &[]);
    let evil = "%3Cscript%3Ealert(1)%3C/script%3E%22%3E%3Cimg%20src=x%20onerror=alert(1)%3E";
    for path in [
        format!("/topics?q={evil}"),
        format!("/topics?q={evil}&internal={evil}"),
        format!("/topics/orders?partition={evil}&offset={evil}&time={evil}&key={evil}&isolation={evil}&limit={evil}"),
        format!("/topics/orders/messages?key={evil}"),
        format!("/topics/orders/messages?find={evil}"),
        "/topics/orders/messages?partition=-1&offset=-1&limit=-1".to_owned(),
        "/topics/orders/messages?partition=99999999999999999999&offset=99999999999999999999&limit=99999999999999999999".to_owned(),
        "/topics/orders/messages?offset=NaN&time=NaN&limit=NaN".to_owned(),
        "/topics/orders/messages?limit=1e9&offset=1e9".to_owned(),
        format!("/topics/orders/messages?key={}", "k".repeat(5000)),
        format!("/topics/{evil}"),
        format!("/topics/{evil}/messages"),
        format!("/topics/{evil}/messages/0/0"),
        format!("/topics/{evil}/partitions"),
        "/topics/orders/messages/%3Cb%3E/%3Ci%3E".to_owned(),
        "/topics/orders/messages/0/-1".to_owned(),
        "/topics/orders/messages/0/9223372036854775808".to_owned(),
        "/topics/orders/messages/2147483648/0".to_owned(),
        format!("/topics/{}/partitions", "n".repeat(300)),
        format!("/fragments/topic-messages?topic={evil}"),
        format!("/fragments/topics-table?q={evil}"),
    ] {
        let (res, body) = send(&app, get(&path)).await;
        assert!(
            res.status().is_success() || res.status() == axum::http::StatusCode::NOT_FOUND,
            "{path}: {}",
            res.status()
        );
        assert!(!res.status().is_server_error(), "{path}");
        assert!(!body.contains("<script>alert") && !body.contains("<img src=x"), "{path}: {body:.300}");
        if res.headers().get(axum::http::header::CONTENT_TYPE).is_some_and(|c| c.to_str().unwrap().starts_with("text/html")) && !path.starts_with("/fragments") {
            assert_eq!(body.matches("<script").count(), body.matches("<script src=").count(), "{path}");
        }
    }
}

#[test]
fn the_stream_vocabulary_is_closed_and_has_the_topics_ids() {
    use kafka_console::stream::{WATCHABLE, parse_watch};
    for id in [
        "topics-sentence",
        "topics-table",
        "topic-sentence",
        "topic-messages",
    ] {
        assert!(WATCHABLE.contains(&id), "{id}");
        assert_eq!(parse_watch(id), Ok(vec![id]));
    }
    for bad in [
        "topic-evil",
        "topics-table,evil",
        "Topic-Messages",
        "topic-messages ",
        "../topic-messages",
    ] {
        assert!(parse_watch(bad).is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn the_stream_takes_the_topics_ids_and_refuses_one_that_is_not_in_the_vocabulary() {
    let (app, _) = app("/", &[]);
    let ok = crate::support::head(
        &app,
        get("/events?watch=topics-sentence,topics-table,topic-sentence,topic-messages"),
    )
    .await;
    assert_eq!(ok.status(), axum::http::StatusCode::OK);
    assert!(
        ok.headers()[axum::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    for bad in [
        "topic-evil",
        "topic-messages,evil",
        "TOPIC-MESSAGES",
        "topics-table%0Aevil",
    ] {
        let (res, _) = send(&app, get(&format!("/events?watch={bad}"))).await;
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST, "{bad}");
    }
}
