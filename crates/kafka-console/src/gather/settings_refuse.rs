//! A refused change to a broker config: Kafka's code and sentence as the broker said them, what would pass, and the other
//! ways a change is refused, each asked of the broker just now with `validate_only` (nothing is changed by any of them).

use kafka_wire::messages::ApiKey;

use super::settings_configs::of_broker;
use super::settings_edit::{SettingsFields, call_lines, send};
use crate::backend::Backend;
use crate::model::{Refusal, SettingsRefused, SettingsResult, SettingsScope, Span, Tok};
use crate::refusal::refusal;
use crate::settings_words::{DYNAMIC, allowed, flag_of, is_dynamic};

/// What the call shows for the value of a config the console does not edit.
const NOT_SHOWN: &str = "(the value, not shown)";

/// The words of a refused answer.
#[must_use]
pub fn said(code: i16, message: Option<String>) -> Refusal {
    refusal(code, message.as_deref())
}

/// The three other refusals a change meets: a config that is not Kafka's, a value too small, another broker's id. Each
/// is Kafka's answer to a dry run now; one that Kafka does not refuse is left out.
async fn others(b: &Backend, node: i32) -> Vec<(String, String, String)> {
    let probes: [(&str, String, &str, &str, String); 3] = [
        (
            "Not a Kafka config",
            String::new(),
            "foo.bar",
            "1",
            "a name the broker does not have".to_owned(),
        ),
        (
            "Too small or not a number",
            String::new(),
            "log.segment.bytes",
            "524288",
            "log.segment.bytes is at least 1 MiB".to_owned(),
        ),
        (
            "Another broker's id",
            (node + 1).to_string(),
            "log.retention.ms",
            "1",
            format!("this broker is node {node}"),
        ),
    ];
    let mut out = Vec::new();
    for (title, name, key, value, sub) in probes {
        if let Some((code, Some(message))) = send(b, &name, key, value, true).await
            && code != 0
        {
            out.push((title.to_owned(), message, sub));
        }
    }
    out
}

/// What the page says about a refusal besides Kafka's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The rule that was broken.
    pub rule: Vec<Span>,
    /// What would pass.
    pub pass: Vec<Span>,
    /// The flag that sets the config at start, where there is one.
    pub flag: Option<String>,
}

/// The reading of a refusal of `key`, from what the broker lists as fixed (`fixed`: name and value).
#[must_use]
pub fn reading(key: &str, fixed: &[(String, String)]) -> Reading {
    if is_dynamic(key) {
        return Reading {
            rule: vec![
                Span::mono(key),
                Span::text(format!(" is one of the five, and takes: {}", allowed(key))),
            ],
            pass: vec![
                Span::mono(key),
                Span::text(format!(" takes: {}", allowed(key))),
            ],
            flag: None,
        };
    }
    let Some((_, now)) = fixed.iter().find(|(k, _)| k == key) else {
        let five: Vec<&str> = DYNAMIC.iter().map(|d| d.0).collect();
        return Reading {
            rule: vec![
                Span::text("The rule: a broker takes only the configs it has. "),
                Span::mono(key),
                Span::text(" is not one of them."),
            ],
            pass: vec![Span::text(format!(
                "The five that can change while it runs are {}.",
                five.join(", ")
            ))],
            flag: None,
        };
    };
    let rule = vec![
        Span::text("The rule: only five broker configs act on a running broker. "),
        Span::mono(key),
        Span::text(" is read once, at start."),
    ];
    match flag_of(key) {
        Some(flag) => Reading {
            rule,
            pass: vec![
                Span::text("Start the broker with the flag that sets it. For "),
                Span::mono(key),
                Span::text(" that is "),
                Span::mono(flag),
                Span::text(format!(", now {now}.")),
            ],
            flag: Some(flag.to_owned()),
        },
        None => Reading {
            rule,
            pass: vec![
                Span::mono(key),
                Span::text(format!(
                    " is {now} and no flag sets it: it is fixed for this broker."
                )),
            ],
            flag: None,
        },
    }
}

/// The page of a refusal. `refusal` is what the broker said (or `None`: it said nothing, which is shown as such).
pub async fn refused(
    b: &Backend,
    f: SettingsFields<'_>,
    scope: SettingsScope,
    node: i32,
    refusal: Option<Refusal>,
    validate_only: bool,
) -> SettingsResult {
    let retention = b.intervals().map(|i| i.retention_check.as_secs());
    let fixed = of_broker(b, node, retention)
        .await
        .map(|c| c.fixed)
        .unwrap_or_default();
    let mut refusal =
        refusal.unwrap_or_else(|| said(-1, Some("The broker did not answer.".to_owned())));
    // A config that is not one of the five may be a secret (`ssl.key.password`): what was typed for it is never said back, in
    // the call or in the broker's sentence (SEC-189's planted-secret gate, `tests/redaction`).
    let value = if is_dynamic(f.key) {
        f.value
    } else {
        if !f.value.trim().is_empty() {
            refusal.message = refusal.message.replace(f.value, NOT_SHOWN);
        }
        NOT_SHOWN
    };
    let read = reading(f.key, &fixed);
    let version = b.version(ApiKey::IncrementalAlterConfigs);
    let mut call = call_lines(version, scope, node, f.key, value);
    call.push(vec![]);
    call.push(vec![(
        Tok::Plain,
        format!("-> {} ({})", refusal.name, refusal.code),
    )]);
    SettingsResult::Refused(Box::new(SettingsRefused {
        key: f.key.to_owned(),
        refusal,
        rule: read.rule,
        would_pass: read.pass,
        flag: read.flag,
        call,
        others: others(b, node).await,
        fixed: fixed.into_iter().map(|(k, _)| k).collect(),
        validate_only,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said_text(spans: &[Span]) -> String {
        format!("{spans:?}")
    }

    #[test]
    fn a_fixed_config_has_a_flag_a_dynamic_one_a_range_and_an_unknown_one_the_five() {
        let fixed = vec![
            ("num.partitions".to_owned(), "1".to_owned()),
            ("broker.id".to_owned(), "1".to_owned()),
        ];
        let r = reading("num.partitions", &fixed);
        assert_eq!(r.flag.as_deref(), Some("--default-partitions"));
        assert!(said_text(&r.pass).contains("now 1."), "{r:?}");
        assert!(
            said_text(&r.rule).contains("only five broker configs"),
            "{r:?}"
        );
        let r = reading("broker.id", &fixed);
        assert_eq!(r.flag, None);
        assert!(said_text(&r.pass).contains("no flag sets it"), "{r:?}");
        let r = reading("log.segment.bytes", &fixed);
        assert!(said_text(&r.pass).contains("At least 1,048,576"), "{r:?}");
        let r = reading("foo.bar", &fixed);
        assert_eq!(r.flag, None);
        assert!(said_text(&r.rule).contains("is not one of them"), "{r:?}");
        assert!(said_text(&r.pass).contains("log.retention.ms"), "{r:?}");
    }
}
