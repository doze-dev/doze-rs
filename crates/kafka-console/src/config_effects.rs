//! When a change to a topic config counts at the broker: a pure table from a key to the moment the broker reads it, and
//! the sentences that say so. Where the broker keeps a key and does not act on it, the table says that and nothing else:
//! no effect is invented.
//!
//! The moments are what the broker does (`docs/port/behaviours/admin.md` ADM-6 and ADM-D7, `topic_config/cleanup.rs`,
//! `handlers/produce/check.rs`, `retention.rs`): a completeness test in `tests/live_config.rs` reads every key the
//! broker lists and fails when one has no entry here.

use crate::model::ConfigPace;

/// Why a key is kept and not acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// Kafka lists it and this broker accepts and shows it, and does nothing with it.
    NotActedOn,
    /// It is about replicas or leaders, and there is one broker.
    OneBroker,
    /// It is about tiered storage, which this broker does not have.
    NoTieredStorage,
}

/// The moment the broker reads a config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// For each request that needs it (a produce, a delete of records).
    NextRequest,
    /// When the next record is appended, by the open partition's writer.
    NextAppend,
    /// At the cleaner's next look.
    CleanerLook,
    /// At the next retention pass.
    RetentionPass,
    /// Never: the broker keeps the value and does nothing with it.
    Kept(Kept),
}

/// The moment a change to `key` counts; `None` for a name that is not a topic config this broker lists.
#[must_use]
pub fn moment(key: &str) -> Option<Moment> {
    Some(match key {
        "cleanup.policy"
        | "compression.type"
        | "max.message.bytes"
        | "message.timestamp.type"
        | "message.timestamp.after.max.ms"
        | "message.timestamp.before.max.ms" => Moment::NextRequest,
        "segment.bytes" | "segment.ms" | "index.interval.bytes" => Moment::NextAppend,
        "min.cleanable.dirty.ratio"
        | "min.compaction.lag.ms"
        | "max.compaction.lag.ms"
        | "delete.retention.ms" => Moment::CleanerLook,
        "retention.ms" | "retention.bytes" => Moment::RetentionPass,
        "compression.gzip.level"
        | "compression.lz4.level"
        | "compression.zstd.level"
        | "segment.jitter.ms"
        | "segment.index.bytes"
        | "file.delete.delay.ms"
        | "preallocate"
        | "flush.messages"
        | "flush.ms" => Moment::Kept(Kept::NotActedOn),
        "min.insync.replicas"
        | "unclean.leader.election.enable"
        | "leader.replication.throttled.replicas"
        | "follower.replication.throttled.replicas" => Moment::Kept(Kept::OneBroker),
        "local.retention.ms"
        | "local.retention.bytes"
        | "remote.storage.enable"
        | "remote.log.copy.disable"
        | "remote.log.delete.on.disable" => Moment::Kept(Kept::NoTieredStorage),
        _ => return None,
    })
}

/// `15 s`, `5 m`: the pace as the boards write it.
#[must_use]
pub fn pace_word(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} m", secs.div_ceil(60))
    } else {
        format!("{} h", secs.div_ceil(3600))
    }
}

fn within(secs: Option<u64>) -> String {
    secs.map_or_else(String::new, |s| format!(", within {}", pace_word(s)))
}

/// What "takes effect" says in the teaching of a dialog: the moment, in words.
#[must_use]
pub fn takes_effect(m: Moment, pace: Option<ConfigPace>) -> String {
    match m {
        Moment::NextRequest => "on the next request".to_owned(),
        Moment::NextAppend => {
            "from the next append, by the open partition's writer; existing segments keep their size"
                .to_owned()
        }
        Moment::CleanerLook => match pace {
            Some(p) if !p.cleaner_on => {
                "never while the cleaner is off (the broker was started without it)".to_owned()
            }
            p => format!(
                "at the cleaner's next look{}",
                within(p.map(|p| p.cleaner_secs))
            ),
        },
        Moment::RetentionPass => format!(
            "at the next retention pass{}",
            within(pace.map(|p| p.retention_secs))
        ),
        Moment::Kept(k) => kept_reason(k).to_owned(),
    }
}

fn kept_reason(k: Kept) -> &'static str {
    match k {
        Kept::NotActedOn => {
            "never: this broker keeps the value and shows it, and does nothing with it"
        }
        Kept::OneBroker => "never: one broker, no replicas, so there is nothing for it to decide",
        Kept::NoTieredStorage => "never: this broker has no tiered storage",
    }
}

/// The sentence under a row of the page.
#[must_use]
pub fn sentence(m: Moment, pace: Option<ConfigPace>) -> String {
    match m {
        Moment::NextRequest => "A change counts on the next request.".to_owned(),
        Moment::NextAppend => {
            "A change counts from the next append, by the open partition's writer; existing segments keep their size."
                .to_owned()
        }
        Moment::CleanerLook => match pace {
            Some(p) if !p.cleaner_on => {
                "The cleaner is off on this broker, so no change counts.".to_owned()
            }
            p => format!(
                "A change counts at the cleaner's next look{}.",
                within(p.map(|p| p.cleaner_secs))
            ),
        },
        Moment::RetentionPass => format!(
            "A change counts at the next retention pass{}.",
            within(pace.map(|p| p.retention_secs))
        ),
        Moment::Kept(k) => {
            let why = match k {
                Kept::NotActedOn => "This broker keeps the value and shows it; it does nothing with it.",
                Kept::OneBroker => "One broker, no replicas: this broker has no use for it.",
                Kept::NoTieredStorage => "No tiered storage: this broker has no use for it.",
            };
            why.to_owned()
        }
    }
}

/// The clause of the saved page that says who sees the new value and when.
#[must_use]
pub fn sees(m: Moment, pace: Option<ConfigPace>) -> String {
    match m {
        Moment::NextRequest => {
            "the broker uses the new value on the next request that needs it".to_owned()
        }
        Moment::NextAppend => "the open partition's writer uses it from the next append".to_owned(),
        Moment::CleanerLook => format!(
            "the cleaner sees the new value at its next look{}",
            within(pace.map(|p| p.cleaner_secs))
        ),
        Moment::RetentionPass => format!(
            "retention uses it at the next pass{}",
            within(pace.map(|p| p.retention_secs))
        ),
        Moment::Kept(_) => "the broker keeps the value and does nothing with it".to_owned(),
    }
}

/// The time column of the saved page's story: when the broker reads the new value.
#[must_use]
pub fn when_label(m: Moment, pace: Option<ConfigPace>) -> String {
    match m {
        Moment::NextRequest => "next request".to_owned(),
        Moment::NextAppend => "next append".to_owned(),
        Moment::CleanerLook => pace.map_or_else(
            || "next look".to_owned(),
            |p| format!("within {}", pace_word(p.cleaner_secs)),
        ),
        Moment::RetentionPass => pace.map_or_else(
            || "next pass".to_owned(),
            |p| format!("within {}", pace_word(p.retention_secs)),
        ),
        Moment::Kept(_) => "never".to_owned(),
    }
}

/// Who reads the value at that moment, as a sentence of the story.
#[must_use]
pub fn reader(m: Moment) -> &'static str {
    match m {
        Moment::NextRequest => "The next request that needs it reads it.",
        Moment::NextAppend => "The next append reads it, in the open partition's writer.",
        Moment::CleanerLook => "The cleaner's next look reads it.",
        Moment::RetentionPass => "The next retention pass reads it.",
        Moment::Kept(_) => "Nothing reads it: this broker only keeps it.",
    }
}

/// The sentence under the page's name about when a change counts, where the broker's pace is known and its cleaner is on.
#[must_use]
pub fn page_moment(pace: Option<ConfigPace>) -> String {
    match pace {
        Some(p) if p.cleaner_on => format!(
            "The cleaner reads them at every look, so a change counts from the next one{}.",
            within(Some(p.cleaner_secs))
        ),
        _ => String::new(),
    }
}

/// The same, for a key by name (the sentence of its row); an unknown key has none.
#[must_use]
pub fn sentence_of(key: &str, pace: Option<ConfigPace>) -> Option<String> {
    moment(key).map(|m| sentence(m, pace))
}

/// The line "What it is" of a dialog for the keys the boards teach; any other key is taught from Kafka's own text.
#[must_use]
pub fn what_it_is(key: &str) -> Option<&'static str> {
    Some(match key {
        "min.cleanable.dirty.ratio" => {
            "how much of the log may be dirty before the cleaner takes it"
        }
        "min.compaction.lag.ms" | "max.compaction.lag.ms" => {
            "how long a record waits before it may be cleaned (min), and how long before it must be (max)"
        }
        "cleanup.policy" => {
            "compact keeps the latest record of each key, delete discards old segments, both together do both"
        }
        "segment.bytes" => "how big a segment file grows before the log starts a new one",
        "retention.ms" => "how long a record is kept before retention may delete its segment",
        "retention.bytes" => {
            "how large a partition's log may grow before retention deletes old segments"
        }
        "max.message.bytes" => "the largest record batch the broker accepts for this topic",
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
