//! Known-answer tests: the worked SigV4 examples from the public S3 documentation
//! ("Signature Calculations for the Authorization Header", "Authenticating
//! Requests: Using Query Parameters", "Signature Calculations for streaming
//! uploads"). The signatures below are reproduced from the AWS documentation,
//! written down from memory of those pages (no network access to re-check them);
//! an implementation that matches all of them cannot be matching by accident.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use crate::chunked::{ChunkAuth, ChunkDecoder, ChunkError, Event};
use crate::claim::Claim;
use crate::error::AuthError;
use crate::key::SigningKey;
use crate::verify::check_signature;
use bytes::Bytes;
use http::request::Parts;

const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn parts(method: &str, uri: &str, headers: &[(&str, &str)]) -> Parts {
    let mut b = http::Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(()).unwrap().into_parts().0
}

fn key() -> SigningKey {
    SigningKey::derive(SECRET, "20130524", "us-east-1", "s3").unwrap()
}

fn auth_header(signed: &str, sig: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders={signed},Signature={sig}"
    )
}

fn check(p: &Parts, payload_hash: &str) -> Result<String, AuthError> {
    let claim = Claim::parse(p).unwrap().unwrap();
    check_signature(p, &claim, &key(), payload_hash, true)
}

#[test]
fn get_object_with_range() {
    let a = auth_header(
        "host;range;x-amz-content-sha256;x-amz-date",
        "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41",
    );
    let p = parts(
        "GET",
        "/test.txt",
        &[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", "20130524T000000Z"),
            ("authorization", &a),
        ],
    );
    check(&p, EMPTY).unwrap();
}

#[test]
fn put_object_path_with_dollar() {
    let a = auth_header(
        "date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class",
        "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd",
    );
    let h = "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072";
    let p = parts(
        "PUT",
        "/test%24file.text",
        &[
            ("date", "Fri, 24 May 2013 00:00:00 GMT"),
            ("host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", h),
            ("x-amz-date", "20130524T000000Z"),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ("authorization", &a),
        ],
    );
    check(&p, h).unwrap();
}

#[test]
fn get_bucket_lifecycle_valueless_query_param() {
    let a = auth_header(
        "host;x-amz-content-sha256;x-amz-date",
        "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543",
    );
    let p = parts(
        "GET",
        "/?lifecycle",
        &[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", "20130524T000000Z"),
            ("authorization", &a),
        ],
    );
    check(&p, EMPTY).unwrap();
}

#[test]
fn list_objects_sorted_query() {
    let a = auth_header(
        "host;x-amz-content-sha256;x-amz-date",
        "34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7",
    );
    let p = parts(
        "GET",
        "/?prefix=J&max-keys=2",
        &[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", "20130524T000000Z"),
            ("authorization", &a),
        ],
    );
    check(&p, EMPTY).unwrap();
}

#[test]
fn presigned_url() {
    let p = parts(
        "GET",
        "/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host&X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404",
        &[("host", "examplebucket.s3.amazonaws.com")],
    );
    check(&p, "UNSIGNED-PAYLOAD").unwrap();
}

#[test]
fn wrong_signature_reports_both_strings() {
    let a = auth_header("host;x-amz-content-sha256;x-amz-date", &"0".repeat(64));
    let p = parts(
        "GET",
        "/?lifecycle",
        &[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", "20130524T000000Z"),
            ("authorization", &a),
        ],
    );
    match check(&p, EMPTY) {
        Err(AuthError::SignatureMismatch {
            canonical_request,
            string_to_sign,
            ..
        }) => {
            assert!(canonical_request.starts_with("GET\n/\nlifecycle=\n"));
            assert!(string_to_sign.starts_with(
                "AWS4-HMAC-SHA256\n20130524T000000Z\n20130524/us-east-1/s3/aws4_request\n"
            ));
        }
        other => panic!("expected mismatch, got {other:?}"),
    }
}

/// The streaming-upload example: a 64 KiB + 1 KiB object sent as three signed chunks.
#[test]
fn streaming_signed_chunks_from_the_s3_docs() {
    let seed = "4f232c4386841ef735655705268965c44a0e4690baa4adea153f7db9fa80a0a9";
    let a = auth_header(
        "content-encoding;content-length;host;x-amz-content-sha256;x-amz-date;x-amz-decoded-content-length;x-amz-storage-class",
        seed,
    );
    let p = parts(
        "PUT",
        "/examplebucket/chunkObject.txt",
        &[
            ("host", "s3.amazonaws.com"),
            ("x-amz-date", "20130524T000000Z"),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ("content-encoding", "aws-chunked"),
            ("x-amz-decoded-content-length", "66560"),
            ("content-length", "66824"),
            ("x-amz-content-sha256", "STREAMING-AWS4-HMAC-SHA256-PAYLOAD"),
            ("authorization", &a),
        ],
    );
    let scope = check(&p, "STREAMING-AWS4-HMAC-SHA256-PAYLOAD").unwrap();
    assert_eq!(scope, "20130524/us-east-1/s3/aws4_request");

    let mut wire = Vec::new();
    wire.extend_from_slice(b"10000;chunk-signature=ad80c730a21e5b8d04586a2213dd63b9a0e99e0e2307b0ade35a65485a288648\r\n");
    wire.extend(std::iter::repeat_n(b'a', 65536));
    wire.extend_from_slice(b"\r\n400;chunk-signature=0055627c9e194cb4542bae2aa5492e3c1575bbb81b612b7d234b86a503ef5497\r\n");
    wire.extend(std::iter::repeat_n(b'a', 1024));
    wire.extend_from_slice(b"\r\n0;chunk-signature=b6c6ea8a5354eaf15b3cb7646744f4275b71ea724fed81ceb9323e279d449df9\r\n\r\n");

    let run = |wire: &[u8], piece: usize| -> Result<usize, ChunkError> {
        let mut dec = ChunkDecoder::new(
            Some(ChunkAuth {
                key: key(),
                amz_date: "20130524T000000Z".into(),
                scope: scope.clone(),
                prev_signature: seed.into(),
            }),
            false,
        );
        let mut total = 0;
        for part in wire.chunks(piece) {
            let mut input = Bytes::copy_from_slice(part);
            loop {
                match dec.next(&mut input)? {
                    Event::Data(d) => total += d.len(),
                    Event::Need => break,
                    Event::Done => return Ok(total),
                }
            }
        }
        dec.finish_at_eof()?;
        Ok(total)
    };
    // Arbitrary network framing must not matter.
    for piece in [wire.len(), 8192, 100, 7, 1] {
        assert_eq!(run(&wire, piece).unwrap(), 66560, "piece size {piece}");
    }
    // Flip one payload byte: the chunk signature must fail.
    let mut bad = wire.clone();
    if let Some(b) = bad.get_mut(200) {
        *b = b'b';
    }
    assert!(matches!(run(&bad, 4096), Err(ChunkError::BadSignature)));
}
