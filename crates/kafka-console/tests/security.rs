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
        scripts: vec![base.url("/static/c.js")],
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
        regions: &["home-live"],
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
    // The only scripts are the console's own files (each a `<script src>`: there is no inline script).
    assert_eq!(
        doc.matches("<script").count(),
        doc.matches("<script src=").count(),
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
            &shell(payload, pages::unavailable(payload, "/", payload), &base),
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
            pages::live_fragment(&h, &base),
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
        page.contains("href=\"/topics/a%2Fb%3Fc%3Dd%22e%3Cf%3E/transactions/x\""),
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
        assert_eq!(
            body.matches("<script").count(),
            body.matches("<script src=").count(),
            "{path}"
        );
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
    let (res, body) = send(&app, get("/events?tail=%3Cscript%3E")).await;
    assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    // The 400 is plain text and does not echo what was sent.
    assert!(
        res.headers()[axum::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    assert!(!body.contains("<script>"), "{body}");
}

/// Every form of the topic writes, fed what a hostile page or person would put in it: nothing panics, nothing is a 5xx, and
/// whatever is echoed is text.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn hostile_input_in_every_topic_form_is_answered_and_echoed_as_text() {
    let (app, _) = app("/", &[]);
    let long_name = "a".repeat(249);
    let huge = "A".repeat(900 * 1024);
    let evil = [
        "<script>alert(1)</script>".to_owned(),
        "\"><img src=x onerror=alert(1)>".to_owned(),
        "a\r\nSet-Cookie: x=1".to_owned(),
        "k\nX-Injected: yes".to_owned(),
        "nul\0byte".to_owned(),
        long_name.clone(),
        "a".repeat(250),
        huge,
        "javascript:alert(1)".to_owned(),
    ];
    let enc = |s: &str| -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    };
    for value in &evil {
        let v = enc(value);
        let bodies = [
            (
                "/topics/new",
                format!(
                    "name={v}&partitions={v}&template={v}&cleanup.policy={v}&retention.ms={v}&mode=create"
                ),
            ),
            (
                "/topics/new",
                format!("name={v}&name=orders&partitions=3&mode=check"),
            ),
            (
                "/topics/orders/produce",
                format!("partition={v}&key={v}&value={v}&hn0={v}&hv0={v}&hn0=dup&acks={v}"),
            ),
            (
                "/topics/inventory.levels/produce",
                format!("key=&value={v}&hn0={v}&hv0={v}&acks=-1"),
            ),
            ("/topics/orders/partitions/add", format!("count={v}")),
            (
                "/topics/orders/delete-records",
                format!("partition={v}&offset={v}"),
            ),
            ("/topics/orders/delete", format!("confirm={v}")),
            (
                "/topics/orders/delete",
                "confirm=%ff%fe%e2%9c&confirm=orders".to_owned(),
            ),
        ];
        for (path, body) in bodies {
            let (res, doc) = send(&app, support::post(path, &body)).await;
            assert!(res.status().as_u16() < 500, "{path}: {}", res.status());
            if res.status().is_success() {
                assert_inert(&doc, value, path);
                // Nothing in a header line of the answer: a newline in a field cannot reach one.
                assert!(
                    !res.headers().iter().any(|(_, v)| v
                        .to_str()
                        .is_ok_and(|s| s.contains("X-Injected") || s.contains("Set-Cookie"))),
                    "{path}"
                );
            }
        }
        for query in [
            format!(
                "/topics/new?step=review&name={v}&partitions={v}&template={v}&cleanup.policy={v}&retention.ms={v}"
            ),
            format!(
                "/topics/orders/produce?partition={v}&key={v}&value={v}&hn0={v}&hv0={v}&acks={v}"
            ),
            format!("/topics/orders/partitions/add?count={v}"),
            format!("/topics/orders/delete-records?partition={v}&offset={v}"),
        ] {
            if query.len() > 8000 {
                continue;
            }
            let (res, doc) = send(&app, get(&query)).await;
            assert!(res.status().as_u16() < 500, "{query:.80}: {}", res.status());
            if res.status().is_success() {
                assert_inert(&doc, value, "a dialog or step from a query");
            }
        }
    }
    // A name that is shown is shown as text, in the call, in the terminal command and in the refusal.
    let (_, doc) = send(
        &app,
        support::post(
            "/topics/new",
            "name=%3Cscript%3Ealert(1)%3C%2Fscript%3E&partitions=3&mode=create",
        ),
    )
    .await;
    assert!(
        doc.contains("&lt;script&gt;alert(1)&lt;/script&gt;")
            && doc.contains("Refused · INVALID_TOPIC_EXCEPTION (17)"),
        "{doc:.400}"
    );
    // The 249-byte name is a valid one: the broker takes it, and it is the one thing no sentence refuses.
    let (_, doc) = send(
        &app,
        support::post(
            "/topics/new",
            &format!("name={long_name}&partitions=1&mode=check"),
        ),
    )
    .await;
    assert!(doc.contains("<b>Checked.</b>"), "{doc:.300}");
}

/// A request header or a refused body never becomes a header of the answer, and a body of exactly the limit is read.
#[tokio::test]
async fn a_field_of_the_size_of_the_body_limit_is_read_and_a_larger_body_is_refused() {
    let (app, _) = app("/", &[]);
    let near = format!(
        "name={}&partitions=1&mode=check",
        "a".repeat((1 << 20) - 64)
    );
    let (res, _) = send(&app, support::post("/topics/new", &near)).await;
    assert!(res.status().as_u16() < 500);
    let over = format!("value={}", "a".repeat((1 << 20) + 1));
    let (res, _) = send(&app, support::post("/topics/orders/produce", &over)).await;
    assert_eq!(res.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
}
// The Topics pages (slice 9b, reading half) are in `security/topics.rs`: they share the helpers above.
#[path = "security/topics.rs"]
mod topics;
