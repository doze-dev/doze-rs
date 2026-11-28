//! Big work stays out of the region's actor and off the event loop: a copy is made outside the actor, so another call of the same
//! region is answered while it runs; a copy of a source replaced meanwhile is refused; an upload's bytes and sums come out the same
//! in batches.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::{Duration, Instant};

use support::{call, start, temp};

const MIB: usize = 1024 * 1024;

/// Before: CopyObject read, summed and wrote the source inside the actor, and a HeadBucket of the same region waited for all of it.
/// After: the actor only plans the copy and keeps the result.
#[tokio::test]
async fn a_call_of_the_same_region_is_answered_while_a_copy_runs() {
    let dir = temp("offloop-copy");
    let srv = start(&dir).await;
    assert_eq!(call(&srv, "PUT", "/big", &[], b"").await.status, 200);
    let body: Vec<u8> = (0..48 * MIB).map(|i| (i % 251) as u8).collect();
    let put = call(&srv, "PUT", "/big/src", &[], &body).await;
    assert_eq!(put.status, 200, "{}", put.text());
    let start_at = Instant::now();
    let copy = async {
        let r = call(
            &srv,
            "PUT",
            "/big/dst",
            &[("x-amz-copy-source", "/big/src")],
            b"",
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.text());
        start_at.elapsed()
    };
    // Asked 20 ms in; counted from then, since on a held loop the timer itself fires late.
    let head = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(call(&srv, "HEAD", "/big", &[], b"").await.status, 200);
        start_at.elapsed().saturating_sub(Duration::from_millis(20))
    };
    let (copied, headed) = tokio::join!(copy, head);
    assert!(
        headed * 3 < copied,
        "HeadBucket waited {headed:?} for a copy of {copied:?}"
    );
    let got = call(&srv, "GET", "/big/dst", &[], b"").await;
    assert_eq!(got.body.len(), body.len());
    assert!(got.body == body, "the copy is the source");
    assert_eq!(got.header("etag"), put.header("etag"));
    srv.stop().await;
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn an_upload_in_many_batches_is_summed_and_written_whole() {
    use md5::Digest;
    let dir = temp("offloop-batches");
    let srv = start(&dir).await;
    assert_eq!(call(&srv, "PUT", "/batches", &[], b"").await.status, 200);
    // Not a multiple of a batch, nor of a network piece.
    let body: Vec<u8> = (0..(3 * MIB + 12_345))
        .map(|i| (i * 7 % 256) as u8)
        .collect();
    let md5 = aws_core::hex::encode(&md5::Md5::digest(&body));
    let sha = aws_core::base64::encode(&sha2::Sha256::digest(&body));
    // The server checks both against what it summed: a wrong one is refused.
    let headers = [
        (
            "Content-MD5",
            aws_core::base64::encode(&md5::Md5::digest(&body)),
        ),
        ("x-amz-checksum-sha256", sha),
    ];
    let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let put = call(&srv, "PUT", "/batches/k", &headers, &body).await;
    assert_eq!(put.status, 200, "{}", put.text());
    assert_eq!(put.header("etag"), Some(format!("\"{md5}\"").as_str()));
    let wrong = call(
        &srv,
        "PUT",
        "/batches/k",
        &[(
            "x-amz-checksum-sha256",
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
        )],
        &body,
    )
    .await;
    assert_eq!(wrong.status, 400, "{}", wrong.text());
    let got = call(&srv, "GET", "/batches/k", &[], b"").await;
    assert!(got.body == body);
    assert_eq!(put.header("etag"), got.header("etag"));
    srv.stop().await;
    std::fs::remove_dir_all(&dir).ok();
}
