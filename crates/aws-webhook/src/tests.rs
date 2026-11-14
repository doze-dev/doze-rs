//! The client against a real local TLS server: a trusted certificate is called, an untrusted one is refused with the flag that
//! fixes it, `--ca-file` and `--insecure-outbound-tls` each fix it, and API Gateway's lenient mode still checks the name.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use super::test_server::{Ca, Identity, ok, serve};
use super::*;

fn ca_only() -> Client {
    Client::new(Ca::get().trust())
}

fn answering(body: &'static str) -> test_server::Answerer {
    Arc::new(move |_| {
        format!(
            "HTTP/1.1 201 Created\r\nX-Seen: yes\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    })
}

#[tokio::test]
async fn a_server_the_ca_signed_is_called_and_answers() {
    let server = serve(Ca::get().identity(), answering("hello")).await;
    let url = format!("{}/hook?x=1", server.url());
    let headers = vec![("X-Mine".to_owned(), "1".to_owned())];
    let r = Request {
        headers: &headers,
        body: b"{}",
        ..Request::new("POST", &url)
    };
    let a = ca_only().fetch(&r).await.unwrap();
    assert_eq!((a.status, &a.body[..]), (201, &b"hello"[..]));
    assert!(a.headers.contains(&("X-Seen".to_owned(), "yes".to_owned())));
    let seen = server.seen(1).await;
    assert!(seen[0].starts_with("POST /hook?x=1 HTTP/1.1\r\n"), "{}", seen[0]);
    assert!(seen[0].contains(&format!("Host: 127.0.0.1:{}\r\n", server.port)));
    assert!(seen[0].ends_with("\r\n\r\n{}"));
    // By name too: the certificate is for localhost.
    let url = format!("https://localhost:{}/", server.port);
    assert_eq!(ca_only().fetch(&Request::new("GET", &url)).await.unwrap().status, 201);
}

#[tokio::test]
async fn a_self_signed_server_is_refused_and_the_error_names_both_flags() {
    let server = serve(Identity::self_signed(), ok()).await;
    let url = server.url();
    let Err(Failure::Tls(why)) = ca_only().fetch(&Request::new("GET", &url)).await else {
        panic!("a self-signed certificate was accepted");
    };
    assert!(why.contains("not signed by an authority this machine trusts"), "{why}");
    assert!(why.contains("--ca-file") && why.contains("--insecure-outbound-tls"), "{why}");
    assert!(why.contains(&format!("127.0.0.1:{}", server.port)), "{why}");
    assert!(server.seen(0).await.is_empty());
}

#[tokio::test]
async fn a_ca_file_holding_the_servers_own_certificate_lets_it_through() {
    let identity = Identity::self_signed();
    let dir = std::env::temp_dir().join(format!("aws-webhook-cafile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("server.pem");
    std::fs::write(&file, identity.pem()).unwrap();
    let server = serve(identity, ok()).await;
    let mut trust = Trust::new(Some(&file), false).unwrap();
    trust.os_store = false;
    let a = Client::new(trust).fetch(&Request::new("GET", &server.url())).await;
    assert_eq!(a.unwrap().status, 200);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn the_insecure_flag_accepts_any_certificate() {
    let server = serve(Identity::self_signed(), ok()).await;
    let insecure = Client::new(Trust {
        insecure: true,
        ..Trust::default()
    });
    let a = insecure.fetch(&Request::new("GET", &server.url())).await;
    assert_eq!(a.unwrap().status, 200);
}

#[tokio::test]
async fn skip_verification_accepts_an_unknown_issuer_but_still_checks_the_name() {
    let server = serve(Identity::self_signed(), ok()).await;
    let url = server.url();
    let lenient = Request {
        skip_verification: true,
        ..Request::new("GET", &url)
    };
    assert_eq!(ca_only().fetch(&lenient).await.unwrap().status, 200);
    // The certificate is for localhost and 127.0.0.1; 127.0.0.2 is the same machine under a name it does not list.
    let other = format!("https://127.0.0.2:{}/", server.port);
    let wrong_name = Request {
        skip_verification: true,
        ..Request::new("GET", &other)
    };
    match ca_only().fetch(&wrong_name).await {
        Err(Failure::Tls(why)) => assert!(why.contains("is not for the name 127.0.0.2"), "{why}"),
        // macOS has no 127.0.0.2 unless an alias is made; then nothing can be said about the name.
        Err(Failure::Connect(_) | Failure::Timeout(_)) => {}
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn plain_http_still_works_and_a_silent_server_times_out() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((mut s, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).await.unwrap();
            if String::from_utf8_lossy(&buf[..n]).contains("/slow") {
                held.push(s);
                continue;
            }
            s.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").await.unwrap();
        }
    });
    let url = format!("http://127.0.0.1:{port}/fast");
    let r = Request {
        status_only: true,
        ..Request::new("POST", &url)
    };
    assert_eq!(ca_only().fetch(&r).await.unwrap().status, 204);
    let url = format!("http://127.0.0.1:{port}/slow");
    let r = Request {
        timeout: Duration::from_millis(200),
        ..Request::new("POST", &url)
    };
    let Err(Failure::Timeout(why)) = ca_only().fetch(&r).await else {
        panic!("no timeout");
    };
    assert!(why.contains("in 200ms"), "{why}");
    let Err(Failure::Unsupported(_)) = ca_only().fetch(&Request::new("GET", "ftp://x/")).await
    else {
        panic!("ftp was tried");
    };
}

#[tokio::test]
async fn the_os_store_is_read_and_does_not_trust_a_self_signed_server() {
    let server = serve(Identity::self_signed(), ok()).await;
    let os = Client::new(Trust::new(None, false).unwrap());
    let Err(Failure::Tls(why)) = os.fetch(&Request::new("GET", &server.url())).await else {
        panic!("the OS store trusted a certificate made a moment ago");
    };
    assert!(why.contains("--ca-file"), "{why}");
}
