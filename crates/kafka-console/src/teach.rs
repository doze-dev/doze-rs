//! The teaching under the Create-topic fields: what Kafka says (its own documentation string, from the broker's table),
//! the default, the range, when it takes effect, and what is so on this broker. Pure functions from facts to
//! [`Teach`]; the facts are read by the caller.

use kafka_broker::{ConfigHelp, Limits};

use crate::format::number;
use crate::model::{Span, Teach};
use crate::validate::Taken;

/// Kafka's documentation string as plain text: tags removed, whitespace collapsed, and no more than the first
/// sentences that fit in about 300 characters (the full text is in the Config page, slice 9d).
#[must_use]
pub fn plain_doc(doc: &str) -> String {
    let mut text = String::with_capacity(doc.len());
    let mut in_tag = false;
    for c in doc.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => text.push(c),
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for sentence in text.split_inclusive(". ") {
        if !out.is_empty() && out.chars().count() + sentence.chars().count() > 300 {
            break;
        }
        out.push_str(sentence);
    }
    out.trim_end().to_owned()
}

fn t(s: &str) -> Span {
    Span::text(s)
}

fn m(s: &str) -> Span {
    Span::mono(s)
}

/// The topic's name.
#[must_use]
pub fn name(taken: Option<&Taken>) -> Teach {
    let mut here = vec![
        m("a.b"),
        t(" and "),
        m("a_b"),
        t(" collide, and names that differ only in letter case are refused."),
    ];
    match taken {
        Some(Taken::Free) => here.push(t(" The name is free.")),
        Some(Taken::Exists) => here.push(t(" A topic has this name already.")),
        Some(Taken::Collides(other)) => {
            here.push(t(" It collides with "));
            here.push(Span::mono(other.clone()));
            here.push(t("."));
        }
        None => {}
    }
    Teach {
        says: "legal characters are ASCII letters and digits, . _ and -, at most 249 of them."
            .to_owned(),
        default: vec![t("none: a topic needs a name.")],
        range: vec![
            t("1 to 249 characters, not "),
            m("."),
            t(" or "),
            m(".."),
            t("."),
        ],
        takes_effect: vec![t(
            "at once; the name can never be changed, only deleted and made again.",
        )],
        here,
    }
}

/// The number of partitions.
#[must_use]
pub fn partitions(limits: Option<Limits>) -> Teach {
    let most = limits.map_or_else(
        || "1 or more".to_owned(),
        |l| format!("1 to {}", number(i64::from(l.max_partitions))),
    );
    let mut default = vec![Span::mono(
        limits.map_or(1, |l| l.default_partitions).to_string(),
    )];
    default.push(t(" here ("));
    default.push(m("--default-partitions"));
    default.push(t(")"));
    let mut range = vec![t(&most)];
    if limits.is_some() {
        range.push(t(" ("));
        range.push(m("--max-partitions"));
        range.push(t("); above that is refused INVALID_PARTITIONS."));
    }
    Teach {
        says: "the number of partitions to create in the topic, or -1 for the default.".to_owned(),
        default,
        range,
        takes_effect: vec![t("at once. It can be raised later, never lowered.")],
        here: vec![t(
            "order is kept inside a partition only. One broker holds all of them, with replication factor 1.",
        )],
    }
}

/// `cleanup.policy`, from the broker's own table.
#[must_use]
pub fn cleanup(help: &ConfigHelp) -> Teach {
    Teach {
        says: plain_doc(help.doc),
        default: vec![Span::mono(help.default.clone())],
        range: vec![m("delete"), t(", "), m("compact"), t(", or both.")],
        takes_effect: vec![t(
            "read for each request; the cleaner and the retention pass look at their own intervals.",
        )],
        here: vec![
            t("a topic that compacts, alone or with delete, refuses a record with no key ("),
            m("INVALID_RECORD"),
            t(")."),
        ],
    }
}

/// `retention.ms`, from the broker's own table.
#[must_use]
pub fn retention(help: &ConfigHelp) -> Teach {
    let mut default = vec![Span::mono(help.default.clone())];
    if let Ok(ms) = help.default.parse::<i64>() {
        default.push(t(&format!(
            " ({})",
            crate::validate::retention_words(&ms.to_string()).trim_start_matches("= ")
        )));
    }
    Teach {
        says: plain_doc(help.doc),
        default,
        range: vec![t("-1, or 0 and up.")],
        takes_effect: vec![t(
            "at the next retention pass; the active segment is never deleted.",
        )],
        here: vec![t("only the delete policy uses it.")],
    }
}

#[cfg(test)]
mod tests;
