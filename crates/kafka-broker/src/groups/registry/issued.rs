//! The member ids handed to clients that have not yet come back with them (JoinGroup v4 and later), kept apart from the
//! groups so that a client that only ever asks for an id leaves a table entry and no group.

use std::collections::HashMap;
use std::sync::PoisonError;
use std::time::Duration;

use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use tokio::time::Instant;

use super::Groups;
use crate::groups::machine::{Answer, IdSource, JoinAnswer};

/// How long a member id is good for when the client did not say how long its session is (Kafka keeps a pending member for
/// the session timeout of its JoinGroup).
pub(super) const ISSUED_FOR: Duration = Duration::from_secs(300);
/// How often the table of ids is gone through for the ones past their time (and always when it is full).
pub(super) const PURGE_EVERY: Duration = Duration::from_secs(10);
/// The most ids handed out and not yet come back with. A client that only ever asks for an id costs an entry here, not a
/// group.
pub(super) const MOST_ISSUED: usize = 100_000;
/// The most one group may have outstanding: a client cannot use up the room of all the others.
pub(super) const MOST_ISSUED_PER_GROUP: usize = 1_000;

/// The ids handed out, and when the table was last gone through for the stale ones.
#[derive(Debug)]
pub(super) struct Issued {
    /// Each id with when it was handed out and for how long it is good.
    pub(super) ids: HashMap<(GroupId, MemberId), (Instant, Duration)>,
    /// How many of the ids are each group's.
    pub(super) per_group: HashMap<GroupId, usize>,
    pub(super) purged: Instant,
}

impl Groups {
    pub(super) fn issued(&self) -> std::sync::MutexGuard<'_, Issued> {
        self.issued.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hands a client without an id one to come back with (MEMBER_ID_REQUIRED).
    pub(super) fn issue(&self, group: &GroupId, session_ms: i32) -> Answer {
        let now = Instant::now();
        let mut issued = self.issued();
        // Looking through every id each time would make a flood of them quadratic; at the cap the room is made, and
        // otherwise now and then.
        if issued.ids.len() >= self.most_issued || now.duration_since(issued.purged) > PURGE_EVERY {
            issued
                .ids
                .retain(|_, (at, good_for)| now.duration_since(*at) < *good_for);
            let mut counts: HashMap<GroupId, usize> = HashMap::new();
            for (group, _) in issued.ids.keys() {
                *counts.entry(group.clone()).or_default() += 1;
            }
            issued.per_group = counts;
            issued.purged = now;
        }
        let group_full = issued
            .per_group
            .get(group)
            .is_some_and(|n| *n >= MOST_ISSUED_PER_GROUP);
        if issued.ids.len() >= self.most_issued || group_full {
            return Answer::Join(JoinAnswer::refused(
                ResponseError::CoordinatorNotAvailable,
                None,
            ));
        }
        let id = new_member_id(None);
        // Good for the session timeout the client asked with, as Kafka keeps a pending member.
        let good_for = u64::try_from(session_ms)
            .ok()
            .filter(|ms| *ms > 0)
            .map_or(ISSUED_FOR, Duration::from_millis);
        issued
            .ids
            .insert((group.clone(), id.clone()), (now, good_for));
        *issued.per_group.entry(group.clone()).or_default() += 1;
        Answer::Join(JoinAnswer::refused(
            ResponseError::MemberIdRequired,
            Some(id),
        ))
    }

    pub(super) fn is_issued(&self, group: &GroupId, id: &MemberId) -> bool {
        self.issued()
            .ids
            .get(&(group.clone(), id.clone()))
            .is_some_and(|(at, good_for)| Instant::now().duration_since(*at) < *good_for)
    }

    pub(super) fn spend(&self, group: &GroupId, id: &MemberId) {
        let mut issued = self.issued();
        if issued.ids.remove(&(group.clone(), id.clone())).is_some()
            && let Some(n) = issued.per_group.get_mut(group)
        {
            *n = n.saturating_sub(1);
        }
    }
}

/// Member ids: `doze-` and 32 hex digits, behind the instance id for a static member.
pub(crate) fn member_ids() -> IdSource {
    Box::new(new_member_id)
}

fn new_member_id(instance: Option<&InstanceId>) -> MemberId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Without randomness two ids must still differ: the time and a count will do for a local broker.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        bytes[..8].copy_from_slice(&count.to_be_bytes());
        bytes[8..].copy_from_slice(&now.to_be_bytes()[8..]);
    }
    MemberId::issued(bytes, instance)
}
