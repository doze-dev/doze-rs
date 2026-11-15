//! The refusal for a `BatchWriteItem` with too many (or no) write requests for a table. Recorded 2026-11-15 with 26 puts:
//! `1 validation error detected: Value at 'RequestItems.<table>.member' failed to satisfy constraint: Member must have length less
//! than or equal to 25`. (A recording of 2026-11-09 quoted the whole request as the Java SDK prints it; AWS has since stopped.) The
//! empty list's wording and the count when several tables are wrong are `(memory)`, in the same form.

use aws_wire::ApiError;
use serde_json::Value as Json;

use crate::req::validation;

/// The most write requests one table may have in a `BatchWriteItem`.
const MAX: usize = 25;

/// The refusal for a batch write whose request list is too long or empty, or `None` if that is not what is wrong with `body`.
pub(crate) fn batch_write(body: &str) -> Option<ApiError> {
    let json: Json = serde_json::from_str(body).ok()?;
    let tables = json.get("RequestItems")?.as_object()?;
    let wrong: Vec<String> = tables
        .iter()
        .filter_map(|(name, w)| {
            let n = w.as_array()?.len();
            let constraint = if n > MAX {
                format!("Member must have length less than or equal to {MAX}")
            } else if n == 0 {
                "Member must have length greater than or equal to 1".to_owned()
            } else {
                return None;
            };
            Some(format!(
                "Value at 'RequestItems.{name}.member' failed to satisfy constraint: {constraint}"
            ))
        })
        .collect();
    let count = match wrong.len() {
        0 => return None,
        1 => "1 validation error detected".to_owned(),
        n => format!("{n} validation errors detected"),
    };
    Some(validation(format!("{count}: {}", wrong.join("; "))))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn puts(n: usize) -> Vec<Json> {
        (0..n)
            .map(|i| json!({"PutRequest": {"Item": {"pk": {"S": i.to_string()}}}}))
            .collect()
    }

    #[test]
    fn only_a_list_of_the_wrong_length_is_this_error() {
        assert!(batch_write(&json!({"RequestItems": {"t": puts(25)}}).to_string()).is_none());
        let e = batch_write(&json!({"RequestItems": {"t": puts(26)}}).to_string()).unwrap();
        assert_eq!(
            e.message,
            "1 validation error detected: Value at 'RequestItems.t.member' failed to satisfy constraint: Member must have length less than or equal to 25"
        );
    }

    #[test]
    fn an_empty_list_is_refused_too() {
        let e = batch_write(&json!({"RequestItems": {"t": []}}).to_string()).unwrap();
        assert!(
            e.message
                .ends_with("Member must have length greater than or equal to 1")
        );
    }

    #[test]
    fn each_wrong_table_is_counted() {
        let e =
            batch_write(&json!({"RequestItems": {"a": [], "b": puts(26)}}).to_string()).unwrap();
        assert!(e.message.starts_with("2 validation errors detected: "));
    }
}
