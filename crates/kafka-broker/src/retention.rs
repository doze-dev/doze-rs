//! Offsets that have outlived their retention are forgotten, and with them the groups that have nothing left.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use kafka_meta::GroupLife;
use kafka_storage::fs::Fs;
use kafka_wire::ids::GroupId;

use crate::groups::{Deleted, Members, State, View, expired};
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
        // Held still for the whole look: one view of the groups and their records.
        let described: HashMap<GroupId, _> = self.groups.list().await.into_iter().collect();
        let lives: HashMap<GroupId, GroupLife> = match blocking::run(|| self.meta.group_lives()) {
            Ok(lives) => lives.into_iter().collect(),
            Err(e) => {
                tracing::warn!(error = %e, "offsets retention: the groups' records could not be read");
                return;
            }
        };
        let with_offsets = blocking::run(|| self.meta.groups_with_offsets()).unwrap_or_default();
        let ids: BTreeSet<GroupId> = with_offsets
            .into_iter()
            .chain(lives.keys().cloned())
            .chain(described.keys().cloned())
            .collect();
        for id in ids {
            self.check_group(
                &id,
                described.get(&id),
                lives.get(&id),
                now_ms,
                retention_ms,
            )
            .await;
        }
    }

    async fn check_group(
        &self,
        id: &GroupId,
        described: Option<&crate::groups::Description>,
        life: Option<&GroupLife>,
        now_ms: i64,
        retention_ms: i64,
    ) {
        let Ok(offsets) = blocking::run(|| self.meta.group_offsets(id)) else {
            return;
        };
        let members = match described {
            Some(d) if !d.members.is_empty() => Members::Running {
                rebalancing: matches!(
                    d.state,
                    State::PreparingRebalance | State::CompletingRebalance
                ),
                consumer: d.protocol_type.as_deref() == Some("consumer"),
                subscribed: d.subscribed.as_ref(),
            },
            Some(_) => Members::Empty,
            None => Members::NotInMemory,
        };
        let running = matches!(members, Members::Running { .. });
        // A group in memory that has just been asked to join, and been told the id to come back with, has no member
        // yet and a record that says it will: forgetting it would lose that id. It stays until it has been emptied, or
        // the broker has started again.
        let waiting = matches!(members, Members::Empty) && life.is_none_or(|l| l.live);
        let view = View {
            members,
            life,
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
            if let Deleted::NotEmpty = self.groups.delete(id).await {
                return;
            }
            if let Err(e) = blocking::run(|| self.meta.forget_group_life(id)) {
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
