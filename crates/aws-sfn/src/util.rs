//! Small things several handlers share.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// A version 4 UUID from the system's randomness (the clock when there is none).
pub(crate) fn uuid() -> String {
    let mut seed = [0u8; 8];
    if getrandom::fill(&mut seed).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        seed = (nanos as u64).to_le_bytes();
    }
    aws_asl::Env::new(0, u64::from_le_bytes(seed)).uuid()
}

/// Tags as the API lists them: `[{key, value}]`.
pub(crate) fn tag_list(tags: &BTreeMap<String, String>) -> Value {
    Value::Array(
        tags.iter()
            .map(|(k, v)| json!({"key": k, "value": v}))
            .collect(),
    )
}
