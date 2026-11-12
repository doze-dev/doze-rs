//! Nothing from the broker or the request reaches a page unescaped: hostile strings in every field of every view-model,
//! through every page and the shell, and in the places a request writes to a page.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_console::assets::AssetUrls;
use kafka_console::base::Base;
use kafka_console::fixtures;
use kafka_console::model::{
    Access, Attention, ClientSnippet, ConnectView, HomeState, HomeView, ProbeView, Rule, Span, Tok,
};
use kafka_console::pages;
use kafka_console::ui::shell::{Chrome, Crumb, Page, Rail, document};
use support::{app, get, send};

const HOSTILE: [&str; 6] = [
    "<script>alert(1)</script>",
    "\"><img src=x onerror=alert(1)>",
    "'><svg/onload=alert(1)>",
    "</pre><script>x</script>",
    "javascript:alert(1)",
    "&lt;already&gt; & \" ' <b>x</b>",
];

fn access(s: &str) -> Access {
    Access {
        label: s.to_owned(),
        sasl: true,
        detail: s.to_owned(),
    }
}

fn probe(s: &str) -> ProbeView {
    let rule = |ok| Rule {
        ok,
        title: s.to_owned(),
        detail: vec![Span::text(s), Span::mono(s), Span::strong(s)],
        value: s.to_owned(),
    };
    ProbeView {
        verdict: kafka_console::model::Verdict::NotReachable,
        rules: vec![rule(true), rule(false)],
        listen: s.to_owned(),
        advertised: s.to_owned(),
        same: false,
        metadata_version: 12,
    }
}

fn home(s: &str, state: HomeState) -> HomeView {
    let mut v = fixtures::home(state);
    v.listen = s.to_owned();
    v.probe = probe(s);
    v.instance.started = s.to_owned();
    v.instance.profile = s.to_owned();
    v.instance.data_dir = s.to_owned();
    v.instance.access = access(s);
    v
}

fn attention(s: &str) -> Attention {
    let mut a = fixtures::attention();
    a.transactional_id = s.to_owned();
    a.partition = s.to_owned();
    a.topic = s.to_owned();
    a.since = s.to_owned();
    a
}

fn connect(s: &str) -> ConnectView {
    ConnectView {
        listen: s.to_owned(),
        loopback: false,
        probe: probe(s),
        access: access(s),
        snippets: vec![ClientSnippet {
            name: "x",
            tag: "y",
            code: vec![vec![
                (Tok::Keyword, s.to_owned()),
                (Tok::Str, s.to_owned()),
                (Tok::Comment, s.to_owned()),
                (Tok::Plain, s.to_owned()),
            ]],
        }],
    }
}

fn shell(s: &str, main: maud::Markup, base: &Base) -> String {
    let urls = AssetUrls {
        css: base.url("/static/c.css"),
        script: base.url("/static/c.js"),
        favicon: base.url("/static/f.svg"),
    };
    let chrome = Chrome {
        address: s.to_owned(),
        profile: s.to_owned(),
        security: s.to_owned(),
        locked: true,
        unprotected: true,
    };
    let page = Page {
        base,
        assets: &urls,
        chrome: &chrome,
        active: Rail::Home,
        crumbs: vec![
            Crumb {
                label: s.to_owned(),
                href: None,
            },
            Crumb {
                label: s.to_owned(),
                href: Some(base.url("/")),
            },
        ],
        title: s,
        watch: &["home-instance"],
    };
    document(&page, main).into_string()
}

/// What an unescaped payload would leave behind in a document.
fn assert_inert(doc: &str, payload: &str, what: &str) {
    for live in [
        "<script>alert",
        "<img src=x",
        "<svg/onload",
        "</pre><script>",
        "<script>x",
        "<b>x</b>",
    ] {
        assert!(
            !doc.contains(live),
            "{what}: {live:?} came through for {payload:?}"
        );
    }
    // The only script is the console's own file.
    assert_eq!(
        doc.matches("<script").count(),
        1,
        "{what}: a script tag that is not the console's, for {payload:?}"
    );
    assert!(
        !doc.contains("<img"),
        "{what}: an img tag came through for {payload:?}"
    );
    assert!(
        !doc.contains("href=\"javascript:"),
        "{what}: a javascript: link for {payload:?}"
    );
}

#[test]
fn hostile_text_in_every_field_of_every_page_comes_out_inert() {
    let base = Base::root();
    for payload in HOSTILE {
        for state in [
            HomeState::Steady,
            HomeState::Quiet,
            HomeState::Attention(attention(payload)),
        ] {
            let page = shell(payload, pages::home(&home(payload, state), &base), &base);
            assert_inert(&page, payload, "home");
        }
        assert_inert(
            &shell(payload, pages::connect(&connect(payload), &base), &base),
            payload,
            "connect",
        );
        assert_inert(
            &shell(payload, pages::advertised(&probe(payload), &base), &base),
            payload,
            "advertised",
        );
        assert_inert(
            &shell(payload, pages::planned(payload, &base), &base),
            payload,
            "planned",
        );
        assert_inert(
            &shell(payload, pages::not_found(payload, &base), &base),
            payload,
            "not found",
        );
        // The fragments are fetched alone and swapped in: they must be as clean as the page.
        let h = home(payload, HomeState::Attention(attention(payload)));
        for frag in [
            pages::sentence_fragment(&h, &base),
            pages::callout_fragment(&h, &base),
            pages::instance_fragment(&h, &base),
            pages::probe_home(&h.probe, &base),
            pages::probe_connect(&h.probe, &base),
            pages::probe_advertised(&h.probe, &base),
        ] {
            let doc = format!("<script src=a></script>{}", frag.into_string());
            assert_inert(&doc, payload, "fragment");
        }
    }
}

#[test]
fn a_payload_that_is_shown_is_shown_as_text() {
    let base = Base::root();
    let doc = shell(
        "<i>",
        pages::connect(&connect("<script>alert(1)</script>"), &base),
        &base,
    );
    assert!(doc.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        doc.contains("&lt;i&gt;"),
        "the title and the crumbs show the text"
    );
    // The quote that would end an attribute is not a quote in it.
    let doc = shell(
        "x",
        pages::connect(&connect("\"><img src=x onerror=alert(1)>"), &base),
        &base,
    );
    assert!(doc.contains("&quot;&gt;&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!doc.contains("\"><img"));
}

#[test]
fn a_topic_name_in_a_link_is_encoded_and_a_prefix_is_always_a_path() {
    let base = Base::root();
    let mut a = attention("x");
    a.topic = "a/b?c=d\"e<f>".to_owned();
    let page = pages::home(&home("x", HomeState::Attention(a)), &base).into_string();
    assert!(
        page.contains("href=\"/topics/a%2Fb%3Fc%3Dd%22e%3Cf%3E/producers\""),
        "{page}"
    );
    for prefix in [
        "javascript:alert(1)",
        "//evil.example",
        "http://evil.example",
        "\"><script>",
        "..",
        "/../x",
    ] {
        let url = Base::new(prefix).url("/connect");
        assert!(
            url.starts_with('/') && !url.starts_with("//"),
            "{prefix}: {url}"
        );
    }
}

#[tokio::test]
async fn a_path_that_is_not_a_page_is_shown_escaped_and_with_the_security_headers() {
    let (app, _) = app("/", &[]);
    for path in [
        "/%3Cscript%3Ealert(1)%3C/script%3E",
        "/'onerror='alert(1)",
        "/%22%3E%3Cimg%20src=x%20onerror=alert(1)%3E",
    ] {
        let (res, body) = send(&app, get(path)).await;
        assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);
        assert!(
            !body.contains("<script>alert") && !body.contains("<img src=x"),
            "{path}: {body:.400}"
        );
        assert_eq!(body.matches("<script").count(), 1, "{path}");
    }
}

#[tokio::test]
async fn a_hostile_query_is_never_echoed() {
    let (app, _) = app("/", &[]);
    let (res, body) = send(
        &app,
        get("/?state=%3Cscript%3Ealert(1)%3C/script%3E&x=%3Cimg%3E"),
    )
    .await;
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    assert!(!body.contains("alert(1)") && !body.contains("<img>"));
    let (res, body) = send(&app, get("/events?watch=%3Cscript%3E")).await;
    assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    // The 400 is plain text, and says the id with its quotes (Debug), never as markup.
    assert!(
        res.headers()[axum::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    assert!(
        body.contains("\\\"") || body.contains("\"<script>\""),
        "{body}"
    );
}

// ---- the Topics pages (slice 9b, reading half) ------------------------------------------------------------------------

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
    all.push(format!("<img src=x onerror=alert(1)>{}", "A".repeat(200_000)));
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
        HeaderRow { key: s.to_owned(), value: value(s) },
        HeaderRow { key: "x".to_owned(), value: shown(Some(&[0xff, 0xfe, 0x00]), 3) },
    ];
    r.txn = Some(RecordTxn { id: Some(s.to_owned()), state: Some(s.to_owned()), since_ms: Some(0), timeout_ms: Some(1) });
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
            MessageQuery { partition: Some(0), offset: Some(16_098), ..MessageQuery::default() },
            MessageQuery { reading: Reading::Committed, ..MessageQuery::default() },
            MessageQuery { key: Some(p.chars().take(100).collect()), ..MessageQuery::default() },
        ];
        for q in queries {
            let v = hostile_messages(p, q);
            let main = topic_pages::messages_page(&v, &base, p, &v.query.to_query());
            assert_inert(&shell(p, main, &base), p, "messages");
            assert_inert(&shell(p, topic_pages::partitions_page(&v.head, &base), &base), p, "partitions");
            let frag = format!("<script src=a></script>{}", topic_pages::messages_fragment(&v, &base, "").into_string());
            assert_inert(&frag, p, "messages fragment");
            let frag = format!("<script src=a></script>{}", topic_pages::topic_sentence_fragment(&v.head, &base).into_string());
            assert_inert(&frag, p, "topic sentence");
        }
        let offline = hostile_messages(p, MessageQuery::default());
        assert!(!offline.head.offline().is_empty());
        assert_inert(&shell(p, topic_pages::messages_page(&offline, &base, p, ""), &base), p, "offline");
        let mut empty = world::messages("orders.dlq", MessageQuery::default(), vec![p.to_owned()]).unwrap();
        empty.head.name = p.to_owned();
        assert_inert(&shell(p, topic_pages::messages_page(&empty, &base, p, ""), &base), p, "empty topic");
        assert_inert(&shell(p, topic_pages::record_page(&hostile_record(p), &base), &base), p, "record");
        assert_inert(&shell(p, topic_pages::no_record(p, 1, 2, &base), &base), p, "no record");
        assert_inert(&shell(p, topic_pages::unknown(p, &base), &base), p, "unknown topic");
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
        assert_inert(&shell(&payload, maud::PreEscaped(page), &base), &payload, "list");
        let mut none = world::topics("", false, Some("none"));
        none.filter.clone_from(&payload);
        assert_inert(&shell(&payload, topic_pages::list_page(&none, &base, &payload, ""), &base), &payload, "none");
        let mut filtered = world::topics("zzz", false, None);
        filtered.filter.clone_from(&payload);
        assert_inert(&shell(&payload, topic_pages::list_page(&filtered, &base, &payload, ""), &base), &payload, "filtered");
        let frag = format!("<script src=a></script>{}{}", topic_pages::list_fragment(&v, &base, "").into_string(), topic_pages::sentence_fragment(&v, &base, "").into_string());
        assert_inert(&frag, &payload, "list fragments");
    }
    // A row that is only a name: the link to it is encoded.
    let mut v = world::topics("", false, None);
    v.rows[0] = TopicRow { name: "a/b?c=d\"e<f>".to_owned(), ..v.rows[0].clone() };
    let page = topic_pages::list_page(&v, &base, "x", "").into_string();
    assert!(page.contains("href=\"/topics/a%2Fb%3Fc%3Dd%22e%3Cf%3E\""), "{page:.3000}");
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
            assert_eq!(body.matches("<script").count(), 1, "{path}");
        }
    }
}

#[test]
fn the_stream_vocabulary_is_closed_and_has_the_topics_ids() {
    use kafka_console::stream::{WATCHABLE, parse_watch};
    for id in ["topics-sentence", "topics-table", "topic-sentence", "topic-messages"] {
        assert!(WATCHABLE.contains(&id), "{id}");
        assert_eq!(parse_watch(id), Ok(vec![id]));
    }
    for bad in ["topic-evil", "topics-table,evil", "Topic-Messages", "topic-messages ", "../topic-messages"] {
        assert!(parse_watch(bad).is_err(), "{bad:?}");
    }
}
