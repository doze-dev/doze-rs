//! What an OffsetFetch may be refused for besides the group's own offsets: the member that asks is not what it says it
//! is, or the partition has offsets that a transaction is still deciding.

use std::collections::HashSet;

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::ids::GroupId;

use crate::Broker;
use crate::groups::fetch_refusal;

impl<F: Fs> Broker<F> {
    /// The member check for one group of an OffsetFetch.
    pub(super) async fn member_refusal(
        &self,
        group: &str,
        member: Option<&str>,
        epoch: i32,
    ) -> Option<E> {
        if member.is_none() && epoch < 0 {
            return None;
        }
        let described = self.groups.describe(&GroupId::parse(group).ok()?).await?;
        fetch_refusal(&described.consumer?, member, epoch)
    }

    /// [`Self::pending_offsets`] if the request asked for stable offsets only.
    pub(super) async fn pending_if(
        &self,
        require_stable: bool,
        group: &str,
    ) -> HashSet<(String, i32)> {
        if require_stable {
            self.pending_offsets(group).await
        } else {
            HashSet::new()
        }
    }

    /// The partitions of `group` that a transaction has offsets for and has not yet ended (until its markers are written,
    /// its offsets are neither committed nor gone).
    pub(super) async fn pending_offsets(&self, group: &str) -> HashSet<(String, i32)> {
        let mut pending = HashSet::new();
        for entry in self.txns.all() {
            let e = entry.read().await;
            if e.rec.state.is_open() {
                pending.extend(
                    e.rec
                        .offsets
                        .iter()
                        .filter(|o| o.group == group)
                        .map(|o| (o.topic.clone(), o.partition)),
                );
            }
        }
        pending
    }
}
