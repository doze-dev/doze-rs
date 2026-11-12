//! Every link of the topic write pages resolves, under any prefix; the dialogs are routes that answer a script their body
//! alone and a browser a full page; and the POST of each write answers the board's state.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::collections::BTreeSet;

use axum::http::{StatusCode, header};
use support::{app, get, local_links, post, send};

/// The pages the parallel read builder makes (`/topics`, a topic, its messages and partitions): until the branches are
/// merged a link to one is a 404, and the allow-list says so (`expires: 9b`).
fn read_builders(path: &str) -> bool {
    let rest = path.trim_start_matches("/kafka");
    let parts: Vec<&str> = rest.trim_matches('/').split('/').collect();
    matches!(parts.as_slice(), ["topics"])
        || matches!(parts.as_slice(), ["topics", t] if *t != "new")
        || matches!(parts.as_slice(), ["topics", t, "messages" | "partitions"] if *t != "new")
        || matches!(parts.as_slice(), ["topics", t, "messages", _, _] if *t != "new")
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn every_link_of_the_write_pages_resolves_or_is_the_read_builders() {
    for prefix in ["/", "/kafka"] {
        let (app, state) = app(prefix, &[]);
        let u = |p: &str| state.base.url(p);
        let mut queue: Vec<String> = [
            "/topics/new",
            "/topics/new?step=config&name=orders.retry&partitions=3&template=short",
            "/topics/new?step=review&name=orders.retry&partitions=3&template=compact&cleanup.policy=compact&retention.ms=-1",
            "/topics/orders/produce",
            "/topics/orders/partitions/add",
            "/topics/orders/delete-records?partition=0",
            "/topics/orders/delete",
        ]
        .iter()
        .map(|p| u(p))
        .collect();
        // The result pages are the answers to the POSTs: their links are followed too.
        for (path, body) in [
            (
                "/topics/new",
                "name=orders.retry&partitions=3&cleanup.policy=delete&retention.ms=1&mode=create",
            ),
            (
                "/topics/new",
                "name=orders&partitions=3&cleanup.policy=delete&retention.ms=1&mode=create",
            ),
            (
                "/topics/orders/produce",
                "partition=2&key=k&value=v&acks=-1",
            ),
            ("/topics/inventory.levels/produce", "key=&value=v&acks=-1"),
            ("/topics/orders/partitions/add", "count=2"),
            (
                "/topics/inventory.levels/delete-records",
                "partition=0&offset=1",
            ),
            ("/topics/orders/delete", "confirm=orders"),
        ] {
            let (res, text) = send(&app, post(&u(path), body)).await;
            assert_eq!(res.status(), StatusCode::OK, "{path} {body}");
            queue.extend(local_links(&text));
        }
        let mut seen = BTreeSet::new();
        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) || read_builders(&path) {
                continue;
            }
            let (res, text) = send(&app, get(&path)).await;
            // The fixture world is stateless: a topic it just "made" is not there to open, and the page says so.
            if res.status() == StatusCode::NOT_FOUND && text.contains("There is no topic called") {
                continue;
            }
            assert_eq!(res.status(), StatusCode::OK, "{path}");
            if res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
            {
                for link in local_links(&text) {
                    assert!(
                        link.starts_with(&u("/")) || link == state.base.prefix(),
                        "{path} links to {link}, outside the base"
                    );
                    queue.push(link);
                }
            }
        }
        assert!(
            seen.len() >= 20,
            "{prefix}: only {} urls crawled: {seen:?}",
            seen.len()
        );
        for want in [
            "/groups",
            "/topics/orders/log",
            "/topics/orders/config",
            "/topics/orders/producers",
            "/topics/new",
        ] {
            assert!(
                seen.iter().any(|s| s.starts_with(&u(want))),
                "{prefix}: {want} was never reached: {seen:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_dialog_is_a_full_page_to_a_browser_and_its_body_alone_to_a_script() {
    let (app, _) = app("/", &[]);
    for (path, id) in [
        ("/topics/orders/produce", "produce"),
        ("/topics/orders/partitions/add", "add-partitions"),
        (
            "/topics/orders/delete-records?partition=0",
            "delete-records",
        ),
        ("/topics/orders/delete", "delete-topic"),
    ] {
        let (res, page) = send(&app, get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        assert!(
            page.starts_with("<!DOCTYPE html>")
                && page.contains(&format!(
                    "<div class=\"modal\" role=\"dialog\" aria-modal=\"true\" aria-labelledby=\"{id}-title\""
                )),
            "{path}: {page:.300}"
        );
        assert!(page.contains("<nav class=\"rail\""), "{path}: in the shell");
        let mut ask = get(path);
        ask.headers_mut().insert("x-fragment", "1".parse().unwrap());
        let (res, alone) = send(&app, ask).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            alone.starts_with("<div class=\"ovl ovl-dialog root\" data-overlay")
                && alone.contains(&format!("aria-labelledby=\"{id}-title\"")),
            "{path}: {alone:.200}"
        );
        assert!(
            alone.contains("<form method=\"post\"")
                && !alone.contains("<html")
                && !alone.contains("<nav"),
            "{path}"
        );
        // Either way the dialog posts to its own address, so it works with no script.
        assert!(
            page.contains(&format!("action=\"{}\"", path.split('?').next().unwrap())),
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_fixture_world_answers_each_post_with_the_boards_state() {
    let (app, _) = app("/", &[]);
    let (status, taken) = send(
        &app,
        post(
            "/topics/new",
            "name=orders&partitions=3&cleanup.policy=delete&retention.ms=1&mode=create",
        ),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    assert!(
        taken.contains("Refused · TOPIC_ALREADY_EXISTS (36)"),
        "{taken:.300}"
    );
    let (_, made) = send(&app, post("/topics/new", "name=orders.retry&partitions=3&cleanup.policy=delete&retention.ms=604800000&mode=create")).await;
    assert!(made.contains("Created orders.retry."));
    let (_, keyless) = send(
        &app,
        post("/topics/inventory.levels/produce", "key=&value=v&acks=-1"),
    )
    .await;
    assert!(keyless.contains("Refused · INVALID_RECORD (87)"));
    let (_, sent) = send(
        &app,
        post(
            "/topics/orders/produce",
            "partition=2&key=k&value=v&acks=-1",
        ),
    )
    .await;
    assert!(sent.contains("Sent to orders-2 at offset 16,058."));
    let (_, refused) = send(&app, post("/topics/orders/partitions/add", "count=2")).await;
    assert!(refused.contains("INVALID_PARTITIONS (37)"));
    let (res, _) = send(&app, post("/topics/orders/partitions/add", "count=5")).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let (_, policy) = send(
        &app,
        post(
            "/topics/inventory.levels/delete-records",
            "partition=0&offset=5",
        ),
    )
    .await;
    assert!(policy.contains("POLICY_VIOLATION (44)"));
    let (_, gone) = send(&app, post("/topics/orders/delete", "confirm=orders")).await;
    assert!(gone.contains("Deleted orders."));
    let (res, _) = send(&app, post("/topics/ghost/delete", "confirm=ghost")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
