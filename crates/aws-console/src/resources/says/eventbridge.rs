//! EventBridge: how a rule's, a bus's and an archive's page speak, and what the list of each says of its rows.

use super::super::page::{list, plural};
use super::super::path;
use super::super::{Page, Read, Verb};
use super::pattern::{pattern_words, targets_words};
use crate::model::{Badge, ResourceRow, Tone};

/// The bus a rule is on: `larder` for `larder/order-placed`, `default` for a rule named without one.
fn bus_of(r: &Read<'_>) -> String {
    r.text("EventBusName")
        .unwrap_or_else(|| "default".to_owned())
}

/// The ids of the targets a rule has, from its list.
fn targets(r: &Read<'_>) -> Vec<String> {
    r.sub("Targets")
        .map(|s| {
            s.rows
                .iter()
                .filter_map(|row| row.cells.first().cloned().flatten())
                .map(|a| path::tail(&a))
                .collect()
        })
        .unwrap_or_default()
}

/// `order-placed matches events from larder.orders with detail type OrderPlaced and a total_cents above 0, and publishes it to the orders-created topic.` A pattern that names none of those, or a rule that is disabled, keeps to the plain form: `order-placed is enabled on the larder bus and sends what matches its pattern to 1 target: orders-created.`
fn rule_sentence(r: &Read<'_>) -> Option<String> {
    let state = r.text("State")?;
    let on = if state == "ENABLED" {
        "enabled"
    } else {
        "disabled"
    };
    let schedule = r.text("ScheduleExpression");
    let arns: Vec<String> = r
        .sub("Targets")
        .map(|s| {
            s.rows
                .iter()
                .filter_map(|row| row.cells.get(2).cloned().flatten())
                .collect()
        })
        .unwrap_or_default();
    if schedule.is_none()
        && state == "ENABLED"
        && let Some(words) = r.text("EventPattern").as_deref().and_then(pattern_words)
        && let Some(to) = targets_words(&arns)
    {
        let name = r.name.rsplit('/').next().unwrap_or(r.name);
        return Some(format!("{name} matches events {words}, and {to}."));
    }
    let named = targets(r);
    let n = i64::try_from(named.len()).unwrap_or(0);
    let name = r.name.rsplit('/').next().unwrap_or(r.name);
    let head = format!("{name} is {on} on the {} bus", bus_of(r));
    Some(match (schedule, n) {
        (Some(s), 0) => {
            format!("{head} and runs on {s}. It has no target yet, so it does nothing.")
        }
        (Some(s), _) => format!(
            "{head} and runs on {s}, sending to {}: {}.",
            plural(n, "target", "targets"),
            list(&named)
        ),
        (None, 0) => format!(
            "{head} and matches events by its pattern. It has no target yet, so it does nothing."
        ),
        (None, _) => format!(
            "{head} and sends what matches its pattern to {}: {}.",
            plural(n, "target", "targets"),
            list(&named)
        ),
    })
}

fn rule_badges(r: &Read<'_>) -> Vec<Badge> {
    r.text("State")
        .map(|s| {
            let tone = if s == "ENABLED" {
                Tone::Good
            } else {
                Tone::Neutral
            };
            Badge::new(
                if s == "ENABLED" {
                    "Enabled"
                } else {
                    "Disabled"
                },
                tone,
            )
        })
        .into_iter()
        .collect()
}

fn disabled(r: &Read<'_>) -> bool {
    r.text("State").is_some_and(|s| s != "ENABLED")
}

fn enabled(r: &Read<'_>) -> bool {
    r.text("State").is_some_and(|s| s == "ENABLED")
}

/// `2 rules on larder, 1 enabled.`
fn rules_summary(rows: &[ResourceRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let n = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let on = rows
        .iter()
        .filter(|r| r.cells.first().cloned().flatten().as_deref() == Some("ENABLED"))
        .count();
    let on = i64::try_from(on).unwrap_or(0);
    Some(format!(
        "{} listed, {on} enabled and {} disabled.",
        plural(n, "rule", "rules"),
        n - on
    ))
}

/// A rule's header: enable and disable, add a target, delete.
pub const RULE: Page = Page {
    sentence: Some(rule_sentence),
    badges: Some(rule_badges),
    verbs: &[
        Verb::ui("disable", "Disable").when(enabled),
        Verb::ui("enable", "Enable").when(disabled),
        Verb::ui("delete", "Delete").danger(),
    ],
    summary: Some(rules_summary),
};

fn is_default(r: &Read<'_>) -> bool {
    r.name == "default"
}

fn bus_badges(r: &Read<'_>) -> Vec<Badge> {
    vec![Badge::new(
        if is_default(r) {
            "Default event bus"
        } else {
            "Custom event bus"
        },
        Tone::Neutral,
    )]
}

fn custom(r: &Read<'_>) -> bool {
    !is_default(r)
}

/// `larder has 2 rules and 3 archives. Events sent to this bus by name reach only its rules, never the ones on default.`
fn bus_sentence(r: &Read<'_>) -> Option<String> {
    let rules = i64::try_from(r.rows("Rules")?).ok()?;
    let mut out = format!("{} has {}", r.name, plural(rules, "rule", "rules"));
    if let Some(a) = r.rows("Archives").and_then(|n| i64::try_from(n).ok()) {
        out.push_str(&format!(" and {}", plural(a, "archive", "archives")));
    }
    out.push('.');
    if !is_default(r) {
        out.push_str(
            " Events sent to this bus by name reach only its rules, never the ones on default.",
        );
    }
    Some(out)
}

/// A bus's header.
pub const BUS: Page = Page {
    sentence: Some(bus_sentence),
    badges: Some(bus_badges),
    verbs: &[
        Verb::ui("send", "Send events"),
        Verb::ui("delete", "Delete").danger().when(custom),
    ],
    summary: None,
};

fn archive_sentence(r: &Read<'_>) -> Option<String> {
    let bus = path::tail(&r.text("EventSourceArn")?);
    let events = r.number("EventCount").unwrap_or(0);
    let keeps = match r.number("RetentionDays") {
        Some(d) if d > 0 => format!("for {}", plural(d, "day", "days")),
        _ => "indefinitely".to_owned(),
    };
    let what = if r.text("EventPattern").is_some() {
        "the events of its pattern"
    } else {
        "every event"
    };
    Some(format!(
        "{} keeps {what} from the {bus} bus {keeps} and holds {}.",
        r.name,
        plural(events, "event", "events")
    ))
}

/// An archive's header.
pub const ARCHIVE: Page = Page {
    sentence: Some(archive_sentence),
    badges: Some(|r| {
        r.text("State")
            .map(|s| Badge::new(s.clone(), super::tone(&s)))
            .into_iter()
            .collect()
    }),
    verbs: &[
        Verb::ui("replay", "Replay"),
        Verb::ui("delete", "Delete").danger(),
    ],
    summary: None,
};

#[cfg(test)]
mod tests {
    use super::rules_summary;
    use crate::model::ResourceRow;

    fn row(state: &str) -> ResourceRow {
        ResourceRow {
            name: "r".into(),
            label: "r".into(),
            id: "r".into(),
            cells: vec![Some(state.into())],
            link: None,
        }
    }

    #[test]
    fn a_list_counts_its_rules_by_state() {
        assert_eq!(rules_summary(&[]), None);
        assert_eq!(
            rules_summary(&[row("ENABLED"), row("DISABLED"), row("ENABLED")]).as_deref(),
            Some("3 rules listed, 2 enabled and 1 disabled.")
        );
    }
}
