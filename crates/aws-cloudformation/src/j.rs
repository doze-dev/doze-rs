//! Reading and writing members of JSON values without the indexing that panics on a value of the wrong kind.

use serde_json::Value;

/// A member of a JSON object.
pub(crate) trait J {
    /// The member `k`, or null.
    fn at(&self, k: &str) -> &Value;
    /// Sets member `k` (a value that is not an object is left as it is).
    fn put(&mut self, k: impl AsRef<str>, v: Value);
}

impl J for Value {
    fn at(&self, k: &str) -> &Value {
        self.get(k).unwrap_or(&Value::Null)
    }

    fn put(&mut self, k: impl AsRef<str>, v: Value) {
        if let Some(o) = self.as_object_mut() {
            o.insert(k.as_ref().to_owned(), v);
        }
    }
}
