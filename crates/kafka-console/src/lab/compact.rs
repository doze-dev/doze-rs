//! Compact now: the dialog on a compacted partition's Log page that runs the cleaner's real round at once, with the planner's
//! verdict shown first. It is refused, with a sentence, when the partition is not compacted or the planner finds nothing due.

use kafka_broker::CleanOutcome;

use crate::backend::Backend;
use crate::lab::Lab;
use crate::log_words::{pct, would_rewrite};
use crate::model::lab::{LabCompact, LabOnceKind};
use crate::model::{LogCleaner, LogView};

/// The dialog for a partition's log: the planner's verdict and what a round would do. `lab_on`: the Lab is installed.
#[must_use]
pub fn dialog(log: &LogView, lab_on: bool) -> LabCompact {
    let mut v = LabCompact {
        topic: log.topic.clone(),
        partition: log.partition,
        due: None,
        rewrite: Vec::new(),
        checkpoint: (log.checkpoint, log.checkpoint),
        blocked: None,
        refused: None,
        off: !lab_on,
    };
    let Some(c) = &log.compaction else {
        v.blocked = Some(if log.cleanup == crate::model::Cleanup::Delete {
            "This topic does not compact (its cleanup.policy has no compact), so there is no cleaner round to run.".to_owned()
        } else {
            "Nothing has been written to this partition yet, so there is nothing to clean."
                .to_owned()
        });
        return v;
    };
    if matches!(c.cleaner, LogCleaner::Disabled) {
        v.blocked = Some(
            "The cleaner is off (--log-cleaner-enable false): no log is cleaned until it is started again."
                .to_owned(),
        );
        return v;
    }
    if !c.planner.due {
        v.blocked = Some(match c.planner.ratio() {
            Some(r) => format!(
                "The planner finds nothing due: {} of the cleanable log is dirty, and it waits for more than {}.",
                pct(r),
                pct(c.rules.min_dirty_ratio)
            ),
            None => "The planner finds nothing due: nothing sealed is dirty.".to_owned(),
        });
        return v;
    }
    let ratio = c.planner.ratio().unwrap_or(0.0);
    v.due = Some(format!(
        "{} dirty against {}",
        pct(ratio),
        pct(c.rules.min_dirty_ratio)
    ));
    v.rewrite = would_rewrite(log);
    v.checkpoint = (log.checkpoint, c.planner.upto);
    v
}

/// Runs the round. `Ok` is that a round ran and was put in place; `Err` is the dialog again with what the broker said.
pub async fn run(lab: &Lab, backend: &Backend, mut v: LabCompact) -> Result<(), Box<LabCompact>> {
    let out = backend.lab_clean(&v.topic, v.partition).await;
    let name = format!("{}-{}", v.topic, v.partition);
    let (ok, text) = match out {
        CleanOutcome::Done(r) => (
            true,
            format!(
                "Ran the cleaner on {name}: {} segment{} replaced, checkpoint at {}.",
                r.replaced,
                if r.replaced == 1 { "" } else { "s" },
                crate::format::number(r.clean_to)
            ),
        ),
        CleanOutcome::Denied => (
            false,
            "The console may not run the cleaner on this topic.".to_owned(),
        ),
        CleanOutcome::NoSuchPartition => (false, "There is no such partition.".to_owned()),
        CleanOutcome::NotCompacted => (false, "The topic does not compact.".to_owned()),
        CleanOutcome::NothingDue => (
            false,
            "The planner found nothing due when the round was asked for.".to_owned(),
        ),
        CleanOutcome::Failed(why) => (false, format!("The round failed: {why}")),
    };
    lab.note(
        LabOnceKind::Clean,
        if ok {
            text.clone()
        } else {
            format!("Could not clean {name}: {text}")
        },
        ok,
        Some(v.topic.clone()),
    );
    if ok {
        Ok(())
    } else {
        v.refused = Some(text);
        Err(Box::new(v))
    }
}
