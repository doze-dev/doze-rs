//! What the broker would refuse, or would quietly get wrong, refused before boot at its line in doze.hcl. The Go module
//! passed all of it through: a topic Kafka refuses failed the converge after the broker was up, `partitions = 0` and a
//! negative retention were quietly read as their defaults, and `version = "3.7"` quietly ran Kafka 4.

use std::collections::BTreeMap;

use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;

use super::KafkaSpec;

/// The most partitions one topic may have: the broker's `--max-partitions`, which the module leaves at its default.
pub(crate) const MAX_PARTITIONS: i32 = 1000;
/// The longest topic name Kafka allows.
const MAX_NAME: usize = 249;

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn at(path: Vec<PathSeg>, message: String, hint: Option<String>) -> Diagnostic {
    Diagnostic {
        path,
        message,
        hint,
    }
}

/// The Kafka major the instance's `version` asks for: none ("") is 4, the newest.
///
/// # Errors
/// A version that is not one of the four, in words.
pub(crate) fn major(version: &str) -> Result<u8, Diagnostic> {
    match version {
        "" => Ok(4),
        "1" | "2" | "3" | "4" => Ok(version.parse().unwrap_or(4)),
        other => {
            let hint = match other.split_once('.') {
                Some((m, _)) if matches!(m, "1" | "2" | "3" | "4") => {
                    format!("the broker stands for a major as a whole: version = {m}")
                }
                _ => "1, 2, 3 or 4".to_string(),
            };
            Err(at(
                vec![],
                format!("version {other:?} is not a Kafka major this broker stands for"),
                Some(hint),
            ))
        }
    }
}

/// A retention as milliseconds: a Go duration (`168h`, `1h30m`, `0s`) or whole days (`7d`); 0 keeps segments for ever.
///
/// # Errors
/// What is wrong with it, in words.
pub(crate) fn retention_ms(text: &str) -> Result<i64, String> {
    if text.starts_with('-') {
        return Err(format!("{text:?} is negative"));
    }
    if let Some(days) = text.strip_suffix('d')
        && !days.is_empty()
        && days.bytes().all(|b| b.is_ascii_digit())
    {
        let days: i64 = days.parse().map_err(|_| format!("{text:?} is too long"))?;
        return days
            .checked_mul(86_400_000)
            .ok_or_else(|| format!("{text:?} is too long"));
    }
    let d = crate::duration::parse(text)?;
    i64::try_from(d.as_millis()).map_err(|_| format!("{text:?} is too long"))
}

/// Why Kafka would refuse a topic name, if it would (`Topic.validate` in Kafka: 1 to 249 of `[a-zA-Z0-9._-]`, not
/// `.` or `..`).
pub(crate) fn bad_topic_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("a topic needs a name".into());
    }
    if name == "." || name == ".." {
        return Some(format!("{name:?} cannot be a topic's name"));
    }
    if name.len() > MAX_NAME {
        return Some(format!(
            "a topic name is at most {MAX_NAME} characters; this one is {}",
            name.len()
        ));
    }
    let bad: Vec<char> = name
        .chars()
        .filter(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
        .collect();
    (!bad.is_empty()).then(|| {
        format!(
            "topic {name:?} has {bad:?}: a topic name holds only letters, digits, '.', '_' and '-'"
        )
    })
}

fn partitions(path: Vec<PathSeg>, what: &str, n: i32, out: &mut Vec<Diagnostic>) {
    if !(1..=MAX_PARTITIONS).contains(&n) {
        out.push(at(
            path,
            format!("{what} must be from 1 to {MAX_PARTITIONS}, not {n}"),
            (n < 1).then(|| "leave it out for 1".to_string()),
        ));
    }
}

fn configs(i: u32, configs: &BTreeMap<String, String>, out: &mut Vec<Diagnostic>) {
    let names: Vec<&str> = kafka_broker::topic_config_names().collect();
    let path = |k: &str| vec![key("topic"), PathSeg::Index(i), key("config"), key(k)];
    for (k, v) in configs {
        let one = BTreeMap::from([(k.clone(), v.clone())]);
        if let Err(why) = kafka_broker::check_topic_configs(&one) {
            let hint = if names.contains(&k.as_str()) {
                None
            } else {
                Some(match doze_module::spec::closest(k, &names) {
                    Some(near) => format!("did you mean {near:?}?"),
                    None => {
                        "a topic config as Kafka names it, such as cleanup.policy or retention.ms"
                            .into()
                    }
                })
            };
            out.push(at(path(k), why, hint));
        }
    }
    // What only a pair of configs can break (min.compaction.lag.ms above max.compaction.lag.ms).
    if out.is_empty()
        && let Err(why) = kafka_broker::check_topic_configs(configs)
    {
        out.push(at(
            vec![key("topic"), PathSeg::Index(i), key("config")],
            why,
            None,
        ));
    }
}

/// Every problem with the block, each at its path.
pub(crate) fn check(spec: &KafkaSpec, version: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if let Err(d) = major(version) {
        out.push(d);
    }
    partitions(
        vec![key("default_partitions")],
        "default_partitions",
        spec.default_partitions,
        &mut out,
    );
    if let Some(r) = &spec.retention
        && let Err(why) = retention_ms(r)
    {
        let hint = "a duration such as \"168h\", \"90m\" or \"7d\"; leave it out to keep segments for ever";
        out.push(at(
            vec![key("retention")],
            format!("retention {why}"),
            Some(hint.into()),
        ));
    }
    if spec.retention_bytes < -1 {
        out.push(at(
            vec![key("retention_bytes")],
            format!(
                "retention_bytes must be 0 or more, not {}",
                spec.retention_bytes
            ),
            Some("0 (or -1, as Kafka writes it) is no limit".into()),
        ));
    }
    for (i, t) in spec.topic.iter().enumerate() {
        let i = u32::try_from(i).unwrap_or(u32::MAX);
        let here = |k: &str| vec![key("topic"), PathSeg::Index(i), key(k)];
        if let Some(why) = bad_topic_name(&t.name) {
            out.push(at(here("name"), why, None));
        }
        partitions(here("partitions"), "partitions", t.partitions, &mut out);
        let mut own = Vec::new();
        configs(i, &t.configs(), &mut own);
        out.extend(own);
    }
    out
}
