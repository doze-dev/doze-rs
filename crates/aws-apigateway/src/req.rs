//! A request as the operations read it (the JSON the gateway built from the path, query, headers and body; members by their wire
//! names, `restApiId`, `name`, and for v2 path labels the member name, `ApiId`), who is calling, and what an operation answers.

use aws_wire::ApiError;
use aws_wire::service::{Call, Caller};
use serde_json::Value;

/// One decoded request.
#[derive(Debug)]
pub(crate) struct Req {
    pub(crate) body: Value,
    pub(crate) caller: Caller,
    /// The time of the request, epoch milliseconds.
    pub(crate) now_ms: i64,
}

impl Req {
    pub(crate) fn new(call: &Call, now_ms: i64) -> Self {
        Self {
            body: serde_json::from_str(&call.body).unwrap_or(Value::Null),
            // WHY: the call is consumed by the actor; the caller travels with the parsed request.
            caller: call.caller.clone(),
            now_ms,
        }
    }

    pub(crate) fn region(&self) -> &str {
        &self.caller.region
    }

    pub(crate) fn account(&self) -> &str {
        &self.caller.account_id
    }

    /// A string member, or `""`.
    pub(crate) fn s(&self, key: &str) -> &str {
        self.opt(key).unwrap_or_default()
    }

    pub(crate) fn opt(&self, key: &str) -> Option<&str> {
        self.body.get(key).and_then(Value::as_str)
    }

    pub(crate) fn flag(&self, key: &str) -> Option<bool> {
        self.body.get(key).and_then(Value::as_bool)
    }

    pub(crate) fn int(&self, key: &str) -> Option<i64> {
        self.body.get(key).and_then(Value::as_i64)
    }

    pub(crate) fn value(&self, key: &str) -> Option<&Value> {
        self.body.get(key).filter(|v| !v.is_null())
    }

    /// Epoch seconds, as v1 writes a timestamp, to the millisecond (two things made in one second are told apart, as AWS's slower
    /// calls are).
    pub(crate) fn now_secs(&self) -> f64 {
        self.now_ms as f64 / 1000.0
    }

    /// ISO 8601, as v2 writes a timestamp.
    pub(crate) fn now_iso(&self) -> String {
        let whole = aws_core::time::format_iso8601(self.now_ms.div_euclid(1000));
        whole.replace(".000Z", &format!(".{:03}Z", self.now_ms.rem_euclid(1000)))
    }

    /// The patch operations of an `Update*`.
    pub(crate) fn patches(&self) -> Vec<crate::patch::Patch> {
        crate::patch::of(self.value("patchOperations"))
    }
}

/// What an operation answers: its output as JSON, and a status when it is not the model's.
#[derive(Debug, Default)]
pub(crate) struct Out {
    pub(crate) json: Value,
    pub(crate) status: Option<u16>,
}

impl From<Value> for Out {
    fn from(json: Value) -> Self {
        Self { json, status: None }
    }
}

/// An operation's result.
pub(crate) type Res = Result<Out, ApiError>;

/// An answer with nothing in it.
pub(crate) fn none() -> Res {
    Ok(Out::default())
}

/// A random id of `n` characters from `[a-z0-9]`, as API Gateway's ids are.
pub(crate) fn new_id(n: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = vec![0u8; n];
    // WHY: with no randomness from the system the zeros give a valid (if poor) id; uniqueness is checked by the caller where it matters.
    if getrandom::fill(&mut bytes).is_err() {
        bytes.fill(0);
    }
    bytes
        .iter()
        .map(|b| {
            ALPHABET
                .get(usize::from(*b) % ALPHABET.len())
                .copied()
                .map_or('a', char::from)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_lower_case_letters_and_digits() {
        let id = new_id(10);
        assert_eq!(id.len(), 10);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        );
        assert_ne!(id, new_id(10));
    }
}
