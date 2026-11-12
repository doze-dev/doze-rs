#![no_main]
//! Any bytes as an S3 request body: the XML reader must not panic or hang, and whatever it accepts is read into the input of the
//! operations that take an XML payload without a panic either.

use aws_model_s3::tables::TABLES;
use aws_wire::rest::request::{Parts, to_json};
use aws_wire::rest::xmlin::parse;
use http::HeaderMap;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse(data);
    let headers = HeaderMap::new();
    for op in ["DeleteObjects", "CompleteMultipartUpload", "PutBucketCors", "PutBucketLifecycleConfiguration", "PutBucketNotificationConfiguration"] {
        if let Some(shape) = TABLES.find_op(op).and_then(|i| TABLES.op(i)).and_then(|o| o.input) {
            let p = Parts { labels: &[], query: &[], headers: &headers, body: data };
            let _ = to_json(&TABLES, shape, &p);
        }
    }
});
