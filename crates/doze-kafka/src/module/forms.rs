//! The console's forms (protocol minor 2): create a topic, and produce a message with a key, headers and a partition.
//! The SDK has checked the values against the fields (required, whole numbers in range, a choice of the choices)
//! before these run; what only the broker knows (a name taken) is refused here, at its field.
//!
//! Produce keeps its one box (`input_hint`) and asks for nothing it cannot do without, so an app that does not know
//! forms still produces as it always did: the value it typed, on partition 0.

use std::collections::BTreeMap;

use doze_module::Result;
use doze_module::forms::{field_error, pairs};
use doze_module::proto::msg::blocks::{Field, field};
use doze_module::proto::msg::console::Action;
use doze_module::proto::msg::error::{Error, ErrorKind};

use super::check::{MAX_PARTITIONS, bad_topic_name};
use super::client::Client;

/// The actions' ids.
pub(crate) const PRODUCE: &str = "produce";
pub(crate) const CREATE_TOPIC: &str = "create-topic";

/// The longest retention the form takes, in hours: ten years.
const MOST_HOURS: i64 = 87_600;

fn f(id: &str, label: &str, kind: &str, required: bool) -> Field {
    Field {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        required,
        ..Field::default()
    }
}

fn help(field: Field, words: &str) -> Field {
    Field {
        help: Some(words.into()),
        ..field
    }
}

/// Produce's fields. The value is required: an app without forms sends its one box, which the SDK puts there.
pub(crate) fn produce_fields() -> Vec<Field> {
    vec![
        help(
            f("value", "Value", field::TEXT, true),
            "What the message says; JSON reads as JSON in the viewer.",
        ),
        help(
            f("key", "Key", field::TEXT, false),
            "Messages with the same key go to the same partition.",
        ),
        help(
            f("headers", "Headers", field::PAIRS, false),
            "One name=value a line.",
        ),
        Field {
            min: Some(0),
            max: Some(i64::from(MAX_PARTITIONS) - 1),
            ..help(
                f("partition", "Partition", field::NUMBER, false),
                "Leave blank to let the key choose, as a Java client would (partition 0 without a key).",
            )
        },
    ]
}

/// Create a topic: on the instance, not on a row.
pub(crate) fn create_topic() -> Action {
    let number = |id: &str, label: &str, min: i64, max: i64, default: Option<&str>| Field {
        min: Some(min),
        max: Some(max),
        default: default.map(String::from),
        ..f(id, label, field::NUMBER, default.is_some())
    };
    Action {
        id: CREATE_TOPIC.into(),
        label: "Create a topic".into(),
        kind: String::new(),
        destructive: false,
        input_hint: Some("topic name".into()),
        fields: vec![
            help(
                f("name", "Name", field::TEXT, true),
                "Letters, digits, '.', '_' and '-', up to 249 of them.",
            ),
            number(
                "partitions",
                "Partitions",
                1,
                i64::from(MAX_PARTITIONS),
                Some("1"),
            ),
            help(
                number("replication", "Replication", 1, 1, Some("1")),
                "This broker is a single node.",
            ),
            Field {
                default: Some("delete".into()),
                choices: vec!["delete".into(), "compact".into()],
                ..help(
                    f("cleanup", "Clean-up", field::CHOICE, true),
                    "Delete old messages, or keep the newest of each key.",
                )
            },
            help(
                number("retention_hours", "Retention (hours)", 0, MOST_HOURS, None),
                "How long a message is kept. Blank keeps the broker's default; 0 keeps messages for ever.",
            ),
        ],
    }
}

fn value<'a>(values: &'a BTreeMap<String, String>, id: &str) -> Option<&'a str> {
    values
        .get(id)
        .map(String::as_str)
        .filter(|v| !v.trim().is_empty())
}

/// A number the SDK has checked against its field.
fn number(values: &BTreeMap<String, String>, id: &str) -> Result<Option<i64>> {
    value(values, id)
        .map(|v| {
            v.parse::<i64>()
                .map_err(|_| field_error(id, format!("{id} must be a whole number")))
        })
        .transpose()
}

/// The configs a topic is created with, from the form.
pub(crate) fn topic_configs(values: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    if let Some(c) = value(values, "cleanup") {
        out.insert("cleanup.policy".to_string(), c.to_string());
    }
    match number(values, "retention_hours")? {
        Some(0) => {
            out.insert("retention.ms".into(), "-1".into());
        }
        Some(h) => {
            out.insert("retention.ms".into(), (h * 3_600_000).to_string());
        }
        None => {}
    }
    Ok(out)
}

/// Creates the topic the form names, refusing a name Kafka would refuse or one that is taken.
pub(crate) async fn create(c: &mut Client, values: &BTreeMap<String, String>) -> Result<String> {
    let name = value(values, "name").unwrap_or_default().trim();
    if let Some(why) = bad_topic_name(name) {
        return Err(field_error("name", why));
    }
    let partitions = number(values, "partitions")?.unwrap_or(1);
    let partitions =
        i32::try_from(partitions).map_err(|_| field_error("partitions", "too many partitions"))?;
    let configs = topic_configs(values)?;
    if c.topics().await?.contains_key(name) || !c.create_topic(name, partitions, &configs).await? {
        return Err(field_error(
            "name",
            format!("there is already a topic named {name}"),
        ));
    }
    let s = if partitions == 1 { "" } else { "s" };
    Ok(format!(
        "created topic {name} with {partitions} partition{s}"
    ))
}

/// The partition a keyed message goes to when the form leaves it to the key.
fn for_key(key: Option<&[u8]>, count: i32) -> i32 {
    #[cfg(feature = "console")]
    return kafka_console::partitioner::for_key(key, count);
    #[cfg(not(feature = "console"))]
    {
        let _ = (key, count);
        0
    }
}

/// Produces the form's message to `topic`.
pub(crate) async fn produce(
    c: &mut Client,
    topic: &str,
    input: Option<&str>,
    values: &BTreeMap<String, String>,
) -> Result<String> {
    let Some(t) = c.topics().await?.remove(topic) else {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!("there is no topic {topic}"),
        ));
    };
    let count = i32::try_from(t.partitions.len()).unwrap_or(i32::MAX);
    let body = values
        .get("value")
        .map(String::as_str)
        .or(input)
        .unwrap_or_default();
    let key = value(values, "key").map(str::as_bytes);
    let headers = pairs(
        values
            .get("headers")
            .map(String::as_str)
            .unwrap_or_default(),
    );
    let headers: Vec<(&str, &[u8])> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_bytes()))
        .collect();
    let partition = match number(values, "partition")? {
        Some(p) if p < i64::from(count) => i32::try_from(p).unwrap_or(0),
        Some(_) => {
            let last = count - 1;
            return Err(field_error(
                "partition",
                format!("{topic} has partitions 0 to {last}"),
            ));
        }
        None => for_key(key, count),
    };
    let offset = c
        .produce_record(topic, partition, key, body.as_bytes(), &headers)
        .await?;
    Ok(format!(
        "produced to {topic}, partition {partition} offset {offset}"
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn vals(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_forms_values_become_the_topics_configs() {
        let c = topic_configs(&vals(&[("cleanup", "compact"), ("retention_hours", "24")])).unwrap();
        assert_eq!(c["cleanup.policy"], "compact");
        assert_eq!(c["retention.ms"], "86400000");
        let c = topic_configs(&vals(&[("cleanup", "delete"), ("retention_hours", "0")])).unwrap();
        assert_eq!(c["retention.ms"], "-1", "0 keeps messages for ever");
        let c = topic_configs(&vals(&[("cleanup", "delete"), ("retention_hours", "")])).unwrap();
        assert!(
            !c.contains_key("retention.ms"),
            "blank is the broker's default"
        );
    }

    #[test]
    fn an_app_without_forms_fills_the_one_box_of_each_form() {
        use doze_module::forms::{check, one_box};
        let mut v = BTreeMap::new();
        one_box(&produce_fields(), false, Some("hello"), &mut v);
        check(&produce_fields(), false, &mut v).unwrap();
        assert_eq!(v["value"], "hello", "produce's box is its value");
        let mut v = BTreeMap::new();
        one_box(&create_topic().fields, false, Some("audit"), &mut v);
        check(&create_topic().fields, false, &mut v).unwrap();
        assert_eq!(v["name"], "audit", "create-topic's box is its name");
        let e = check(&produce_fields(), false, &mut BTreeMap::new()).unwrap_err();
        assert_eq!(
            e.message,
            doze_module::forms::NEEDS_NEWER_APP,
            "nothing typed"
        );
        let mut v = vals(&[("name", "orders")]);
        doze_module::forms::check(&create_topic().fields, true, &mut v).unwrap();
        assert_eq!(
            (v["partitions"].as_str(), v["cleanup"].as_str()),
            ("1", "delete")
        );
        let e = doze_module::forms::check(
            &create_topic().fields,
            true,
            &mut vals(&[("name", "o"), ("replication", "3")]),
        )
        .unwrap_err();
        assert_eq!(e.field.as_deref(), Some("replication"));
    }
}
