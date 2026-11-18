//! The door to other machines (SEC-250), over a real broker and its Kafka listener: a console served beyond loopback that only an
//! open broker let start without `--console-unprotected` refuses every API request from another machine while the broker has an
//! ACL, made over Kafka or through the console itself, and answers again once none is left. A request from this machine is never
//! refused, nor is anyone when `--console-unprotected` was given. The peer is the `ConnectInfo` the connection loop puts on every
//! request.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_live;
mod support_write;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use bytes::{Buf, Bytes};
use http_body_util::BodyExt;
use kafka_console::base::Base;
use kafka_console::door::{CLOSED, Door};
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_protocol::messages::ResponseHeader;
use kafka_protocol::protocol::Decodable;
use kafka_wire::messages::create_acls_request::AclCreation;
use kafka_wire::messages::delete_acls_request::DeleteAclsFilter;
use kafka_wire::messages::*;
use support::api_get;
use support_live::{Wire, s};
use support_write::{World, page, post_form, world};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const OTHER: &str = "192.168.1.20:51000";
const LOCAL: &str = "127.0.0.1:51000";

/// The console as the binary serves it beyond loopback: with the door, or (`--console-unprotected`) without.
fn console(w: &World, door: bool) -> Router {
    let mut live = Live::new(w.backend.clone(), w.facts.clone());
    if door {
        live = live.with_door(Door {
            local: Some("http://127.0.0.1:9093/".to_owned()),
        });
    }
    let state = State::new(
        Source::Live(Box::new(live)),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    kafka_console::app::console(state)
}

/// `req` as if it came over a connection from `peer`.
fn from(peer: &str, mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    req
}

/// The status and the JSON of a GET of `path` from `peer`.
async fn get(app: &Router, peer: &str, path: &str) -> (StatusCode, serde_json::Value) {
    let (status, text) = page(app, from(peer, api_get(path))).await;
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

/// A Kafka request over the real listener and its decoded answer.
async fn kafka<Q: kafka_protocol::protocol::Encodable, A: Decodable>(
    wire: &mut Wire,
    key: ApiKey,
    version: i16,
    q: &Q,
) -> A {
    let mut body: Bytes = wire.call(key, version, "door-test", q).await;
    ResponseHeader::decode(&mut body, key.response_header_version(version)).unwrap();
    let answer = A::decode(&mut body, version).unwrap();
    assert!(!body.has_remaining());
    answer
}

async fn create_acl(wire: &mut Wire) {
    let mut acl = AclCreation::default();
    acl.resource_type = 2;
    acl.resource_name = s("elsewhere");
    acl.resource_pattern_type = 3;
    acl.principal = s("User:someone");
    acl.host = s("*");
    acl.operation = 8;
    acl.permission_type = 2;
    let mut q = CreateAclsRequest::default();
    q.creations.push(acl);
    let made: CreateAclsResponse = kafka(wire, ApiKey::CreateAcls, 3, &q).await;
    assert_eq!(made.results[0].error_code, 0, "{made:?}");
}

async fn delete_every_acl(wire: &mut Wire) {
    let mut f = DeleteAclsFilter::default();
    f.resource_type_filter = 1;
    f.pattern_type_filter = 1;
    f.operation = 1;
    f.permission_type = 1;
    f.resource_name_filter = None;
    f.principal_filter = None;
    f.host_filter = None;
    let mut q = DeleteAclsRequest::default();
    q.filters.push(f);
    let gone: DeleteAclsResponse = kafka(wire, ApiKey::DeleteAcls, 3, &q).await;
    assert_eq!(gone.filter_results[0].error_code, 0, "{gone:?}");
    assert!(!gone.filter_results[0].matching_acls.is_empty(), "{gone:?}");
}

fn refused(status: StatusCode, body: &serde_json::Value) {
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["code"], CLOSED, "{body}");
    let words = body["message"].as_str().unwrap();
    assert!(
        words.contains("This broker now has ACLs, so the console no longer answers other machines"),
        "{words}"
    );
    assert!(words.contains("http://127.0.0.1:9093/"), "{words}");
    assert!(words.contains("--console-unprotected"), "{words}");
}

#[tokio::test]
async fn an_acl_made_over_kafka_closes_the_console_to_other_machines_until_none_is_left() {
    let w = world("door-kafka", |_, _| {}).await;
    let app = console(&w, true);
    let mut wire = Wire::connect(&w).await;
    // An open broker: another machine is answered like this one.
    for peer in [OTHER, LOCAL] {
        let (status, home) = get(&app, peer, "/api/v1/home").await;
        assert_eq!(status, StatusCode::OK, "{peer}: {home}");
    }
    // Once CreateAcls is answered, the very next request from another machine is refused: the count is in force before the
    // answer is sent.
    create_acl(&mut wire).await;
    for path in [
        "/api/v1/home",
        "/api/v1/shell",
        "/api/v1/topics",
        "/api/v1/access",
        "/api/v1/no-such",
    ] {
        let (status, body) = get(&app, OTHER, path).await;
        refused(status, &body);
    }
    // A write is refused before it is looked at.
    let mut post = post_form("/api/v1/topics/new", &[("name", "sneaky")]);
    post.headers_mut().insert("x-console", "1".parse().unwrap());
    let (status, text) = page(&app, from(OTHER, post)).await;
    refused(status, &serde_json::from_str(&text).unwrap());
    // The page and its files still load, so the page can say why.
    let (status, index) = page(&app, from(OTHER, support::get("/topics"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(index.contains("<meta name=\"doze-base\""), "{index:.200}");
    // This machine is answered, IPv6 and IPv4-mapped loopback too.
    for peer in [LOCAL, "[::1]:51000", "[::ffff:127.0.0.1]:51000"] {
        let (status, access) = get(&app, peer, "/api/v1/access").await;
        assert_eq!(status, StatusCode::OK, "{peer}: {access}");
    }
    // A request with no peer known is not taken to be this machine.
    let (status, body) = page(&app, api_get("/api/v1/home")).await;
    refused(status, &serde_json::from_str(&body).unwrap());
    // Every ACL deleted: the broker is open again, so is the console.
    delete_every_acl(&mut wire).await;
    let (status, home) = get(&app, OTHER, "/api/v1/home").await;
    assert_eq!(status, StatusCode::OK, "{home}");
}

#[tokio::test]
async fn an_acl_made_through_the_console_closes_it_to_the_machine_that_made_it() {
    let w = world("door-console", |_, _| {}).await;
    let app = console(&w, true);
    let acl = serde_json::json!({
        "type": "topic", "name": "orders", "pattern": "literal", "principal": "User:orders",
        "host": "*", "op": "write", "perm": "allow",
    });
    let post = Request::builder()
        .method("POST")
        .uri("/api/v1/access/acls/new")
        .header("host", support::HOST)
        .header("origin", format!("http://{}", support::HOST))
        .header("x-console", "1")
        .header("content-type", "application/json")
        .body(Body::from(acl.to_string()))
        .unwrap();
    let (status, made) = page(&app, from(OTHER, post)).await;
    assert_eq!(status, StatusCode::OK, "{made:.300}");
    assert_eq!(w.broker.acl_count(), 1, "{made:.300}");
    let (status, body) = get(&app, OTHER, "/api/v1/access").await;
    refused(status, &body);
    assert_eq!(get(&app, LOCAL, "/api/v1/access").await.0, StatusCode::OK);
}

#[tokio::test]
async fn unprotected_or_on_loopback_the_console_is_never_closed() {
    let w = world("door-unprotected", |_, _| {}).await;
    let app = console(&w, false);
    let mut wire = Wire::connect(&w).await;
    create_acl(&mut wire).await;
    for peer in [OTHER, LOCAL] {
        let (status, home) = get(&app, peer, "/api/v1/home").await;
        assert_eq!(status, StatusCode::OK, "{peer}: {home}");
    }
}

#[tokio::test]
async fn a_tail_open_to_another_machine_ends_once_the_broker_has_acls() {
    let w = world("door-tail", |_, _| {}).await;
    w.create_topic("orders", 1, &[]).await;
    let app = console(&w, true);
    let mut wire = Wire::connect(&w).await;
    let res = app
        .clone()
        .oneshot(from(
            OTHER,
            api_get("/api/v1/tail/messages?topic=orders&pace=1"),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let mut body = res.into_body();
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(
        String::from_utf8_lossy(&first).contains("ready"),
        "{first:?}"
    );
    // While the broker is open, the tail tells another machine what was written.
    let batch = support_live::batch(Some("k"), b"before the ACL".to_vec(), None);
    let _: Bytes = wire
        .call(
            ApiKey::Produce,
            9,
            "door-test",
            &support_live::produce("orders", batch, None),
        )
        .await;
    let sent = tokio::time::timeout(Duration::from_secs(10), body.frame())
        .await
        .expect("the tail sent nothing")
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&sent).contains("event:"),
        "{sent:?}"
    );
    create_acl(&mut wire).await;
    // Something to tell: the tail would send it, and ends instead.
    let batch = support_live::batch(Some("k"), b"after the ACL".to_vec(), None);
    let _: Bytes = wire
        .call(
            ApiKey::Produce,
            9,
            "door-test",
            &support_live::produce("orders", batch, None),
        )
        .await;
    let next = tokio::time::timeout(Duration::from_secs(10), body.frame())
        .await
        .expect("the tail neither ended nor sent");
    assert!(next.is_none(), "the tail sent after the ACL: {next:?}");
}
