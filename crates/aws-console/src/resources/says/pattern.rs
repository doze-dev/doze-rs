//! An event pattern and a rule's targets said in words, for a rule's sentence: `matches events from larder.orders with detail type
//! OrderPlaced and a total_cents above 0, and publishes it to the orders-created topic`.

use serde_json::Value;

use super::super::page::{list, plural};
use super::super::path;

/// `above`, `at least`...: what a numeric condition's operator says.
fn compare(op: &str) -> Option<&'static str> {
    Some(match op {
        ">" => "above",
        ">=" => "at least",
        "<" => "below",
        "<=" => "at most",
        "=" => "equal to",
        _ => return None,
    })
}

/// The numeric conditions on the top-level fields of `detail`: `a total_cents above 0`.
fn numeric(detail: Option<&Value>) -> Vec<String> {
    let Some(fields) = detail.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, rules) in fields {
        for rule in rules.as_array().into_iter().flatten() {
            let Some(cond) = rule.get("numeric").and_then(Value::as_array) else {
                continue;
            };
            if let [Value::String(op), Value::Number(n), ..] = cond.as_slice()
                && let Some(word) = compare(op)
            {
                out.push(format!("a {key} {word} {n}"));
            }
        }
    }
    out
}

/// What a pattern asks for, for the fields people name (`source`, `detail-type`, numbers in `detail`): `from larder.orders with
/// detail type OrderPlaced and a total_cents above 0`. `None` when it is not JSON or names none of them.
#[must_use]
pub fn pattern_words(pattern: &str) -> Option<String> {
    let v: Value = serde_json::from_str(pattern).ok()?;
    let o = v.as_object()?;
    let strings = |k: &str| -> Vec<String> {
        o.get(k)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut parts = Vec::new();
    let source = strings("source");
    if !source.is_empty() {
        parts.push(format!("from {}", source.join(" or ")));
    }
    let kinds = strings("detail-type");
    if !kinds.is_empty() {
        parts.push(format!("with detail type {}", kinds.join(" or ")));
    }
    let numbers = numeric(o.get("detail"));
    let mut words = parts.join(" ");
    if !numbers.is_empty() {
        let joined = numbers.join(" and ");
        words = if words.is_empty() {
            format!("with {joined}")
        } else {
            format!("{words} and {joined}")
        };
    }
    (!words.is_empty()).then_some(words)
}

/// Where a target sends, from its ARN: `publishes it to the orders-created topic`, `sends it to the pick-lists queue`, `invokes the
/// send-email function`; any other service, `sends it to <name>`.
fn target_phrase(arn: &str) -> String {
    let name = path::tail(arn);
    let service = arn.split(':').nth(2).unwrap_or_default();
    match service {
        "sns" => format!("publishes it to the {name} topic"),
        "sqs" => format!("sends it to the {name} queue"),
        "lambda" => format!("invokes the {name} function"),
        "kinesis" => format!("puts it on the {name} stream"),
        "states" => format!("starts the {name} state machine"),
        _ => format!("sends it to {name}"),
    }
}

/// `and publishes it to the orders-created topic`; with several targets, `and sends it to 2 targets: a and b`.
#[must_use]
pub fn targets_words(arns: &[String]) -> Option<String> {
    match arns {
        [] => None,
        [one] => Some(target_phrase(one)),
        many => {
            let names: Vec<String> = many.iter().map(|a| path::tail(a)).collect();
            let n = i64::try_from(many.len()).unwrap_or(i64::MAX);
            Some(format!(
                "sends it to {}: {}",
                plural(n, "target", "targets"),
                list(&names)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pattern_words, targets_words};

    #[test]
    fn a_pattern_is_said_by_its_source_detail_type_and_numbers() {
        assert_eq!(
            pattern_words(
                r#"{"source":["larder.orders"],"detail-type":["OrderPlaced"],"detail":{"total_cents":[{"numeric":[">",0]}]}}"#
            )
            .as_deref(),
            Some("from larder.orders with detail type OrderPlaced and a total_cents above 0")
        );
        assert_eq!(
            pattern_words(r#"{"detail":{"n":[{"numeric":["<=",5]}]}}"#).as_deref(),
            Some("with a n at most 5")
        );
        assert_eq!(pattern_words(r#"{"detail":{"x":["y"]}}"#), None);
        assert_eq!(pattern_words("not json"), None);
    }

    #[test]
    fn targets_are_said_by_what_they_are() {
        let sns = "arn:aws:sns:us-east-1:000000000000:orders-created".to_owned();
        let sqs = "arn:aws:sqs:us-east-1:000000000000:pick-lists".to_owned();
        assert_eq!(
            targets_words(std::slice::from_ref(&sns)).as_deref(),
            Some("publishes it to the orders-created topic")
        );
        assert_eq!(
            targets_words(&[sns, sqs]).as_deref(),
            Some("sends it to 2 targets: orders-created and pick-lists")
        );
        assert_eq!(targets_words(&[]), None);
    }
}
