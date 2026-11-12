//! Message attributes (the caller's, and the system's), their checks, their MD5, and which are returned.
//!
//! Sources: the rules and messages are those AWS gave in `test_message_limits`, `test_message_system_attributes` and
//! `test_message_attribute_prefix_selection` (recorded 2026-11-09). Where a message below is marked *unrecorded* it is the
//! best reading of the documentation and is on the recording queue.

use aws_model_sqs::keys::{
    MessageAttributeValue as UserValue, MessageSystemAttributeValue as SystemValue,
};
use aws_schema::view::View;
use aws_wire::ApiError;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::core::invalid_parameter;

/// The most message attributes one message may carry.
const MAX_ATTRIBUTES: usize = 10;

/// The value of an attribute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Value {
    Text(String),
    Binary(Vec<u8>),
}

/// One message attribute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Attr {
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) value: Value,
}

/// A value as the wire gave it, before it is checked.
struct Raw<'a> {
    name: &'a str,
    data_type: Option<&'a str>,
    text: Option<&'a str>,
    binary: Option<&'a [u8]>,
}

/// The caller's attributes of a message, checked.
pub(crate) fn user<'a>(
    entries: impl Iterator<Item = (&'a str, View<'a, UserValue>)>,
) -> Result<Vec<Attr>, ApiError> {
    let mut out = Vec::new();
    for (name, v) in entries {
        let raw = Raw {
            name,
            data_type: v.get(UserValue::DATA_TYPE),
            text: v.get(UserValue::STRING_VALUE),
            binary: v.get(UserValue::BINARY_VALUE),
        };
        out.push(check_user(&raw)?);
    }
    if out.len() > MAX_ATTRIBUTES {
        return Err(invalid_parameter(format!(
            "Number of message attributes [{}] exceeds the allowed maximum [{MAX_ATTRIBUTES}].",
            out.len()
        )));
    }
    Ok(out)
}

fn check_user(raw: &Raw<'_>) -> Result<Attr, ApiError> {
    let name = raw.name;
    if name.to_ascii_lowercase().starts_with("aws.")
        || name.to_ascii_lowercase().starts_with("amazon")
    {
        return Err(invalid_parameter(
            "You can't use message attribute names beginning with 'AWS.' or 'Amazon'. These strings are reserved for internal use.",
        ));
    }
    if !valid_name(name) {
        // Unrecorded.
        return Err(invalid_parameter(format!(
            "Invalid message attribute name '{name}'."
        )));
    }
    let data_type = raw.data_type.filter(|t| !t.is_empty()).ok_or_else(|| {
        // Unrecorded.
        invalid_parameter(format!(
            "The message attribute '{name}' must contain a non-empty message attribute type."
        ))
    })?;
    let kind = data_type.split('.').next().unwrap_or("");
    if !matches!(kind, "String" | "Number" | "Binary") {
        return Err(invalid_parameter(format!(
            "The type of message (user) attribute '{name}' is invalid. You must use only the following supported type prefixes: Binary, Number, String."
        )));
    }
    let value = match (kind, raw.text, raw.binary) {
        ("Binary", _, Some(b)) => Value::Binary(b.to_vec()),
        (_, Some(t), _) if kind != "Binary" && !t.is_empty() => Value::Text(t.to_owned()),
        _ => {
            // Unrecorded.
            return Err(invalid_parameter(format!(
                "The message attribute '{name}' must contain a non-empty message attribute value for message attribute type '{kind}'."
            )));
        }
    };
    Ok(Attr {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        value,
    })
}

/// Letters, digits, underscore, hyphen and period; at most 256; no leading, trailing or doubled period.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains("..")
}

/// The system attributes of a message, checked. Only `AWSTraceHeader` exists; AWS does not check its type or its format.
pub(crate) fn system<'a>(
    entries: impl Iterator<Item = (&'a str, View<'a, SystemValue>)>,
) -> Result<Vec<Attr>, ApiError> {
    let mut out = Vec::new();
    for (name, v) in entries {
        if name != "AWSTraceHeader" {
            return Err(invalid_parameter(format!(
                "Message system attribute name '{name}' is invalid."
            )));
        }
        let data_type = v.get(SystemValue::DATA_TYPE).unwrap_or("String");
        let text = v
            .get(SystemValue::STRING_VALUE)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                // Unrecorded.
                invalid_parameter(format!(
                    "The message system attribute '{name}' must contain a non-empty value."
                ))
            })?;
        out.push(Attr {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            value: Value::Text(text.to_owned()),
        });
    }
    Ok(out)
}

/// The MD5 AWS reports for a set of attributes: attributes in name order, each as a 4-byte big-endian length and the name, a
/// length and the data type, one byte for how the value travels (1 text, 2 binary), and a length and the value.
pub(crate) fn md5_of(attrs: &[Attr]) -> Option<String> {
    if attrs.is_empty() {
        return None;
    }
    let mut sorted: Vec<&Attr> = attrs.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut buf = Vec::new();
    for a in &sorted {
        put(&mut buf, a.name.as_bytes());
        put(&mut buf, a.data_type.as_bytes());
        match &a.value {
            Value::Text(t) => {
                buf.push(1);
                put(&mut buf, t.as_bytes());
            }
            Value::Binary(b) => {
                buf.push(2);
                put(&mut buf, b);
            }
        }
    }
    Some(aws_core::hex::encode(&Md5::digest(&buf)))
}

/// Appends a 4-byte big-endian length and then the bytes.
fn put(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
    buf.extend_from_slice(bytes);
}

/// The bytes a set of attributes adds to a message's size.
pub(crate) fn size_of(attrs: &[Attr]) -> usize {
    attrs
        .iter()
        .map(|a| {
            a.name.len()
                + a.data_type.len()
                + match &a.value {
                    Value::Text(t) => t.len(),
                    Value::Binary(b) => b.len(),
                }
        })
        .sum()
}

/// The attributes named by a receive: `All` or `.*` for every one, `prefix.*` for those with that prefix, a name for itself.
pub(crate) fn select<'a>(attrs: &'a [Attr], names: &[&str]) -> Vec<&'a Attr> {
    attrs
        .iter()
        .filter(|a| {
            names.iter().any(|n| match *n {
                "All" | ".*" => true,
                n if n.ends_with(".*") => a.name.starts_with(n.trim_end_matches(".*")),
                n => a.name == n,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(name: &str, ty: &str, v: &str) -> Attr {
        Attr {
            name: name.into(),
            data_type: ty.into(),
            value: Value::Text(v.into()),
        }
    }

    /// The three attributes of `test_message_attributes_and_batches`; AWS answered `be57d8bd8cbfc9eddd238f68e64e5984`.
    #[test]
    fn the_md5_is_the_one_aws_reported() {
        let attrs = [
            text("kind", "String", "order"),
            text("count", "Number", "3"),
            Attr {
                name: "blob".into(),
                data_type: "Binary".into(),
                value: Value::Binary(vec![0, 1]),
            },
        ];
        assert_eq!(
            md5_of(&attrs).as_deref(),
            Some("be57d8bd8cbfc9eddd238f68e64e5984")
        );
        assert_eq!(md5_of(&[]), None);
    }

    /// The trace header of `test_trace_header_on_a_batch`; AWS answered `5ae4d5d7636402d80f4eb6d213245a88`.
    #[test]
    fn the_system_attribute_md5_is_the_one_aws_reported() {
        let header = "Root=1-5759e988-bd862e3fe1be46a994272793;Parent=53995c3f42cd8ad8;Sampled=1";
        assert_eq!(
            md5_of(&[text("AWSTraceHeader", "String", header)]).as_deref(),
            Some("5ae4d5d7636402d80f4eb6d213245a88")
        );
    }

    #[test]
    fn a_receive_selects_by_name_prefix_or_all() {
        let attrs = [
            text("bar.one", "String", "1"),
            text("bar.two", "String", "2"),
            text("baz", "String", "3"),
        ];
        let names = |sel: &[&str]| {
            select(&attrs, sel)
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&["bar.*"]), ["bar.one", "bar.two"]);
        assert_eq!(names(&["baz"]), ["baz"]);
        assert_eq!(names(&["All"]).len(), 3);
        assert_eq!(names(&[".*"]).len(), 3);
        assert!(names(&[]).is_empty());
    }
}
