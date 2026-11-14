//! Offsets that have outlived their retention are forgotten, and with them the groups that have nothing left.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use kafka_meta::GroupLife;
use kafka_storage::fs::Fs;
use kafka_wire::ids::GroupId;

use crate::groups::{Deleted, Members, View, expired, members_of};
use crate::{Broker, blocking, clock};

/// The shortest and longest time between looks when it is not set.
const LEAST_EVERY: Duration = Duration::from_millis(50);
const MOST_EVERY: Duration = Duration::from_secs(600);

/// How long between looks: what the configuration says, else a hundredth of the retention within bounds (and the longest
/// when nothing ever expires).
fn check_every(retention: Duration, set: Option<Duration>) -> Duration {
    set.unwrap_or_else(|| {
        if retention.is_zero() {
            MOST_EVERY
        } else {
            (retention / 100).clamp(LEAST_EVERY, MOST_EVERY)
        }
    })
}

/// Whether a group that is not running with members may yet have them, and so is not forgotten: one in memory whose record
/// says it will (a write behind), or one the broker started after, recorded as having members whose session has not yet
/// run out. A group in memory with no record says nothing of the kind: ids handed to clients are not the group's.
fn may_have_members(
    members: &Members<'_>,
    life: Option<&GroupLife>,
    now_ms: i64,
    started_ms: i64,
) -> bool {
    match (members, life) {
        (Members::Empty, Some(l)) => l.live,
        (Members::NotInMemory, Some(l)) => {
            l.live && now_ms < started_ms.saturating_add(i64::from(l.session_ms))
        }
        _ => false,
    }
}

impl<F: Fs> Broker<F> {
    /// How long between looks: a hundredth of the retention, within bounds, unless the configuration says.
    fn offsets_check_every(&self) -> Duration {
        check_every(
            self.config.offsets_retention,
            self.config.offsets_check_every,
        )
    }

    /// Looks at every group's committed offsets once, now: those that have outlived the retention are forgotten, and a
    /// group left with no members and no offsets is forgotten with them.
    pub async fn check_offsets(&self) {
        let now_ms = clock::now().get();
        let retention_ms =
            i64::try_from(self.config.offsets_retention.as_millis()).unwrap_or(i64::MAX);
        // Which groups to look at is taken once; each is then looked at as it is when its turn comes, so that a member
        // that has joined and committed since is seen and not judged by what the group was.
        let in_memory = self.groups.list().await.into_iter().map(|(id, _)| id);
        let lives = match blocking::run(|| self.meta.group_lives()) {
            Ok(lives) => lives,
            Err(e) => {
                tracing::warn!(error = %e, "offsets retention: the groups' records could not be read");
                return;
            }
        };
        let with_offsets = blocking::run(|| self.meta.groups_with_offsets()).unwrap_or_default();
        let ids: BTreeSet<GroupId> = with_offsets
            .into_iter()
            .chain(lives.into_iter().map(|(id, _)| id))
            .chain(in_memory)
            .collect();
        for id in ids {
            self.check_group(&id, now_ms, retention_ms).await;
        }
    }

    async fn check_group(&self, id: &GroupId, now_ms: i64, retention_ms: i64) {
        // In this order: what a group has committed is read first, its record next and its members last, so that a member
        // that joins and commits while this is going on has its commit left out of `offsets` (and so never expired),
        // never judged by a record read before it came.
        let Ok(offsets) = blocking::run(|| self.meta.group_offsets(id)) else {
            return;
        };
        let life = blocking::run(|| self.meta.group_life(id)).ok().flatten();
        let described = self.groups.describe(id).await;
        let members = members_of(described.as_ref());
        let running = matches!(members, Members::Running { .. });
        let waiting = may_have_members(&members, life.as_ref(), now_ms, self.started_ms);
        let view = View {
            members,
            life: life.as_ref(),
            offsets: &offsets,
        };
        let gone = expired(&view, now_ms, retention_ms, self.started_ms);
        if !gone.is_empty() {
            match blocking::run(|| self.meta.expire_offsets(id, &gone)) {
                Ok(n) => tracing::info!(group = %id, offsets = n, "committed offsets expired"),
                Err(e) => {
                    tracing::error!(group = %id, error = %e, "forgetting expired offsets failed");
                    return;
                }
            }
        }
        // Nothing left and nobody there: the group is gone, as a real Kafka forgets it.
        if gone.len() == offsets.len() && !running && !waiting {
            let (outcome, cleaned) = self
                .groups
                .delete_with(id, || blocking::run(|| self.meta.forget_group_life(id)))
                .await;
            if let (Deleted::Gone | Deleted::NoSuchGroup, Some(Err(e))) = (outcome, cleaned) {
                tracing::error!(group = %id, error = %e, "forgetting a group's record failed");
            }
        }
    }

    /// Looks at the offsets now and then, as long as the broker lives.
    pub fn start_offsets_sweeper(self: &Arc<Self>) -> tokio::task::JoinHandle<()>
    where
        F: Send + Sync + 'static,
    {
        let broker = Arc::downgrade(self);
        let every = self.offsets_check_every();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let Some(broker) = broker.upgrade() else {
                    break;
                };
                broker.check_offsets().await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn life(live: bool, session_ms: u32) -> GroupLife {
        GroupLife {
            live,
            session_ms,
            empty_since_ms: (!live).then_some(1),
            protocol_type: "consumer".into(),
            consumer: None,
        }
    }

    #[test]
    fn a_group_may_have_members_if_its_record_says_so_and_nothing_else_does() {
        let (now, started) = (1_000_000, 900_000);
        let may = |members: &Members<'_>, l: Option<&GroupLife>| {
            may_have_members(members, l, now, started)
        };
        // In memory with none: only a record that says live (a write behind) keeps it.
        assert!(may(&Members::Empty, Some(&life(true, 10_000))));
        assert!(!may(&Members::Empty, Some(&life(false, 10_000))));
        assert!(
            !may(&Members::Empty, None),
            "an id given to a client is not the group's"
        );
        // Not in memory, recorded as having members: until the session has run out after the start.
        assert!(
            may(&Members::NotInMemory, Some(&life(true, 200_000))),
            "started + 200 s is after now"
        );
        assert!(
            !may(&Members::NotInMemory, Some(&life(true, 50_000))),
            "started + 50 s is before now"
        );
        assert!(!may(&Members::NotInMemory, Some(&life(false, 200_000))));
        assert!(!may(&Members::NotInMemory, None));
    }

    #[test]
    fn the_time_between_looks_is_a_hundredth_of_the_retention_within_bounds() {
        let secs = Duration::from_secs;
        assert_eq!(
            check_every(secs(168 * 3600), None),
            MOST_EVERY,
            "a week is 1.7 h: capped"
        );
        assert_eq!(check_every(secs(600), None), secs(6));
        assert_eq!(
            check_every(secs(1), None),
            LEAST_EVERY,
            "10 ms is raised to 50 ms"
        );
        assert_eq!(
            check_every(Duration::ZERO, None),
            MOST_EVERY,
            "nothing expires"
        );
        assert_eq!(
            check_every(secs(600), Some(secs(1))),
            secs(1),
            "what is set wins"
        );
    }
}
