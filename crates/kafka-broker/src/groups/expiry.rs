//! When the offsets sweep will take a group's committed offsets, as a date, worked out by the same rules as
//! [`expired`](super::expired) (feature `inspect`: the console shows the date).
//!
//! The date is a prediction from what is so now: a member that joins before it changes the answer, and so does a
//! commit. A test holds [`first_expiry`] to [`expired`] itself, at the moments just before and after each date, over a
//! grid of groups, so the two cannot drift apart.

use kafka_meta::GroupOffset;

use super::retention::{Members, View};

/// The first moment (milliseconds since the epoch) at which `expired` takes each of `view.offsets`, in their order;
/// `None` for one it never takes while things stay as they are (a group with members on a topic it subscribes to, a
/// group in a rebalance, a group that may yet have members).
///
/// `expired` takes an offset when its basis is older than the retention (`basis < now - retention`), so the first moment is
/// `basis + retention + 1`.
pub(crate) fn first_expiry(
    view: &View<'_>,
    retention_ms: i64,
    started_ms: i64,
) -> Vec<Option<i64>> {
    if retention_ms <= 0 {
        return vec![None; view.offsets.len()];
    }
    let after = |basis: i64| Some(basis.saturating_add(retention_ms).saturating_add(1));
    let own = |o: &GroupOffset| after(o.offset.committed_ms);
    // Where the broker has started since and the group is recorded as having had members: they are taken to be there
    // until the longest session has run out.
    let presumed_until = |session_ms: u32| started_ms.saturating_add(i64::from(session_ms));
    view.offsets
        .iter()
        .map(|o| match (&view.members, view.life) {
            (
                Members::Running {
                    rebalancing,
                    consumer,
                    subscribed,
                },
                _,
            ) => match subscribed {
                Some(topics)
                    if *consumer && !*rebalancing && !topics.contains(o.topic.as_str()) =>
                {
                    own(o)
                }
                _ => None,
            },
            (_, None) => own(o),
            (members, Some(life)) if life.consumer.is_some() => {
                if !life.live {
                    own(o)
                } else if matches!(members, Members::NotInMemory) {
                    // Waiting is over once the session has run out; the offset's own age then decides.
                    own(o).map(|a| a.max(presumed_until(life.session_ms)))
                } else {
                    None
                }
            }
            (members, Some(life)) => {
                let since = if life.live {
                    matches!(members, Members::NotInMemory).then(|| presumed_until(life.session_ms))
                } else {
                    life.empty_since_ms
                };
                match since {
                    Some(since) => after(since),
                    None if life.live => None,
                    None => own(o),
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
