//! What each kind says of its resource ([`super::Page`]): a sentence, badges, verbs. One file a service, as its descriptor.

pub mod dynamodb;
pub mod eventbridge;
pub mod kinesis;
pub mod kms;
pub mod lambda;
pub mod logs;
pub mod pattern;
pub mod s3;
pub mod sns;
pub mod sqs;
pub mod ssm;
pub mod stepfunctions;

use super::Read;
use crate::model::{Badge, Tone};

/// How a status word is shown: what is working, what is in the middle of something, what failed, and the rest.
#[must_use]
pub fn tone(status: &str) -> Tone {
    let s = status.to_ascii_uppercase();
    match s.as_str() {
        "OK" | "ACTIVE" | "ENABLED" | "AVAILABLE" | "READY" | "SUCCEEDED" | "CONFIRMED"
        | "CREATE_COMPLETE" | "UPDATE_COMPLETE" | "IMPORT_COMPLETE" => Tone::Good,
        "ALARM" | "FAILED" | "TIMED_OUT" | "ABORTED" | "ROLLBACK_COMPLETE" | "ROLLBACK_FAILED"
        | "UNAVAILABLE" => Tone::Bad,
        "INSUFFICIENT_DATA"
        | "CREATING"
        | "UPDATING"
        | "DELETING"
        | "PENDING"
        | "PENDINGDELETION"
        | "PENDINGIMPORT"
        | "UPDATE_ROLLBACK_COMPLETE" => Tone::Warn,
        _ if s.ends_with("_FAILED") => Tone::Bad,
        _ if s.ends_with("_IN_PROGRESS") || s.starts_with("PENDING") => Tone::Warn,
        _ => Tone::Neutral,
    }
}

/// The badge of a status word, when there is one.
#[must_use]
pub fn status(word: Option<String>) -> Vec<Badge> {
    word.map(|w| Badge::new(w.clone(), tone(&w)))
        .into_iter()
        .collect()
}

/// A function that gives a detail the badge of the status at `$path` of its answers.
macro_rules! status_badge {
    ($name:ident, $path:expr) => {
        #[doc = concat!("The badge of `", $path, "`.")]
        #[must_use]
        pub fn $name(r: &Read<'_>) -> Vec<Badge> {
            status(r.text($path))
        }
    };
}

status_badge!(alarm, "MetricAlarms.@AlarmName.StateValue");
status_badge!(stack, "Stacks.0.StackStatus");
status_badge!(rule, "State");
status_badge!(function, "Configuration.State");

#[cfg(test)]
mod tests {
    use super::tone;
    use crate::model::Tone;

    #[test]
    fn a_status_word_has_a_tone() {
        assert_eq!(tone("ACTIVE"), Tone::Good);
        assert_eq!(tone("ALARM"), Tone::Bad);
        assert_eq!(tone("CREATE_FAILED"), Tone::Bad);
        assert_eq!(tone("UPDATE_IN_PROGRESS"), Tone::Warn);
        assert_eq!(tone("PendingDeletion"), Tone::Warn);
        assert_eq!(tone("DISABLED"), Tone::Neutral);
    }
}
