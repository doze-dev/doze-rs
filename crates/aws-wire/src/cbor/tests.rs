#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use aws_model_sqs::meta::META;
use aws_model_sqs::tables::TABLES;

use super::*;

fn json(hex: &str) -> Result<String, CborError> {
    let bytes = aws_core::hex::decode(hex).unwrap();
    to_json(&bytes)
}

#[test]
fn the_examples_of_rfc_8949_appendix_a_read_as_the_json_they_stand_for() {
    for (hex, want) in [
        ("00", "0"),
        ("17", "23"),
        ("1818", "24"),
        ("1903e8", "1000"),
        ("1bffffffffffffffff", "18446744073709551615"),
        ("20", "-1"),
        ("3863", "-100"),
        ("3bffffffffffffffff", "-18446744073709551616"),
        ("f4", "false"),
        ("f5", "true"),
        ("f6", "null"),
        ("f7", "null"),
        ("fb3ff199999999999a", "1.1"),
        ("f93c00", "1.0"),
        ("f97bff", "65504.0"),
        ("fa47c35000", "100000.0"),
        ("f97c00", "\"Infinity\""),
        ("f9fc00", "\"-Infinity\""),
        ("f97e00", "\"NaN\""),
        ("60", "\"\""),
        ("6161", "\"a\""),
        ("62225c", "\"\\\"\\\\\""),
        ("62c3bc", "\"\u{fc}\""),
        ("4401020304", "\"AQIDBA==\""),
        ("80", "[]"),
        ("83010203", "[1,2,3]"),
        ("8301820203820405", "[1,[2,3],[4,5]]"),
        ("a0", "{}"),
        ("a26161016162820203", "{\"a\":1,\"b\":[2,3]}"),
        // Indefinite lengths.
        ("9fff", "[]"),
        ("9f018202039f0405ffff", "[1,[2,3],[4,5]]"),
        ("bf61610161629f0203ffff", "{\"a\":1,\"b\":[2,3]}"),
        ("7f657374726561646d696e67ff", "\"streaming\""),
        // A tag only says how to read what follows: tag 1 is a timestamp, which the JSON protocol carries as the number.
        ("c11a514b67b0", "1363896240"),
        ("c1fb41d452d9ec200000", "1363896240.5"),
    ] {
        assert_eq!(json(hex).unwrap(), want, "{hex}");
    }
}

#[test]
fn an_empty_body_is_an_empty_object() {
    assert_eq!(to_json(&[]).unwrap(), "{}");
}

#[test]
fn what_is_not_one_item_is_refused() {
    assert_eq!(json("18").unwrap_err(), CborError::Truncated);
    assert_eq!(json("6361").unwrap_err(), CborError::Truncated);
    // Bytes after the one item.
    assert_eq!(json("820102ff").unwrap_err(), CborError::Trailing);
    // A map key that is not text.
    assert_eq!(json("a201020304").unwrap_err(), CborError::Unsupported);
    // A text string that is not UTF-8.
    assert_eq!(json("62c328").unwrap_err(), CborError::Utf8);
    // A break outside an indefinite item, and the reserved additional information.
    assert_eq!(json("ff").unwrap_err(), CborError::Unsupported);
    assert_eq!(json("1c").unwrap_err(), CborError::Unsupported);
    // An array that claims more than the body holds does not allocate for it.
    assert_eq!(
        json("9bffffffffffffffff01").unwrap_err(),
        CborError::Truncated
    );
}

#[test]
fn nesting_is_bounded() {
    let deep = "81".repeat(200) + "00";
    assert_eq!(json(&deep).unwrap_err(), CborError::TooDeep);
    let ok = "81".repeat(50) + "00";
    assert!(json(&ok).is_ok());
}

fn parts(path: &str, protocol: Option<&str>) -> Parts {
    let mut b = http::Request::builder().method("POST").uri(path);
    if let Some(p) = protocol {
        b = b.header("smithy-protocol", p);
    }
    b.body(()).unwrap().into_parts().0
}

#[test]
fn a_request_is_told_by_its_header_and_routed_by_its_path() {
    assert!(is_request(&parts("/", Some("rpc-v2-cbor"))));
    assert!(!is_request(&parts("/", None)));
    let p = parts(
        "/service/AmazonSQS/operation/SendMessage",
        Some("rpc-v2-cbor"),
    );
    assert_eq!(
        TABLES.op(route(&META, &TABLES, &p).unwrap()).unwrap().name,
        "SendMessage"
    );
    for bad in [
        "/",
        "/service/AmazonSQS",
        "/service/Other/operation/SendMessage",
        "/service/AmazonSQS/operation/NoSuchThing",
    ] {
        let e = route(&META, &TABLES, &parts(bad, Some("rpc-v2-cbor"))).unwrap_err();
        assert_eq!(
            (e.status, e.code.as_ref()),
            (404, "UnknownOperationException"),
            "{bad}"
        );
    }
}

#[test]
fn an_error_is_a_cbor_map_with_the_type_and_the_message() {
    let id = RequestId::new();
    let err = ApiError::modeled("QueueDoesNotExist", "gone");
    let r = failure(&META, &TABLES, &id, &err, false);
    assert_eq!(r.status(), 400);
    assert_eq!(r.headers()["content-type"], "application/cbor");
    assert_eq!(r.headers()["smithy-protocol"], "rpc-v2-cbor");
    assert_eq!(
        r.headers()["x-amzn-query-error"],
        "AWS.SimpleQueueService.NonExistentQueue;Sender"
    );
    assert_eq!(
        to_json(r.body()).unwrap(),
        r#"{"__type":"com.amazonaws.sqs#QueueDoesNotExist","message":"gone"}"#
    );
    let nope = failure(
        &META,
        &TABLES,
        &id,
        &ApiError::new(501, "NotImplemented", "x"),
        false,
    );
    assert_eq!(nope.headers()["x-doze-not-built"], "1");
}
