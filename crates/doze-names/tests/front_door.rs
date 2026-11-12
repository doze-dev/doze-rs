//! The front door against real sockets on free loopback ports and a private home (never the machine's :80): routing
//! by Host with the Host kept, the 404 that says what is fronted, take-over, https for a declared domain from the doze
//! authority or the person's own files, the redirect from http, following a rotation, and a WebSocket carried through.
//! Go: ingress_test.go, ca_test.go.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod front_door_support;

use doze_names::ca::Ca;
use doze_names::front_door::serve;
use doze_names::{Name, Registry};
use front_door_support::{backend, config, get, https_get, scratch, until};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_peer_fronts_every_peers_routes_keeping_the_host() {
    let home = scratch("routes");
    let aws = Registry::open(&home, "doze-aws");
    aws.claim(&Name::qualified("aws", "harbour"))
        .unwrap()
        .route(&backend("aws").await.to_string())
        .unwrap();
    let kafka = Registry::open(&home, "doze-kafka");
    let cfg = config(&kafka, &home.join("ca"));
    let plain = cfg.plain.clone();
    let mut door = serve(cfg);
    assert!(door.bound(Duration::from_secs(5)).await);
    let (status, body) = get(&plain, "sqs.ap-south-1.aws.harbour.doze").await;
    assert!(status.contains("200"), "{status}");
    assert_eq!(
        body, "aws sqs.ap-south-1.aws.harbour.doze",
        "the Host is kept: doze-aws builds URLs from it"
    );
    let (status, body) = get(&plain, "aws.harbor.doze").await;
    assert!(
        status.contains("404") && body.contains("http://aws.harbour.doze"),
        "{status}\n{body}"
    );
    assert_eq!(
        kafka.ingress_holder().map(|e| e.owner).as_deref(),
        Some("doze-kafka")
    );
    door.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_takes_the_front_door_over_when_the_holder_exits() {
    let home = scratch("takeover");
    let web = Registry::open(&home, "doze");
    web.claim(&Name::qualified("web", "shop"))
        .unwrap()
        .route(&backend("web").await.to_string())
        .unwrap();
    let first = config(&web, &home.join("ca"));
    let mut second_cfg = first.clone();
    second_cfg.registry = Registry::open(&home, "doze-aws");
    let plain = first.plain.clone();
    let mut a = serve(first);
    assert!(a.bound(Duration::from_secs(5)).await);
    let mut b = serve(second_cfg);
    assert!(
        !b.bound(Duration::from_millis(200)).await,
        "the second took a port the first held"
    );
    a.close().await;
    assert!(
        b.bound(Duration::from_secs(6)).await,
        "the standby never took over"
    );
    assert_eq!(get(&plain, "web.shop.doze").await.1, "web web.shop.doze");
    b.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_domain_is_served_over_https_and_http_redirects_and_a_rotation_is_followed() {
    let home = scratch("https");
    let reg = Registry::open(&home, "doze");
    let web = reg.claim(&Name::qualified("web", "shop")).unwrap();
    let dom = web.twin(&Name::domain("dev.acme.com")).unwrap();
    dom.route(&backend("web").await.to_string()).unwrap();
    let ca_dir = home.join("ca");
    let cfg = config(&reg, &ca_dir);
    let (plain, tls) = (cfg.plain.clone(), cfg.tls.clone());
    let mut door = serve(cfg);
    assert!(door.bound(Duration::from_secs(5)).await);
    until(|| reg.ingress_tls_holder().is_some()).await;
    assert!(
        https_get(&tls, "dev.acme.com", None).await.is_err(),
        "no authority and no files: no handshake"
    );
    let first = Ca::load_or_create(&ca_dir).unwrap();
    let (status, body) = https_get(&tls, "dev.acme.com", Some(first.root_der()))
        .await
        .unwrap();
    assert!(
        status.contains("200") && body == "web dev.acme.com",
        "{status} {body}"
    );
    let (status, _) = get(&plain, "dev.acme.com").await;
    assert!(
        status.contains("307"),
        "plain http to a declared domain goes to https: {status}"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    let second = Ca::rotate(&ca_dir).unwrap();
    assert!(
        https_get(&tls, "dev.acme.com", Some(second.root_der()))
            .await
            .is_ok(),
        "the new root signs"
    );
    assert!(
        https_get(&tls, "dev.acme.com", Some(first.root_der()))
            .await
            .is_err(),
        "the old one no longer does"
    );
    door.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_own_files_are_served_with_no_authority() {
    // Go: TestTheFrontDoorServesAPersonsOwnFilesWithoutAnAuthority.
    let home = scratch("own");
    let other = Ca::load_or_create(&home.join("other")).unwrap();
    other.leaf("api.acme.com").unwrap();
    let reg = Registry::open(&home, "doze");
    let api = reg.claim(&Name::qualified("api", "shop")).unwrap();
    let dom = api.twin(&Name::domain("api.acme.com")).unwrap();
    let certs = home.join("other/certs");
    let (crt, key) = (
        certs.join("api.acme.com.crt"),
        certs.join("api.acme.com.key"),
    );
    dom.certificate(crt.to_str().unwrap(), key.to_str().unwrap())
        .unwrap();
    dom.route(&backend("own").await.to_string()).unwrap();
    let no_ca = home.join("ca");
    let cfg = config(&reg, &no_ca);
    let tls = cfg.tls.clone();
    let door = serve(cfg);
    until(|| reg.ingress_tls_holder().is_some()).await;
    let (_, body) = https_get(&tls, "api.acme.com", Some(other.root_der()))
        .await
        .unwrap();
    assert_eq!(body, "own api.acme.com");
    assert!(
        !no_ca.join("root.crt").exists(),
        "serving the person's files must not make an authority"
    );
    door.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_websocket_upgrade_is_carried_through() {
    let home = scratch("ws");
    let reg = Registry::open(&home, "doze");
    reg.claim(&Name::qualified("vite", "shop"))
        .unwrap()
        .route(&backend("vite").await.to_string())
        .unwrap();
    let cfg = config(&reg, &home.join("ca"));
    let plain = cfg.plain.clone();
    let mut door = serve(cfg);
    assert!(door.bound(Duration::from_secs(5)).await);
    let mut s = TcpStream::connect(&plain).await.unwrap();
    let req = "GET /hmr HTTP/1.1\r\nHost: vite.shop.doze\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n";
    s.write_all(req.as_bytes()).await.unwrap();
    let mut head = vec![0u8; 512];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut head))
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&head[..n]).starts_with("HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&head[..n])
    );
    s.write_all(b"ping").await.unwrap();
    let mut echo = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut echo))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&echo, b"ping", "bytes flow both ways after the switch");
    door.close().await;
}
