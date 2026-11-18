//! Kafka's own descriptions of the codes a group write answers, and the console's reading of each refusal.

use super::span_ms;
use crate::model::{GroupHead, GroupSpan as S, RefusalStep};
use crate::refusal::{code_name, refusal};

// --- Kafka's words and the console's reading of a refusal ---

/// Kafka's own description of the codes a group write can answer (its `Errors.java`).
#[must_use]
pub fn description(code: i16) -> &'static str {
    match code {
        3 => "This server does not host this topic-partition.",
        12 => "The metadata field of the offset request was too large.",
        14 => "The coordinator is loading and hence can't process requests.",
        15 => "The coordinator is not available.",
        16 => "This is not the correct coordinator.",
        22 => "Specified group generation id is not valid.",
        24 => "The configured groupId is invalid.",
        25 => "The coordinator is not aware of this member.",
        27 => "The group is rebalancing, so a rejoin is needed.",
        29 => "Topic authorization failed.",
        30 => "Group authorization failed.",
        68 => "The group is not empty.",
        69 => "The group id does not exist.",
        82 => {
            "The broker rejected this static consumer since another consumer with the same group.instance.id has registered with a different member.id."
        }
        86 => {
            "Deleting offsets of a topic is forbidden while the consumer group is actively subscribed to it."
        }
        113 => {
            "The member epoch is stale. The member must retry after receiving its updated member epoch via the ConsumerGroupHeartbeat API."
        }
        _ => "The server experienced an unexpected error when processing the request.",
    }
}

/// Kafka's code and sentence for an answer to a group write.
#[must_use]
pub fn kafka_refusal(code: i16) -> crate::model::Refusal {
    let mut r = refusal(code, None);
    r.name = code_name(code);
    r.message = description(code).to_owned();
    r
}

fn members_now(h: &GroupHead) -> String {
    match h.members.len() {
        1 => "1 member now".to_owned(),
        n => format!("{n} members now"),
    }
}

fn leave_step(h: &GroupHead, then: &str) -> RefusalStep {
    let who = if h.members.len() == 1 {
        h.members.first().map_or_else(String::new, |m| m.id.clone())
    } else {
        format!("the {} members", h.members.len())
    };
    let session = h
        .members
        .iter()
        .map(|m| m.session_ms)
        .max()
        .filter(|s| *s > 0)
        .map(|s| span_ms(i64::try_from(s).unwrap_or(0)));
    RefusalStep {
        blocking: true,
        title: then.to_owned(),
        sub: match session {
            Some(s) => format!("{who} leaves, or its {s} session runs out."),
            None => format!("{who} leaves, or its session runs out."),
        },
        value: members_now(h),
    }
}

/// The rule and the steps for a reset that Kafka refused.
#[must_use]
pub fn reset_reading(h: &GroupHead, code: i16) -> (Vec<S>, Vec<RefusalStep>) {
    match code {
        25 | 22 | 27 => (
            vec![S::text(format!(
                "A commit with no generation, which is what a reset is, is accepted only while the group has no members. {} {}, so Kafka asks which member is committing.",
                h.id,
                match h.members.len() {
                    0 => "has none now".to_owned(),
                    1 => format!(
                        "has one, {}",
                        h.members.first().map_or("", |m| m.id.as_str())
                    ),
                    n => format!("has {n}"),
                }
            ))],
            vec![
                leave_step(h, "1. Stop the app"),
                RefusalStep {
                    blocking: false,
                    title: "2. The group is Empty, and the same reset is accepted".to_owned(),
                    sub: "The preview shows each partition before anything is sent.".to_owned(),
                    value: String::new(),
                },
                RefusalStep {
                    blocking: false,
                    title: "3. Start the app again".to_owned(),
                    sub: "It resumes from the new offsets.".to_owned(),
                    value: String::new(),
                },
            ],
        ),
        _ => (
            vec![S::text(
                "Kafka answered the commit with this code, and nothing was stored.",
            )],
            Vec::new(),
        ),
    }
}

/// The rule and the steps for a group delete that Kafka refused.
#[must_use]
pub fn delete_reading(h: &GroupHead, code: i16) -> (Vec<S>, Vec<RefusalStep>) {
    match code {
        68 => (
            vec![S::text(
                "DeleteGroups checks in this order: a group with members is refused; an empty one is deleted with its offsets; a group with nothing in it is not found (GROUP_ID_NOT_FOUND, 69).",
            )],
            vec![RefusalStep {
                blocking: true,
                title: "Every member has to leave".to_owned(),
                sub: "Stop the app, or wait out its session after it dies.".to_owned(),
                value: members_now(h),
            }],
        ),
        69 => (
            vec![S::text(
                "The group has no members, no committed offsets and no record: there is nothing to delete.",
            )],
            Vec::new(),
        ),
        _ => (
            vec![S::text(
                "Kafka answered the delete with this code, and nothing was deleted.",
            )],
            Vec::new(),
        ),
    }
}
