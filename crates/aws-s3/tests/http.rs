//! S3 over a real socket, through the real gateway: the wire behaviours a model test cannot see (addressing, `Expect`, the
//! aws-chunked encoding, streaming, what survives a restart).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

mod support;
use support::{call, head, parse, request, start, temp};

#[tokio::test]
async fn an_object_goes_in_and_comes_out_with_its_headers() {
    let dir = temp("roundtrip");
    let srv = start(&dir).await;
    assert_eq!(call(&srv, "PUT", "/bkt", &[], b"").await.status, 200);
    let put = call(
        &srv,
        "PUT",
        "/bkt/a/b%20c.txt",
        &[
            ("Content-Type", "text/plain"),
            ("x-amz-meta-origin", "here"),
        ],
        b"hello world",
    )
    .await;
    assert_eq!(put.status, 200);
    assert_eq!(
        put.header("ETag"),
        Some("\"5eb63bbbe01eeed093cb22bb8f5acdc3\"")
    );
    let get = call(&srv, "GET", "/bkt/a/b%20c.txt", &[], b"").await;
    assert_eq!((get.status, get.text().as_str()), (200, "hello world"));
    assert_eq!(get.header("Content-Type"), Some("text/plain"));
    assert_eq!(get.header("x-amz-meta-origin"), Some("here"));
    let part = call(
        &srv,
        "GET",
        "/bkt/a/b%20c.txt",
        &[("Range", "bytes=6-")],
        b"",
    )
    .await;
    assert_eq!((part.status, part.text().as_str()), (206, "world"));
    assert_eq!(part.header("Content-Range"), Some("bytes 6-10/11"));
    let head = call(&srv, "HEAD", "/bkt/a/b%20c.txt", &[], b"").await;
    assert_eq!(
        (head.status, head.header("Content-Length"), head.body.len()),
        (200, Some("11"), 0)
    );
    let list = call(
        &srv,
        "GET",
        "/bkt?list-type=2&prefix=a%2F&delimiter=%2F&encoding-type=url",
        &[],
        b"",
    )
    .await;
    assert!(
        list.text().contains("<Key>a%2Fb%20c.txt</Key>"),
        "{}",
        list.text()
    );
    let missing = call(&srv, "GET", "/bkt/none", &[], b"").await;
    assert_eq!(missing.status, 404);
    assert!(missing.text().contains("<Code>NoSuchKey</Code>"));
    assert_eq!(
        call(&srv, "HEAD", "/bkt/none", &[], b"").await.body.len(),
        0
    );
    assert_eq!(
        call(&srv, "DELETE", "/bkt/a/b%20c.txt", &[], b"")
            .await
            .status,
        204
    );
    srv.stop().await;
}

#[tokio::test]
async fn configuration_is_kept_as_sent_and_what_is_not_built_is_refused_by_name() {
    let dir = temp("config");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    let cors = "<CORSConfiguration><CORSRule><AllowedMethod>GET</AllowedMethod><AllowedOrigin>https://example.com</AllowedOrigin><MaxAgeSeconds>300</MaxAgeSeconds></CORSRule></CORSConfiguration>";
    assert_eq!(
        call(&srv, "PUT", "/bkt?cors", &[], cors.as_bytes())
            .await
            .status,
        200
    );
    let got = call(&srv, "GET", "/bkt?cors", &[], b"").await.text();
    assert!(
        got.contains("<CORSConfiguration xmlns=")
            && got.contains("<AllowedMethod>GET</AllowedMethod>")
            && got.contains("<MaxAgeSeconds>300</MaxAgeSeconds>"),
        "{got}"
    );
    assert_eq!(
        call(&srv, "DELETE", "/bkt?cors", &[], b"").await.status,
        204
    );
    assert_eq!(call(&srv, "GET", "/bkt?cors", &[], b"").await.status, 404);

    let inv = "<InventoryConfiguration><Id>weekly</Id><IsEnabled>true</IsEnabled><Destination><S3BucketDestination><Bucket>arn:aws:s3:::reports</Bucket><Format>CSV</Format></S3BucketDestination></Destination><IncludedObjectVersions>All</IncludedObjectVersions><Schedule><Frequency>Weekly</Frequency></Schedule></InventoryConfiguration>";
    assert_eq!(
        call(&srv, "PUT", "/bkt?inventory&id=weekly", &[], inv.as_bytes())
            .await
            .status,
        200
    );
    let got = call(&srv, "GET", "/bkt?inventory&id=weekly", &[], b"")
        .await
        .text();
    assert!(got.contains("<Frequency>Weekly</Frequency>"), "{got}");
    assert!(
        call(&srv, "GET", "/bkt?inventory", &[], b"")
            .await
            .text()
            .contains("<InventoryConfiguration>")
    );

    for (method, path) in [("POST", "/bkt/k?restore"), ("GET", "/bkt/k?torrent")] {
        let r = call(&srv, method, path, &[], b"").await;
        assert_eq!(r.status, 501, "{method} {path}");
        assert_eq!(r.header("x-doze-not-built"), Some("1"));
        assert!(
            r.text().contains("<Code>NotImplemented</Code>"),
            "{}",
            r.text()
        );
    }
    srv.stop().await;
}

#[tokio::test]
async fn an_unsigned_request_is_served_as_the_local_account() {
    let dir = temp("anonymous");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    call(
        &srv,
        "PUT",
        "/bkt/page.html",
        &[("Content-Type", "text/html")],
        b"<p>hi</p>",
    )
    .await;
    // What curl sends: no Authorization header at all.
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    s.write_all(b"GET /bkt/page.html HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let r = parse(&raw);
    assert_eq!((r.status, r.text().as_str()), (200, "<p>hi</p>"));
    assert_eq!(r.header("Content-Type"), Some("text/html"));
    srv.stop().await;
}

#[tokio::test]
async fn a_virtual_host_names_the_bucket() {
    let dir = temp("vhost");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/photos", &[], b"").await;
    let host = format!("photos.s3.localhost:{}", srv.addr.port());
    assert_eq!(
        request(srv.addr, "PUT", &host, "/cat.jpg", &[], b"meow")
            .await
            .status,
        200
    );
    let path_style = call(&srv, "GET", "/photos/cat.jpg", &[], b"").await;
    assert_eq!(path_style.text(), "meow");
    assert_eq!(
        request(srv.addr, "GET", &host, "/cat.jpg", &[], b"")
            .await
            .text(),
        "meow"
    );
    srv.stop().await;
}

#[tokio::test]
async fn expect_continue_is_answered_before_the_body_is_sent() {
    let dir = temp("expect");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    let h = head(
        "PUT",
        "127.0.0.1",
        "/bkt/k",
        &[("Expect", "100-continue")],
        5,
    );
    s.write_all(h.as_bytes()).await.unwrap();
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut buf))
        .await
        .expect("100 Continue")
        .unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 100 Continue"));
    s.write_all(b"hello").await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    assert_eq!(parse(&raw).status, 200);
    assert_eq!(call(&srv, "GET", "/bkt/k", &[], b"").await.text(), "hello");
    srv.stop().await;
}

/// `body` in the aws-chunked encoding with an unsigned trailing checksum.
fn chunked(parts: &[&[u8]], trailer: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(format!("{:x}\r\n", p.len()).as_bytes());
        out.extend_from_slice(p);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("0\r\n{trailer}\r\n\r\n").as_bytes());
    out
}

#[tokio::test]
async fn a_chunked_upload_with_a_trailing_checksum_is_checked() {
    let dir = temp("trailer");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    let headers = [
        ("x-amz-content-sha256", "STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
        ("Content-Encoding", "aws-chunked"),
        ("x-amz-decoded-content-length", "11"),
        ("x-amz-trailer", "x-amz-checksum-crc32"),
        ("x-amz-sdk-checksum-algorithm", "CRC32"),
    ];
    // CRC32 of "hello world" is 0x0d4a1185, which is "DUoRhQ==" in base64.
    let good = chunked(&[b"hello ", b"world"], "x-amz-checksum-crc32:DUoRhQ==");
    let r = call(&srv, "PUT", "/bkt/good", &headers, &good).await;
    assert_eq!(
        (r.status, r.header("x-amz-checksum-crc32")),
        (200, Some("DUoRhQ==")),
        "{}",
        r.text()
    );
    assert_eq!(
        call(&srv, "GET", "/bkt/good", &[], b"").await.text(),
        "hello world"
    );
    let bad = chunked(&[b"hello ", b"world"], "x-amz-checksum-crc32:AAAAAA==");
    let r = call(&srv, "PUT", "/bkt/bad", &headers, &bad).await;
    assert_eq!(r.status, 400);
    assert!(r.text().contains("<Code>BadDigest</Code>"), "{}", r.text());
    assert_eq!(
        call(&srv, "GET", "/bkt/bad", &[], b"").await.status,
        404,
        "a body that failed its checksum is not kept"
    );
    srv.stop().await;
}

#[tokio::test]
async fn versions_multipart_and_a_restart() {
    let dir = temp("restart");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    let on = b"<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>";
    assert_eq!(
        call(&srv, "PUT", "/bkt?versioning", &[], on).await.status,
        200
    );
    let v1 = call(&srv, "PUT", "/bkt/k", &[], b"one").await;
    let v2 = call(&srv, "PUT", "/bkt/k", &[], b"two").await;
    let (id1, id2) = (
        v1.header("x-amz-version-id").unwrap().to_owned(),
        v2.header("x-amz-version-id").unwrap().to_owned(),
    );
    assert_ne!(id1, id2);
    let del = call(&srv, "DELETE", "/bkt/k", &[], b"").await;
    assert_eq!(del.header("x-amz-delete-marker"), Some("true"));
    assert_eq!(call(&srv, "GET", "/bkt/k", &[], b"").await.status, 404);
    assert_eq!(
        call(&srv, "GET", &format!("/bkt/k?versionId={id1}"), &[], b"")
            .await
            .text(),
        "one"
    );
    let started = call(&srv, "POST", "/bkt/big?uploads", &[], b"")
        .await
        .text();
    let upload = started
        .split("<UploadId>")
        .nth(1)
        .unwrap()
        .split("</UploadId>")
        .next()
        .unwrap()
        .to_owned();
    let five = vec![b'a'; 5 * 1024 * 1024];
    let p1 = call(
        &srv,
        "PUT",
        &format!("/bkt/big?partNumber=1&uploadId={upload}"),
        &[],
        &five,
    )
    .await;
    let p2 = call(
        &srv,
        "PUT",
        &format!("/bkt/big?partNumber=2&uploadId={upload}"),
        &[],
        b"tail",
    )
    .await;
    let body = format!(
        "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{}</ETag></Part><Part><PartNumber>2</PartNumber><ETag>{}</ETag></Part></CompleteMultipartUpload>",
        p1.header("ETag").unwrap(),
        p2.header("ETag").unwrap()
    );
    let done = call(
        &srv,
        "POST",
        &format!("/bkt/big?uploadId={upload}"),
        &[],
        body.as_bytes(),
    )
    .await;
    assert_eq!(done.status, 200, "{}", done.text());
    srv.stop().await;
    // Everything above is still there after the service has been stopped and started again.
    let srv = start(&dir).await;
    assert_eq!(
        call(&srv, "GET", &format!("/bkt/k?versionId={id2}"), &[], b"")
            .await
            .text(),
        "two"
    );
    let big = call(&srv, "GET", "/bkt/big", &[], b"").await;
    assert_eq!(big.body.len(), 5 * 1024 * 1024 + 4);
    assert!(
        big.header("ETag").unwrap().ends_with("-2\""),
        "{:?}",
        big.header("ETag")
    );
    let versions = call(&srv, "GET", "/bkt?versions", &[], b"").await.text();
    assert_eq!(versions.matches("<Version>").count(), 3, "{versions}");
    assert_eq!(versions.matches("<DeleteMarker>").count(), 1);
    srv.stop().await;
}
