//! Two header rules S3 holds that the model does not state, over a real socket: `x-amz-checksum-mode` in any case (the AWS CLI's
//! downloads send `enabled`), and `aws-chunked` never kept as an object's `Content-Encoding`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod support;
use support::{call, start, temp};

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
async fn a_lower_case_checksum_mode_is_taken_as_s3_takes_it() {
    let dir = temp("checksum-mode");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    let put = call(
        &srv,
        "PUT",
        "/bkt/k",
        &[("x-amz-checksum-crc32", "DUoRhQ==")],
        b"hello world",
    )
    .await;
    assert_eq!(put.status, 200, "{}", put.text());
    // The AWS CLI's download path sends `x-amz-checksum-mode: enabled`; the model's spelling is `ENABLED`.
    for mode in ["enabled", "ENABLED", "Enabled"] {
        let get = call(&srv, "GET", "/bkt/k", &[("x-amz-checksum-mode", mode)], b"").await;
        assert_eq!(
            (get.status, get.text().as_str()),
            (200, "hello world"),
            "{mode}"
        );
        assert_eq!(
            get.header("x-amz-checksum-crc32"),
            Some("DUoRhQ=="),
            "{mode}"
        );
        let head = call(
            &srv,
            "HEAD",
            "/bkt/k",
            &[("x-amz-checksum-mode", mode)],
            b"",
        )
        .await;
        assert_eq!(head.status, 200, "{mode}");
        assert_eq!(
            head.header("x-amz-checksum-crc32"),
            Some("DUoRhQ=="),
            "{mode}"
        );
    }
    // Any other value is still refused, and so is any other enum's wrong case.
    let bad = call(
        &srv,
        "GET",
        "/bkt/k",
        &[("x-amz-checksum-mode", "sometimes")],
        b"",
    )
    .await;
    assert_eq!(bad.status, 400);
    assert!(bad.text().contains("InvalidArgument"), "{}", bad.text());
    let payer = call(
        &srv,
        "GET",
        "/bkt/k",
        &[("x-amz-request-payer", "REQUESTER")],
        b"",
    )
    .await;
    assert_eq!(
        payer.status,
        400,
        "request-payer stays exact: {}",
        payer.text()
    );
    srv.stop().await;
}

#[tokio::test]
async fn a_content_encoding_keeps_its_encodings_but_not_aws_chunked() {
    let dir = temp("encoding");
    let srv = start(&dir).await;
    call(&srv, "PUT", "/bkt", &[], b"").await;
    let framed = chunked(&[b"hello world"], "x-amz-checksum-crc32:DUoRhQ==");
    for (key, sent, kept) in [
        ("a", "aws-chunked", None),
        ("b", "aws-chunked, gzip", Some("gzip")),
        ("c", "gzip", Some("gzip")),
    ] {
        let mut headers = vec![("Content-Encoding", sent)];
        let body: &[u8] = if sent.contains("aws-chunked") {
            headers.extend([
                ("x-amz-content-sha256", "STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
                ("x-amz-decoded-content-length", "11"),
                ("x-amz-trailer", "x-amz-checksum-crc32"),
            ]);
            &framed
        } else {
            b"hello world"
        };
        let put = call(&srv, "PUT", &format!("/bkt/{key}"), &headers, body).await;
        assert_eq!(put.status, 200, "{}", put.text());
        let head = call(&srv, "HEAD", &format!("/bkt/{key}"), &[], b"").await;
        assert_eq!(head.header("Content-Encoding"), kept, "{sent}");
    }
    srv.stop().await;
}
