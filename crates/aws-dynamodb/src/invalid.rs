//! The one refusal DynamoDB words by quoting the request the way the Java SDK prints it: a `BatchWriteItem` with too many (or no) write
//! requests for a table. Recorded 2026-11-09 with 26 puts; the layout of a delete request is `(memory)`.

use aws_wire::ApiError;
use serde_json::Value as Json;

use crate::req::validation;

/// The most write requests one table may have in a `BatchWriteItem`, and the fewest.
const MAX: usize = 25;

/// `AttributeValue(s=k0, n=null, b=null, iNT=null, ...)`: Java's `toString` of the SDK's model, fields in declaration order.
fn attribute_value(v: &Json) -> String {
    const FIELDS: [&str; 16] = [
        "s",
        "n",
        "b",
        "iNT",
        "dECIMAL",
        "dOUBLE",
        "sS",
        "nS",
        "bS",
        "iNTSET",
        "dECIMALSET",
        "dOUBLESET",
        "m",
        "l",
        "nULL",
        "bOOL",
    ];
    let wire_tag = |field: &str| match field {
        "s" => "S",
        "n" => "N",
        "b" => "B",
        "sS" => "SS",
        "nS" => "NS",
        "bS" => "BS",
        "m" => "M",
        "l" => "L",
        "nULL" => "NULL",
        "bOOL" => "BOOL",
        _ => "",
    };
    let parts: Vec<String> = FIELDS
        .iter()
        .map(|field| {
            let shown = v
                .get(wire_tag(field))
                .map_or_else(|| "null".to_owned(), |inner| value(field, inner));
            format!("{field}={shown}")
        })
        .collect();
    format!("AttributeValue({})", parts.join(", "))
}

fn value(field: &str, inner: &Json) -> String {
    match (field, inner) {
        ("m", Json::Object(m)) => map(m),
        ("l", Json::Array(l)) => format!(
            "[{}]",
            l.iter().map(attribute_value).collect::<Vec<_>>().join(", ")
        ),
        ("sS" | "nS" | "bS", Json::Array(a)) => format!(
            "[{}]",
            a.iter()
                .filter_map(Json::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        (_, Json::String(s)) => s.clone(),
        (_, other) => other.to_string(),
    }
}

fn map(m: &serde_json::Map<String, Json>) -> String {
    format!(
        "{{{}}}",
        m.iter()
            .map(|(k, v)| format!("{k}={}", attribute_value(v)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn write_request(w: &Json) -> String {
    let put = w
        .get("PutRequest")
        .and_then(|p| p.get("Item"))
        .and_then(Json::as_object);
    let del = w
        .get("DeleteRequest")
        .and_then(|p| p.get("Key"))
        .and_then(Json::as_object);
    let put = put.map_or_else(
        || "null".to_owned(),
        |i| format!("PutRequest(item={}, workloadProfileName=null)", map(i)),
    );
    let del = del.map_or_else(
        || "null".to_owned(),
        |k| format!("DeleteRequest(key={}, workloadProfileName=null)", map(k)),
    );
    format!("WriteRequest(putRequest={put}, deleteRequest={del})")
}

/// The refusal for a batch write whose request list is too long or empty, or `None` if that is not what is wrong with `body`.
pub(crate) fn batch_write(body: &str) -> Option<ApiError> {
    let json: Json = serde_json::from_str(body).ok()?;
    let tables = json.get("RequestItems")?.as_object()?;
    if !tables
        .values()
        .any(|w| w.as_array().is_some_and(|a| a.len() > MAX || a.is_empty()))
    {
        return None;
    }
    let shown: Vec<String> = tables
        .iter()
        .map(|(name, w)| {
            let items: Vec<String> = w
                .as_array()
                .map_or_else(Vec::new, |a| a.iter().map(write_request).collect());
            format!("{name}=[{}]", items.join(", "))
        })
        .collect();
    Some(validation(format!(
        "1 validation error detected: Value '{{{}}}' at 'requestItems' failed to satisfy constraint: Map value must satisfy constraint: [Member must have length less than or equal to {MAX}, Member must have length greater than or equal to 1]",
        shown.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_string_item_is_printed_as_java_prints_it() {
        let v = attribute_value(&json!({"S": "k0"}));
        assert_eq!(
            v,
            "AttributeValue(s=k0, n=null, b=null, iNT=null, dECIMAL=null, dOUBLE=null, sS=null, nS=null, bS=null, iNTSET=null, dECIMALSET=null, dOUBLESET=null, m=null, l=null, nULL=null, bOOL=null)"
        );
    }

    #[test]
    fn only_a_list_of_the_wrong_length_is_this_error() {
        let ok: Vec<Json> = (0..25)
            .map(|i| json!({"PutRequest": {"Item": {"pk": {"S": i.to_string()}}}}))
            .collect();
        assert!(batch_write(&json!({"RequestItems": {"t": ok}}).to_string()).is_none());
        let long: Vec<Json> = (0..26)
            .map(|i| json!({"PutRequest": {"Item": {"pk": {"S": i.to_string()}}}}))
            .collect();
        let e = batch_write(&json!({"RequestItems": {"t": long}}).to_string()).unwrap();
        assert!(e.message.starts_with("1 validation error detected: Value '{t=[WriteRequest(putRequest=PutRequest(item={pk=AttributeValue(s=0,"));
        assert!(
            e.message
                .ends_with("Member must have length greater than or equal to 1]")
        );
    }
}
