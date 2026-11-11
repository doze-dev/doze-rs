#![no_main]
//! Any bytes as an `Authorization` header, and any text as a query string: parsed without a panic.

use aws_auth::claim::Claim;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Split at the first NUL: the header value before it, the query after.
    let (header, query) = data.split_at(data.iter().position(|b| *b == 0).unwrap_or(data.len()));
    let query = std::str::from_utf8(query.get(1..).unwrap_or(&[])).unwrap_or("");
    let uri = format!("/?{}", query.replace(['\r', '\n', ' ', '#'], ""));
    let Ok(uri) = uri.parse::<http::Uri>() else { return };
    let mut request = http::Request::builder().method("POST").uri(uri);
    if let Ok(value) = http::HeaderValue::from_bytes(header) {
        request = request.header("authorization", value).header("x-amz-date", "20130524T000000Z");
    }
    if let Ok(request) = request.body(()) {
        let (parts, ()) = request.into_parts();
        let _ = Claim::parse(&parts);
    }
});
