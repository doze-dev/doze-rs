#![no_main]
//! Any text as a Query form body, read into the input of a few IAM operations: no panic, and no hang on a key like `A.member.99999999`.

use aws_model_iam::tables::TABLES;
use aws_wire::query::{parse_form, to_json};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let form = parse_form(text);
    for op in ["CreateRole", "TagRole", "CreatePolicy", "UpdateAccountPasswordPolicy"] {
        if let Some(shape) = TABLES.find_op(op).and_then(|i| TABLES.op(i)).and_then(|o| o.input) {
            let _ = to_json(&TABLES, shape, &form);
        }
    }
});
