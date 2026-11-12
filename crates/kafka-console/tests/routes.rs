//! The route table is the router; the guard and the origin proof are in front of every route; every link resolves.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::collections::BTreeSet;

use axum::http::{StatusCode, header};
use kafka_console::routes::{Handler, Method, ROUTES};
use support::{app, get, local_links, post, send};

/// A path that fills the parameters of a pattern with values that are valid for it.
fn concrete(pattern: &str, state: &kafka_console::state::State) -> String {
    let asset = state.assets.urls.css.rsplit('/').next().unwrap().to_owned();
    pattern
        .replace("{id}", "home-live")
        .replace("{name}", &asset)
        .replace("{topic}", "orders")
        .replace("{partition}", "1")
        .replace("{offset}", "16050")
        .replace("{group}", "search-indexer")
        .replace("{member}", "search-indexer-1")
        .replace("{request}", "1")
        .replace("{txn}", "orders-eos")
        .replace("{generation}", "8")
        + if pattern == "/settings/config/edit" {
            // Slice 9h: a broker config is one of five, not a topic's.
            "?key=log.retention.ms"
        } else if pattern == "/access/acls/delete" {
            "?type=topic&name=orders&pattern=literal&principal=User%3Aorders&host=%2A&op=write&perm=allow"
        } else if pattern.ends_with("/config/edit") || pattern.ends_with("/config/reset") {
            "?key=cleanup.policy"
        } else {
            ""
        }
}

/// A body each POST row takes without complaint (the proof of origin is what the guard tests are about).
fn valid_body(pattern: &str) -> &'static str {
    match pattern {
        "/topics/new" => {
            "name=orders.retry&partitions=3&cleanup.policy=delete&retention.ms=1000&mode=create"
        }
        "/topics/{topic}/produce" => "partition=0&key=k&value=v&acks=-1",
        "/topics/{topic}/partitions/add" => "count=5",
        "/topics/{topic}/delete-records" => "partition=0&offset=1",
        "/topics/{topic}/delete" => "confirm=orders",
        "/topics/{topic}/config/edit" => "key=segment.bytes&value=2048",
        "/topics/{topic}/config/reset" => "key=segment.bytes",
        "/settings/config/edit" => "key=log.retention.ms&was=log.retention.ms&value=259200000",
        "/access/acls/new" => "type=topic&name=orders&principal=User%3Aorders&op=write",
        "/access/acls/delete" => {
            "type=topic&name=orders&pattern=literal&principal=User%3Aorders&host=%2A&op=write&perm=allow"
        }
        _ => "back=home",
    }
}

#[tokio::test]
async fn every_row_of_the_table_has_a_handler_and_every_handler_a_row() {
    let (app, state) = app("/", &[]);
    for r in ROUTES {
        let path = concrete(r.path, &state);
        let req = match r.method {
            Method::Get => get(&path),
            Method::Post => post(&path, valid_body(r.path)),
        };
        // The stream never ends: its answer is judged by its head alone.
        let (res, body) = if r.handler == Handler::Events {
            (support::head(&app, req).await, String::new())
        } else {
            send(&app, req).await
        };
        assert!(
            res.status().is_success() || res.status().is_redirection(),
            "{} {} answered {} {body:.200}",
            if r.method == Method::Get {
                "GET"
            } else {
                "POST"
            },
            r.path,
            res.status()
        );
    }
    // And what is not a row is not answered: the router is built from the table and from nothing else.
    for path in [
        "/nope",
        "/topics/orders/nope",
        "/connect/advertised/",
        "/static",
        "/fragments",
        "/events/x",
        "/healthz/x",
        "/api",
    ] {
        let (res, _) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn an_unknown_route_is_a_404_page_in_the_shell_and_a_wrong_method_a_405() {
    let (app, _) = app("/", &[]);
    let (res, body) = send(&app, get("/no/such/page")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert!(
        body.contains("<nav class=\"rail\"") && body.contains("There is no page at"),
        "{body:.300}"
    );
    assert!(body.contains("/no/such/page"));
    let (res, body) = send(&app, post("/connect", "")).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(body.contains("<nav class=\"rail\""), "{body:.200}");
    let (res, _) = send(&app, get("/connect/advertised/check")).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn the_check_form_comes_back_to_its_page_without_a_script_and_swaps_a_fragment_with_one() {
    let (app, _) = app("/k", &[]);
    for (back, to) in [("home", "/k/"), ("advertised", "/k/connect/advertised")] {
        let (res, _) = send(
            &app,
            post("/k/connect/advertised/check", &format!("back={back}")),
        )
        .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(res.headers()[header::LOCATION], to);
    }
    let mut req = post("/k/connect/advertised/check", "back=home");
    req.headers_mut().insert("x-fragment", "1".parse().unwrap());
    let (res, body) = send(&app, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        body.starts_with("<div class=\"frag\" id=\"probe-home\""),
        "{body:.200}"
    );
    let (res, _) = send(&app, post("/k/connect/advertised/check", "back=elsewhere")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let (res, _) = send(&app, post("/k/connect/advertised/check", "")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn every_href_and_src_of_every_page_resolves_under_the_base() {
    for prefix in ["/", "/kafka"] {
        let (app, state) = app(prefix, &[]);
        let mut seen = BTreeSet::new();
        let mut queue: Vec<String> = [
            "/",
            "/?state=quiet",
            "/?state=attention",
            "/connect",
            "/connect/advertised",
        ]
        .iter()
        .map(|p| state.base.url(p))
        .collect();
        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            let (res, body) = send(&app, get(&path)).await;
            assert_eq!(res.status(), StatusCode::OK, "{path}");
            if res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
            {
                for link in local_links(&body) {
                    let want = state.base.url("/");
                    assert!(
                        link.starts_with(&want) || link == state.base.prefix(),
                        "{path} links to {link}, which is outside the base {want}"
                    );
                    queue.push(link);
                }
            }
        }
        assert!(
            seen.len() >= 12,
            "{prefix}: only {} urls crawled: {seen:?}",
            seen.len()
        );
        assert!(
            seen.iter()
                .any(|u| u.contains("/static/console.") && u.ends_with(".css"))
        );
        assert!(
            seen.iter()
                .any(|u| u.contains("/static/console.") && u.ends_with(".js"))
        );
        assert!(seen.iter().any(|u| u.contains("/static/favicon.")));
    }
}

#[tokio::test]
async fn pages_are_not_stored_fragments_revalidate_and_files_are_kept_for_a_year() {
    let (app, state) = app("/", &[]);
    assert_eq!(
        send(&app, get("/")).await.0.headers()[header::CACHE_CONTROL],
        "no-store"
    );
    let (res, body) = send(&app, get("/fragments/home-live")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
    let etag = res.headers()[header::ETAG].to_str().unwrap().to_owned();
    assert!(body.contains("This instance"));
    let mut again = get("/fragments/home-live");
    again
        .headers_mut()
        .insert(header::IF_NONE_MATCH, etag.parse().unwrap());
    let (res, body) = send(&app, again).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    let mut stale = get("/fragments/home-live");
    stale
        .headers_mut()
        .insert(header::IF_NONE_MATCH, "\"other\"".parse().unwrap());
    assert_eq!(send(&app, stale).await.0.status(), StatusCode::OK);
    let (res, _) = send(&app, get(&state.assets.urls.css)).await;
    assert_eq!(
        res.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
    let (res, _) = send(&app, get("/static/console.css")).await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "an unhashed name is not a file"
    );
}

#[tokio::test]
async fn the_stream_takes_only_the_closed_vocabulary_and_an_unknown_fragment_is_a_404() {
    let (app, _) = app("/", &[]);
    let (res, body) = send(&app, get("/events?watch=home-live,evil")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(
        body.contains("\"evil\" is not a fragment this console can watch"),
        "{body}"
    );
    let (res, _) = send(&app, get("/fragments/etc-passwd")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, body) = send(&app, get("/healthz")).await;
    assert_eq!((res.status(), body.as_str()), (StatusCode::OK, "ok\n"));
}
