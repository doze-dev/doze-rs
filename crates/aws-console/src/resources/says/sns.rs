//! SNS: how a topic's and a subscription's page speak.

use super::super::page::{list, plural};
use super::super::path;
use super::super::{Page, Read, Verb};
use crate::model::{Badge, ResourceRow, Tone};

/// A subscription that has not been confirmed has no ARN of its own yet: SNS lists it as this.
const PENDING: &str = "PendingConfirmation";

/// The names a confirmed subscription delivers to, from the topic's list: a queue or function by its name, else the endpoint.
fn endpoints(r: &Read<'_>) -> Vec<String> {
    let mut names: Vec<String> = r
        .sub("Subscriptions")
        .map(|s| {
            s.rows
                .iter()
                .filter(|row| {
                    !row.cells
                        .first()
                        .cloned()
                        .flatten()
                        .is_some_and(|c| c.contains(PENDING))
                })
                .filter_map(|row| row.cells.get(2).cloned().flatten())
                .map(|e| path::tail(&e))
                .collect()
        })
        .unwrap_or_default();
    // The service lists them in no order a person can rely on.
    names.sort();
    names
}

/// `orders-created sends every publish to its 3 confirmed subscriptions: invoice-queue, inventory-queue and analytics-queue.`
fn topic_sentence(r: &Read<'_>) -> Option<String> {
    let confirmed = r.number("Attributes.SubscriptionsConfirmed")?;
    let pending = r.number("Attributes.SubscriptionsPending").unwrap_or(0);
    let name = r.name;
    if confirmed == 0 && pending == 0 {
        return Some(format!(
            "{name} has no subscriptions, so anything published to it goes nowhere."
        ));
    }
    let mut out = if confirmed == 0 {
        format!(
            "{name} has no confirmed subscription, so anything published to it goes nowhere yet."
        )
    } else {
        let names = endpoints(r);
        let who = if names.len() as i64 == confirmed && names.len() <= 5 {
            format!(": {}", list(&names))
        } else {
            String::new()
        };
        format!(
            "{name} sends every publish to its {}{who}.",
            plural(
                confirmed,
                "confirmed subscription",
                "confirmed subscriptions"
            )
        )
    };
    if pending > 0 && confirmed > 0 {
        out.push_str(&format!(" {pending} more waiting to be confirmed."));
    }
    Some(out)
}

fn topic_badges(r: &Read<'_>) -> Vec<Badge> {
    vec![Badge::new(
        if r.flag("Attributes.FifoTopic") {
            "FIFO"
        } else {
            "Standard"
        },
        Tone::Neutral,
    )]
}

/// `4 topics have 9 confirmed subscriptions and 1 pending.`
fn topics_summary(rows: &[ResourceRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let sum = |at: usize| -> i64 {
        rows.iter()
            .filter_map(|r| r.cells.get(at).cloned().flatten()?.parse::<i64>().ok())
            .sum()
    };
    let (confirmed, pending) = (sum(1), sum(2));
    let n = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let mut out = format!(
        "{} {} {}",
        plural(n, "topic", "topics"),
        if n == 1 { "has" } else { "have" },
        plural(
            confirmed,
            "confirmed subscription",
            "confirmed subscriptions"
        )
    );
    if pending > 0 {
        out.push_str(&format!(" and {pending} pending"));
    }
    out.push('.');
    Some(out)
}

/// A topic's header.
pub const TOPIC: Page = Page {
    sentence: Some(topic_sentence),
    badges: Some(topic_badges),
    verbs: &[
        Verb::ui("edit", "Edit"),
        Verb::ui("publish", "Publish message"),
        Verb::ui("delete", "Delete").danger(),
    ],
    summary: Some(topics_summary),
};

fn pending(r: &Read<'_>) -> bool {
    r.flag("Attributes.PendingConfirmation")
}

/// `Messages published to orders-created are delivered to invoice-queue (SQS), in SNS's JSON envelope.`
fn subscription_sentence(r: &Read<'_>) -> Option<String> {
    let topic = path::tail(&r.text("Attributes.TopicArn")?);
    let protocol = r.text("Attributes.Protocol")?;
    let endpoint = r.text("Attributes.Endpoint")?;
    let to = if matches!(protocol.as_str(), "sqs" | "lambda") {
        path::tail(&endpoint)
    } else {
        endpoint
    };
    if pending(r) {
        return Some(format!(
            "This subscription of {topic} to {to} is waiting to be confirmed, so nothing is delivered to it yet."
        ));
    }
    let mut out = format!("Messages published to {topic} are delivered to {to} ({protocol})");
    out.push_str(if r.flag("Attributes.RawMessageDelivery") {
        ", as the bare message body"
    } else {
        ", in SNS's JSON envelope"
    });
    if r.text("Attributes.FilterPolicy").is_some() {
        out.push_str(", only when they match its filter policy");
    }
    out.push('.');
    Some(out)
}

fn subscription_badges(r: &Read<'_>) -> Vec<Badge> {
    let mut out = vec![if pending(r) {
        Badge::new("Pending confirmation", Tone::Warn)
    } else {
        Badge::new("Confirmed", Tone::Good)
    }];
    if let Some(p) = r.text("Attributes.Protocol") {
        out.push(Badge::new(p, Tone::Neutral));
    }
    out
}

/// A subscription's header.
pub const SUBSCRIPTION: Page = Page {
    sentence: Some(subscription_sentence),
    badges: Some(subscription_badges),
    verbs: &[
        Verb::ui("edit", "Edit"),
        Verb::ui("confirm", "Confirm subscription").when(pending),
        Verb::ui("delete", "Delete").danger(),
    ],
    summary: None,
};
