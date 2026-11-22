//! What the tests of a resource's own pages share: an operation as a page sends it.

use serde_json::Value;

use crate::world::World;

/// `POST …/<name>/op/<operation>` with `input`: the status and the answer.
pub(crate) async fn op(w: &World, base: &str, operation: &str, input: &Value) -> (u16, Value) {
    w.send("POST", &format!("{base}/op/{operation}"), input)
        .await
}

/// The same, which must succeed: its answer.
pub(crate) async fn ok(w: &World, base: &str, operation: &str, input: &Value) -> Value {
    let (status, answer) = op(w, base, operation, input).await;
    assert_eq!(status, 200, "{operation}: {answer}");
    answer
}

// Each test binary uses some of these.
#[allow(dead_code)]
/// Whether a list in an answer is absent or empty.
pub(crate) fn none(list: &Value) -> bool {
    list.as_array().is_none_or(Vec::is_empty)
}
