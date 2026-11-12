//! Parameter policies: a JSON array of `{Type, Version, Attributes}`. Only advanced parameters may carry them. `Expiration`
//! removes the parameter at its timestamp; the two notification kinds are kept and returned, and nothing is sent.

use aws_wire::ApiError;
use serde_json::Value;

use crate::model::Policy;

/// The policy types AWS has.
const KINDS: [&str; 3] = [
    "Expiration",
    "ExpirationNotification",
    "NoChangeNotification",
];

/// What a policies document says: the policies, and when the parameter expires.
pub(crate) struct Parsed {
    pub(crate) policies: Vec<Policy>,
    pub(crate) expires_ms: Option<i64>,
}

// fuzz:parser fuzz_policies
/// Reads a policies document.
pub(crate) fn parse(text: &str) -> Result<Parsed, ApiError> {
    let bad = |message: String| ApiError::modeled("InvalidPolicyTypeException", message);
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else {
        return Err(bad(
            "Invalid policies: the policies must be a JSON array.".into()
        ));
    };
    let mut policies = Vec::new();
    let mut expires_ms = None;
    for item in &list {
        let kind = item.get("Type").and_then(Value::as_str).unwrap_or_default();
        if !KINDS.contains(&kind) {
            return Err(bad(format!(
                "Invalid policy type: {kind}. The types are {}.",
                KINDS.join(", ")
            )));
        }
        if kind == "Expiration" {
            let stamp = item
                .pointer("/Attributes/Timestamp")
                .and_then(Value::as_str)
                .and_then(aws_core::time::parse_rfc3339);
            let Some(secs) = stamp else {
                return Err(ApiError::modeled(
                    "InvalidPolicyAttributeException",
                    "Invalid policy attribute: the Expiration policy needs a Timestamp in ISO 8601 form, such as 2030-01-01T00:00:00.000Z.",
                ));
            };
            expires_ms = Some(secs.saturating_mul(1000));
        }
        policies.push(Policy {
            text: item.to_string(),
            kind: kind.to_owned(),
        });
    }
    Ok(Parsed {
        policies,
        expires_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expiration_sets_the_time() {
        let parsed = parse(r#"[{"Type":"Expiration","Version":"1.0","Attributes":{"Timestamp":"2030-01-01T00:00:00.000Z"}}]"#).ok();
        assert_eq!(parsed.and_then(|p| p.expires_ms), Some(1_893_456_000_000));
    }

    #[test]
    fn nonsense_is_refused() {
        assert!(parse("{}").is_err());
        assert!(parse(r#"[{"Type":"Whatever"}]"#).is_err());
        assert!(parse(r#"[{"Type":"Expiration","Attributes":{"Timestamp":"soon"}}]"#).is_err());
    }
}
