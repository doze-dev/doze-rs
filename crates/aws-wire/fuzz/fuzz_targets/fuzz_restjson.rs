#![no_main]
//! Any bytes as a Lambda request body: the REST-JSON reader must not panic or hang, whatever the operation, and whatever it accepts
//! is read into the input of operations that take a JSON object or a raw payload.

use aws_model_lambda::tables::TABLES;
use aws_wire::rest::jsonin::to_json;
use aws_wire::rest::request::Parts;
use http::HeaderMap;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let headers = HeaderMap::new();
    for op in ["CreateFunction", "Invoke", "CreateAlias", "UpdateFunctionConfiguration", "CreateEventSourceMapping", "PutFunctionEventInvokeConfig", "AddPermission"] {
        if let Some(shape) = TABLES.find_op(op).and_then(|i| TABLES.op(i)).and_then(|o| o.input) {
            let p = Parts { labels: &[], query: &[], headers: &headers, body: data };
            let _ = to_json(&TABLES, shape, &p);
        }
    }
});
