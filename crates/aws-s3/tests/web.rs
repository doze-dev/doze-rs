//! What a browser sees: CORS preflights and the headers of actual requests, website pages by host and by path, and the lifecycle
//! hook, all over a real socket through the gateway.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_gateway::{Config, Gateway};
use aws_s3::S3;
use doze_runtime::{Ctx, ManualClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const AUTH: &str = "Authorization: AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=00\r\nx-amz-date: 20260101T000000Z\r\n";

/// The status, the headers (names lower-cased) and the body.
struct Got {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Got {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

async fn http(addr: SocketAddr, method: &str, path: &str, extra: &str, body: &str) -> Got {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let host = if extra.contains("Host:") {
        String::new()
    } else {
        "Host: 127.0.0.1\r\n".to_owned()
    };
    let head = format!(
        "{method} {path} HTTP/1.1\r\n{host}Connection: close\r\n{extra}Content-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).await.unwrap();
    s.write_all(body.as_bytes()).await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
        .collect();
    Got {
        status,
        headers,
        body: body.to_owned(),
    }
}

async fn start(name: &str) -> (SocketAddr, Arc<S3>, Tasks) {
    let dir = std::env::temp_dir().join(format!("aws-s3-web-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let tasks = Tasks::new();
    let ctx = Arc::new(Ctx {
        dir,
        idle: Duration::from_secs(300),
        tasks: tasks.clone(),
        clock: Arc::new(ManualClock::at(1_700_000_000_000)),
        ext: None,
    });
    let s3 = Arc::new(S3::new(ctx));
    let running = Gateway::new(Config::new("127.0.0.1:0".parse().unwrap()))
        .with(s3.clone())
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    std::mem::forget(running);
    (addr, s3, tasks)
}

const CORS: &str = "<CORSConfiguration>\
<CORSRule><AllowedOrigin>http://localhost:3000</AllowedOrigin><AllowedMethod>PUT</AllowedMethod><AllowedMethod>GET</AllowedMethod>\
<AllowedHeader>*</AllowedHeader><ExposeHeader>ETag</ExposeHeader><MaxAgeSeconds>600</MaxAgeSeconds></CORSRule>\
</CORSConfiguration>";

#[tokio::test]
async fn a_browser_upload_is_preflighted_and_its_answer_carries_the_cors_headers() {
    let (addr, _s3, _t) = start("cors").await;
    assert_eq!(http(addr, "PUT", "/upl", AUTH, "").await.status, 200);
    let preflight = "Origin: http://localhost:3000\r\nAccess-Control-Request-Method: PUT\r\nAccess-Control-Request-Headers: content-type\r\n";
    // No configuration yet: refused.
    let refused = http(addr, "OPTIONS", "/upl/photo.jpg", preflight, "").await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    assert!(
        refused.body.contains("<Code>AccessForbidden</Code>"),
        "{}",
        refused.body
    );
    assert!(refused.body.contains("CORS is not enabled for this bucket"));
    let put = http(addr, "PUT", "/upl?cors", AUTH, CORS).await;
    assert_eq!(put.status, 200, "{}", put.body);

    let ok = http(addr, "OPTIONS", "/upl/photo.jpg", preflight, "").await;
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert_eq!(
        ok.header("access-control-allow-origin"),
        Some("http://localhost:3000")
    );
    assert_eq!(ok.header("access-control-allow-methods"), Some("PUT, GET"));
    assert_eq!(
        ok.header("access-control-allow-headers"),
        Some("content-type")
    );
    assert_eq!(ok.header("access-control-max-age"), Some("600"));
    assert_eq!(ok.header("access-control-expose-headers"), Some("ETag"));

    // Another origin, or a method no rule names: 403.
    let other = http(
        addr,
        "OPTIONS",
        "/upl/photo.jpg",
        "Origin: http://evil.test\r\nAccess-Control-Request-Method: PUT\r\n",
        "",
    )
    .await;
    assert_eq!(other.status, 403);
    assert!(
        other.body.contains("<Method>PUT</Method>")
            && other.body.contains("<ResourceType>OBJECT</ResourceType>")
    );
    let delete = http(
        addr,
        "OPTIONS",
        "/upl/photo.jpg",
        "Origin: http://localhost:3000\r\nAccess-Control-Request-Method: DELETE\r\n",
        "",
    )
    .await;
    assert_eq!(delete.status, 403);
    // A virtual-hosted preflight.
    let hosted = http(
        addr,
        "OPTIONS",
        "/photo.jpg",
        &format!("Host: upl.s3.localhost:{}\r\n{preflight}", addr.port()),
        "",
    )
    .await;
    assert_eq!(hosted.status, 200);
    // A bucket that is not there.
    assert_eq!(
        http(addr, "OPTIONS", "/nope/k", preflight, "").await.status,
        404
    );

    // The actual upload (here signed; a presigned PUT is the same request) carries the headers too.
    let upload = http(
        addr,
        "PUT",
        "/upl/photo.jpg",
        &format!("{AUTH}Origin: http://localhost:3000\r\n"),
        "jpeg",
    )
    .await;
    assert_eq!(upload.status, 200);
    assert_eq!(
        upload.header("access-control-allow-origin"),
        Some("http://localhost:3000")
    );
    assert_eq!(upload.header("access-control-expose-headers"), Some("ETag"));
    assert!(upload.header("etag").is_some());
    // A method the rule does not allow is served, without the headers.
    let removed = http(
        addr,
        "DELETE",
        "/upl/photo.jpg",
        &format!("{AUTH}Origin: http://localhost:3000\r\n"),
        "",
    )
    .await;
    assert_eq!(removed.status, 204);
    assert_eq!(removed.header("access-control-allow-origin"), None);
    // An error answer is decorated as well.
    let missing = http(
        addr,
        "GET",
        "/upl/none",
        &format!("{AUTH}Origin: http://localhost:3000\r\n"),
        "",
    )
    .await;
    assert_eq!(missing.status, 404);
    assert_eq!(
        missing.header("access-control-allow-origin"),
        Some("http://localhost:3000")
    );
}

const WEBSITE: &str = "<WebsiteConfiguration><IndexDocument><Suffix>index.html</Suffix></IndexDocument>\
<ErrorDocument><Key>404.html</Key></ErrorDocument></WebsiteConfiguration>";

#[tokio::test]
async fn a_website_serves_its_index_and_error_documents() {
    let (addr, _s3, _t) = start("site").await;
    http(addr, "PUT", "/site", AUTH, "").await;
    let html = format!("{AUTH}Content-Type: text/html\r\n");
    http(addr, "PUT", "/site/index.html", &html, "<h1>home</h1>").await;
    http(addr, "PUT", "/site/docs/index.html", &html, "<h1>docs</h1>").await;
    http(addr, "PUT", "/site/page.html", &html, "<p>page</p>").await;

    // Not a website yet.
    let before = http(addr, "GET", "/_aws/s3-website/site/", "", "").await;
    assert_eq!(before.status, 404);
    assert!(
        before.body.contains("NoSuchWebsiteConfiguration"),
        "{}",
        before.body
    );
    assert_eq!(
        http(addr, "PUT", "/site?website", AUTH, WEBSITE)
            .await
            .status,
        200
    );

    let root = http(addr, "GET", "/_aws/s3-website/site/", "", "").await;
    assert_eq!((root.status, root.body.as_str()), (200, "<h1>home</h1>"));
    assert_eq!(root.header("content-type"), Some("text/html"));
    let docs = http(addr, "GET", "/_aws/s3-website/site/docs/", "", "").await;
    assert_eq!(docs.body, "<h1>docs</h1>");
    let page = http(addr, "GET", "/_aws/s3-website/site/page.html", "", "").await;
    assert_eq!(page.body, "<p>page</p>");
    // A folder named without its slash is redirected to it.
    let folder = http(addr, "GET", "/_aws/s3-website/site/docs", "", "").await;
    assert_eq!(folder.status, 302);
    assert_eq!(
        folder.header("location"),
        Some("/_aws/s3-website/site/docs/")
    );
    // No error document yet: AWS's HTML page, 404.
    let missing = http(addr, "GET", "/_aws/s3-website/site/gone.html", "", "").await;
    assert_eq!(missing.status, 404);
    assert!(
        missing.body.contains("<li>Code: NoSuchKey</li>")
            && missing.body.contains("<li>Key: gone.html</li>"),
        "{}",
        missing.body
    );
    http(addr, "PUT", "/site/404.html", &html, "<h1>lost</h1>").await;
    let custom = http(addr, "GET", "/_aws/s3-website/site/gone.html", "", "").await;
    assert_eq!(
        (custom.status, custom.body.as_str()),
        (404, "<h1>lost</h1>")
    );

    // The website host form, as AWS names it and as a local host.
    for host in [
        "site.s3-website-us-east-1.amazonaws.com".to_owned(),
        format!("site.s3-website.localhost:{}", addr.port()),
    ] {
        let got = http(addr, "GET", "/", &format!("Host: {host}\r\n"), "").await;
        assert_eq!(
            (got.status, got.body.as_str()),
            (200, "<h1>home</h1>"),
            "{host}"
        );
        let redirect = http(addr, "GET", "/docs", &format!("Host: {host}\r\n"), "").await;
        assert_eq!(redirect.header("location"), Some("/docs/"));
    }
    // A page is read with GET or HEAD only.
    assert_eq!(
        http(addr, "POST", "/_aws/s3-website/site/", "", "")
            .await
            .status,
        405
    );
    let head = http(addr, "HEAD", "/_aws/s3-website/site/", "", "").await;
    assert_eq!((head.status, head.body.as_str()), (200, ""));
    // The REST API still answers NoSuchKey for the same miss.
    let rest = http(addr, "GET", "/site/gone.html", AUTH, "").await;
    assert!(rest.body.contains("<Code>NoSuchKey</Code>"));
    // An object's website redirect location.
    http(
        addr,
        "PUT",
        "/site/old.html",
        &format!("{AUTH}x-amz-website-redirect-location: /page.html\r\n"),
        "",
    )
    .await;
    let moved = http(addr, "GET", "/_aws/s3-website/site/old.html", "", "").await;
    assert_eq!(moved.status, 301);
    assert_eq!(
        moved.header("location"),
        Some("/_aws/s3-website/site/page.html")
    );
}

#[tokio::test]
async fn the_lifecycle_hook_sweeps_a_region_now() {
    let (addr, _s3, _t) = start("hook").await;
    http(addr, "PUT", "/life", AUTH, "").await;
    let swept = http(addr, "POST", "/_aws/s3/lifecycle/us-east-1", "", "").await;
    assert_eq!(swept.status, 200, "{}", swept.body);
    assert_eq!(swept.body, r#"{"Actions":0,"Armed":false}"#);
    assert_eq!(
        http(addr, "POST", "/_aws/s3/lifecycle/mars-1", "", "")
            .await
            .status,
        404
    );
}
