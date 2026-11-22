//! A header-bound enum value a service reads without regard to case. The model gives `ChecksumMode` the one value `ENABLED`, and
//! the AWS CLI's download path (the CRT transfer client) sends it as `enabled`; S3 takes both. The decoder compares an enum
//! exactly, so the gateway rewrites such a value to the model's spelling first, and only for the headers a service names.

use aws_schema::tables::{K_STR, Tables};
use serde_json::Value;

/// Rewrites, in the input `value` of the operation whose input structure is `shape`, the value of each member bound to one of
/// `headers` (lower-case names) to the spelling the model gives it, when it differs only in case. A value that matches no
/// spelling is left as it is, for the decoder to refuse.
pub fn fold_enum_headers(tables: &Tables, shape: u32, headers: &[&str], value: &mut Value) {
    let (Some(sh), Some(map)) = (tables.shape(shape), value.as_object_mut()) else {
        return;
    };
    for m in (sh.first..sh.first + sh.count).filter_map(|i| tables.member(i)) {
        let named = m.bind == 4
            && m.kind == K_STR
            && m.bind_str
                .is_some_and(|n| headers.iter().any(|h| h.eq_ignore_ascii_case(n)));
        let Some(set) = named
            .then(|| tables.cons(m.cons))
            .flatten()
            .map(|c| c.enum_set)
        else {
            continue;
        };
        let Some(Value::String(text)) = map.get_mut(m.wire) else {
            continue;
        };
        if let Some(exact) = tables
            .enum_values(set)
            .into_iter()
            .find(|v| v.eq_ignore_ascii_case(text))
        {
            exact.clone_into(text);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_s3::tables::TABLES;
    use serde_json::json;

    use super::*;

    fn input(op: &str) -> u32 {
        TABLES
            .op(TABLES.find_op(op).unwrap())
            .unwrap()
            .input
            .unwrap()
    }

    #[test]
    fn a_named_header_enum_takes_the_models_spelling() {
        for op in ["GetObject", "HeadObject"] {
            let mut v = json!({"Bucket": "b", "ChecksumMode": "enabled"});
            fold_enum_headers(&TABLES, input(op), &["x-amz-checksum-mode"], &mut v);
            assert_eq!(v["ChecksumMode"], "ENABLED", "{op}");
        }
    }

    #[test]
    fn other_enums_and_other_values_are_left_alone() {
        let mut v = json!({"ChecksumMode": "sometimes", "RequestPayer": "REQUESTER"});
        fold_enum_headers(
            &TABLES,
            input("GetObject"),
            &["x-amz-checksum-mode"],
            &mut v,
        );
        assert_eq!(v["ChecksumMode"], "sometimes");
        assert_eq!(v["RequestPayer"], "REQUESTER", "not a named header");
        let mut w = json!({"ChecksumMode": "enabled"});
        fold_enum_headers(&TABLES, input("GetObject"), &[], &mut w);
        assert_eq!(w["ChecksumMode"], "enabled");
    }
}
