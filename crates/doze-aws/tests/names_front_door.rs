//! doze-aws behind the zone's front door, end to end, over http and https: the real binary on a free port, a throwaway
//! zone (its own home, registry and authority, the front door on free loopback ports: never :80/:443, the machine's
//! resolver, its trust store or ~/.doze), the instance's name `cloud.shop.doze` routed to it, and a client that trusts
//! only the test's authority and dials the front door itself, as a resolver pointing every `.doze` name at it would.
//!
//! What it proves, each over both schemes:
//! - a deployed REST API answers at `<api id>.execute-api.<region>.cloud.shop.doze/<stage>/<path>`;
//! - a Lambda function URL answers at `<url id>.lambda-url.<region>.cloud.shop.doze/` (needs `sh` and `curl`: the
//!   `provided` runtime is a shell loop over the Runtime API; skipped, and said, without them);
//! - an S3 website answers at `<bucket>.s3-website.<region>.cloud.shop.doze`, and an object virtual-hosted at
//!   `<bucket>.s3.<region>.cloud.shop.doze`; a bucket named straight under the instance (`<bucket>.cloud.shop.doze`) is
//!   not virtual-hosted today: the gateway reads it path-style, so the key is taken for the bucket;
//! - a name nobody registered gets no handshake over https and the front door's 404 over http.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod names_support;

use std::time::Duration;

use doze_names::ca::Ca;
use doze_names::front_door::{FrontConfig, serve};
use doze_names::{Name, Registry};
use names_support::{
    Door, REGION, Scheme, direct, free_port, have, json, signed, start, zip_bootstrap,
};

const INSTANCE: &str = "cloud.shop.doze";

/// A REST API with `GET /hello` answered by a mock integration, deployed to stage `dev`; its id.
async fn rest_api(endpoint: &str) -> String {
    let h = signed("apigateway");
    let mut h_json = h.clone();
    h_json.push(("Content-Type".into(), "application/json".into()));
    let made = direct(
        endpoint,
        "POST",
        "/restapis",
        &h_json,
        br#"{"name":"shop"}"#,
    )
    .await;
    assert_eq!(made.status, 201, "{} {}", made.head, made.body);
    let api = json(&made)["id"].as_str().unwrap().to_owned();
    let resources = direct(
        endpoint,
        "GET",
        &format!("/restapis/{api}/resources"),
        &h,
        b"",
    )
    .await;
    let root = json(&resources)["item"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let made = direct(
        endpoint,
        "POST",
        &format!("/restapis/{api}/resources/{root}"),
        &h_json,
        br#"{"pathPart":"hello"}"#,
    )
    .await;
    assert_eq!(made.status, 201, "{} {}", made.head, made.body);
    let res = json(&made)["id"].as_str().unwrap().to_owned();
    let method = format!("/restapis/{api}/resources/{res}/methods/GET");
    for (path, body) in [
        (method.clone(), r#"{"authorizationType":"NONE"}"#.to_owned()),
        (
            format!("{method}/integration"),
            r#"{"type":"MOCK","requestTemplates":{"application/json":"{\"statusCode\": 200}"}}"#
                .to_owned(),
        ),
        (format!("{method}/responses/200"), "{}".to_owned()),
        (
            format!("{method}/integration/responses/200"),
            r#"{"responseTemplates":{"application/json":"{\"from\":\"mock\"}"}}"#.to_owned(),
        ),
    ] {
        let a = direct(endpoint, "PUT", &path, &h_json, body.as_bytes()).await;
        assert!(a.status / 100 == 2, "PUT {path}: {} {}", a.head, a.body);
    }
    let deployed = direct(
        endpoint,
        "POST",
        &format!("/restapis/{api}/deployments"),
        &h_json,
        br#"{"stageName":"dev"}"#,
    )
    .await;
    assert_eq!(deployed.status, 201, "{} {}", deployed.head, deployed.body);
    api
}

/// A `provided` function with a URL that needs no signature; the URL's id.
async fn function_url(endpoint: &str) -> String {
    let mut h = signed("lambda");
    h.push(("Content-Type".into(), "application/json".into()));
    let code = aws_core::base64::encode(&zip_bootstrap());
    let body = format!(
        r#"{{"FunctionName":"hello","Runtime":"provided.al2023","Handler":"x","Role":"arn:aws:iam::000000000000:role/r","Timeout":10,"Code":{{"ZipFile":"{code}"}}}}"#
    );
    let made = direct(
        endpoint,
        "POST",
        "/2015-03-31/functions",
        &h,
        body.as_bytes(),
    )
    .await;
    assert_eq!(made.status, 201, "{} {}", made.head, made.body);
    let made = direct(
        endpoint,
        "POST",
        "/2021-10-31/functions/hello/url",
        &h,
        br#"{"AuthType":"NONE"}"#,
    )
    .await;
    assert_eq!(made.status, 201, "{} {}", made.head, made.body);
    let url = json(&made)["FunctionUrl"].as_str().unwrap().to_owned();
    url.trim_start_matches("http://")
        .split('.')
        .next()
        .unwrap()
        .to_owned()
}

/// A bucket `site` with `index.html`, served as a website.
async fn bucket(endpoint: &str) {
    let h = signed("s3");
    let made = direct(endpoint, "PUT", "/site", &h, b"").await;
    assert_eq!(made.status, 200, "{} {}", made.head, made.body);
    let mut html = h.clone();
    html.push(("Content-Type".into(), "text/html".into()));
    let put = direct(endpoint, "PUT", "/site/index.html", &html, b"<h1>site</h1>").await;
    assert_eq!(put.status, 200, "{} {}", put.head, put.body);
    let website = b"<WebsiteConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
        <IndexDocument><Suffix>index.html</Suffix></IndexDocument></WebsiteConfiguration>";
    let put = direct(endpoint, "PUT", "/site?website", &h, website).await;
    assert_eq!(put.status, 200, "{} {}", put.head, put.body);
}

/// API Gateway: the API id is minted at run time, two labels and a region under the instance.
async fn execute_api(door: &Door, scheme: Scheme, api: &str) {
    let host = format!("{api}.execute-api.{REGION}.{INSTANCE}");
    let a = door.get(scheme, &host, "/dev/hello").await;
    assert_eq!(a.status, 200, "{scheme:?} {host}: {} {}", a.head, a.body);
    assert_eq!(json(&a)["from"], "mock", "{scheme:?}");
}

/// A Lambda function URL: the event names the host the request came in on.
async fn lambda_url(door: &Door, scheme: Scheme, id: &str) {
    let host = format!("{id}.lambda-url.{REGION}.{INSTANCE}");
    let a = door.get(scheme, &host, "/orders?n=1").await;
    assert_eq!(a.status, 200, "{scheme:?} {host}: {} {}", a.head, a.body);
    let v = json(&a);
    assert_eq!(v["from"], "lambda", "{scheme:?}");
    assert_eq!(
        v["event"]["requestContext"]["domainName"], host,
        "{scheme:?}"
    );
    assert_eq!(v["event"]["rawPath"], "/orders", "{scheme:?}");
}

/// S3: the website, an object virtual-hosted under an `s3` label, and what a bucket straight under the instance does.
async fn s3_in_host(door: &Door, scheme: Scheme) {
    let host = format!("site.s3-website.{REGION}.{INSTANCE}");
    let a = door.get(scheme, &host, "/").await;
    assert_eq!(a.status, 200, "{scheme:?} {host}: {} {}", a.head, a.body);
    assert_eq!(a.body, "<h1>site</h1>", "{scheme:?}");
    let host = format!("site.s3.{REGION}.{INSTANCE}");
    let a = door
        .ask(scheme, "GET", &host, "/index.html", &signed("s3"))
        .await
        .unwrap();
    assert_eq!(a.status, 200, "{scheme:?} {host}: {} {}", a.head, a.body);
    assert_eq!(a.body, "<h1>site</h1>", "{scheme:?}");
    // Today's truth: a bucket straight under the instance is not virtual-hosted (no `s3` label), so the path is read
    // path-style and its first segment is taken for the bucket.
    let host = format!("site.{INSTANCE}");
    let a = door
        .ask(scheme, "GET", &host, "/index.html", &signed("s3"))
        .await
        .unwrap();
    assert_eq!(a.status, 404, "{scheme:?} {host}: {} {}", a.head, a.body);
    assert!(a.body.contains("NoSuchBucket"), "{scheme:?} {}", a.body);
}

/// A name nobody registered: the front door's 404 over http, no handshake over https.
async fn nobody(door: &Door, scheme: Scheme) {
    let host = "x.nobody.shop.doze";
    match scheme {
        Scheme::Http => {
            let a = door.get(scheme, host, "/").await;
            assert_eq!(a.status, 404, "{}", a.head);
            assert!(a.body.contains("no doze service is fronting"), "{}", a.body);
        }
        Scheme::Https => assert!(
            door.ask(scheme, "GET", host, "/", &[]).await.is_err(),
            "no certificate for a name nobody registered"
        ),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doze_aws_answers_names_under_its_instance_over_http_and_https() {
    let dir = tempfile::tempdir().unwrap();
    let (_server, endpoint) = start(dir.path());

    // The zone: a home of its own, the instance's name routed to doze-aws as the core routes it.
    let home = dir.path().join("zone");
    let reg = Registry::open(&home, "doze");
    let lease = reg.claim(&Name::qualified("cloud", "shop")).unwrap();
    assert_eq!(lease.name.host, INSTANCE);
    lease.route(&endpoint).unwrap();
    let ca = Ca::load_or_create(&home.join("ca")).unwrap();
    let mut cfg = FrontConfig::for_zone(&reg, &home.join("ca"));
    cfg.plain = format!("127.0.0.1:{}", free_port());
    cfg.tls = format!("127.0.0.1:{}", free_port());
    let door = Door {
        plain: cfg.plain.clone(),
        tls: cfg.tls.clone(),
        root: ca.root_der().to_vec(),
    };
    let mut front = serve(cfg);
    assert!(
        front.bound(Duration::from_secs(5)).await,
        "the front door bound"
    );
    for _ in 0..100 {
        if reg.ingress_tls_holder().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(reg.ingress_tls_holder().is_some(), "the https port bound");

    let api = rest_api(&endpoint).await;
    let lambda = if cfg!(unix) && have("sh") && have("curl") {
        Some(function_url(&endpoint).await)
    } else {
        eprintln!("function URL case skipped: the provided runtime needs sh and curl");
        None
    };
    bucket(&endpoint).await;

    for scheme in [Scheme::Http, Scheme::Https] {
        execute_api(&door, scheme, &api).await;
        if let Some(id) = &lambda {
            lambda_url(&door, scheme, id).await;
        }
        s3_in_host(&door, scheme).await;
        nobody(&door, scheme).await;
    }
    front.close().await;
    drop(lease);
}
