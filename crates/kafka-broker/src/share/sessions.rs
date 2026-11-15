//! Share sessions (Kafka's `ShareSessionCache`, `ShareSessionContext`): one per member of a group on this broker, holding
//! the partitions the member fetches and the epoch its next request must carry. A ShareFetch at epoch 0 opens one (and
//! replaces any the member had, without letting go of what it holds), -1 closes it, any other epoch must be the one
//! expected and moves it on. ShareAcknowledge moves the same epoch but never opens a session.
//!
//! A session ends when it is closed, replaced, or its connection goes; nothing else times it out. The cache holds at most
//! `group.share.max.share.sessions` and makes room for nobody.

use std::collections::HashMap;

use kafka_wire::ResponseError as E;
use kafka_wire::Uuid;
use kafka_wire::ids::{GroupId, MemberId};

/// One partition of a session, by topic id.
pub(crate) type Part = (Uuid, i32);

/// Kafka's `ShareRequestMetadata.nextEpoch`: after the largest comes 1, never 0.
#[must_use]
pub(crate) fn next_epoch(epoch: i32) -> i32 {
    match epoch {
        e if e < 0 => -1,
        i32::MAX => 1,
        e => e + 1,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cached {
    part: Part,
    /// Added to the session and not yet reported, or in error the last time: reported in the next response whatever it
    /// holds (Kafka's `requiresUpdateInResponse`).
    report: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Session {
    /// The partitions, in the order a response reports them (one that had data goes to the back).
    parts: Vec<Cached>,
    /// The epoch the next request must carry.
    pub(crate) epoch: i32,
    /// The connection that made it.
    connection: u64,
}

/// What a ShareFetch is to do, after its session has been looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Context {
    /// Epoch 0: the request's own partitions, reported whatever they hold.
    Full(Vec<Part>),
    /// A session continued: every partition of the session; only those with something to say are reported.
    Incremental(Vec<Part>),
    /// Epoch -1: nothing fetched; the session is closed once the acknowledgements are done.
    Final,
}

/// Every share session of the broker.
#[derive(Debug)]
pub(crate) struct Sessions {
    by_member: HashMap<(GroupId, MemberId), Session>,
    most: usize,
}

impl Sessions {
    #[must_use]
    pub(crate) fn new(most: usize) -> Self {
        Self {
            by_member: HashMap::new(),
            most,
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.by_member.len()
    }

    /// A ShareFetch's session (Kafka's `SharePartitionManager.newContext`): `asked` is the request's partitions, `forgotten`
    /// those it drops, `acks` whether it carries acknowledgements.
    ///
    /// # Errors
    /// INVALID_REQUEST (acknowledgements at epoch 0), SHARE_SESSION_NOT_FOUND, INVALID_SHARE_SESSION_EPOCH,
    /// SHARE_SESSION_LIMIT_REACHED.
    pub(crate) fn fetch(
        &mut self,
        key: (GroupId, MemberId),
        epoch: i32,
        (asked, forgotten): (&[Part], &[Part]),
        acks: bool,
        connection: u64,
    ) -> Result<Context, E> {
        match epoch {
            0 => {
                if acks {
                    return Err(E::InvalidRequest);
                }
                // The member's old session goes first: a replacement needs no room of its own.
                self.by_member.remove(&key);
                if self.by_member.len() >= self.most {
                    return Err(E::ShareSessionLimitReached);
                }
                let mut parts: Vec<Cached> = Vec::new();
                for p in asked {
                    if !parts.iter().any(|c| c.part == *p) {
                        parts.push(Cached { part: *p, report: true });
                    }
                }
                self.by_member.insert(
                    key,
                    Session {
                        parts,
                        epoch: next_epoch(0),
                        connection,
                    },
                );
                Ok(Context::Full(asked.to_vec()))
            }
            -1 => {
                if self.by_member.contains_key(&key) {
                    Ok(Context::Final)
                } else {
                    Err(E::ShareSessionNotFound)
                }
            }
            _ => {
                let s = self.by_member.get_mut(&key).ok_or(E::ShareSessionNotFound)?;
                if s.epoch != epoch {
                    return Err(E::InvalidShareSessionEpoch);
                }
                for p in asked {
                    if !s.parts.iter().any(|c| c.part == *p) {
                        s.parts.push(Cached { part: *p, report: true });
                    }
                }
                s.parts.retain(|c| !forgotten.contains(&c.part));
                s.epoch = next_epoch(epoch);
                Ok(Context::Incremental(s.parts.iter().map(|c| c.part).collect()))
            }
        }
    }

    /// A ShareAcknowledge's session (Kafka's `acknowledgeSessionUpdate`). `Ok(true)`: the session is to be closed after
    /// the acknowledgements.
    ///
    /// # Errors
    /// INVALID_SHARE_SESSION_EPOCH (epoch 0, or not the one expected), SHARE_SESSION_NOT_FOUND.
    pub(crate) fn acknowledge(&mut self, key: &(GroupId, MemberId), epoch: i32) -> Result<bool, E> {
        if epoch == 0 {
            return Err(E::InvalidShareSessionEpoch);
        }
        let s = self.by_member.get_mut(key).ok_or(E::ShareSessionNotFound)?;
        if epoch == -1 {
            return Ok(true);
        }
        if s.epoch != epoch {
            return Err(E::InvalidShareSessionEpoch);
        }
        s.epoch = next_epoch(epoch);
        Ok(false)
    }

    /// Whether the session is where a response to a request at `epoch` expects it: another request has not moved it since
    /// (Kafka's check before an incremental response).
    #[must_use]
    pub(crate) fn still_at(&self, key: &(GroupId, MemberId), epoch: i32) -> bool {
        self.by_member
            .get(key)
            .is_some_and(|s| s.epoch == next_epoch(epoch))
    }

    /// Which partitions of an incremental response are reported, given what each holds (`has`: records or an error),
    /// in the session's order. A partition with records goes to the back of the order; one in error is reported again
    /// next time.
    pub(crate) fn report(&mut self, key: &(GroupId, MemberId), results: &[(Part, bool, bool)]) -> Vec<Part> {
        let Some(s) = self.by_member.get_mut(key) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut to_back = Vec::new();
        for (part, records, error) in results {
            let Some(c) = s.parts.iter_mut().find(|c| c.part == *part) else {
                continue;
            };
            if *records || *error || c.report {
                out.push(*part);
                c.report = *error;
            }
            if *records {
                to_back.push(*part);
            }
        }
        for part in to_back {
            if let Some(i) = s.parts.iter().position(|c| c.part == part) {
                let c = s.parts.remove(i);
                s.parts.push(c);
            }
        }
        out
    }

    /// Closes a member's session. `true`: it had one.
    pub(crate) fn close(&mut self, key: &(GroupId, MemberId)) -> bool {
        self.by_member.remove(key).is_some()
    }

    /// Closes the sessions a connection made: the group and member of each, with its partitions.
    pub(crate) fn close_connection(&mut self, connection: u64) -> Vec<((GroupId, MemberId), Vec<Part>)> {
        let keys: Vec<(GroupId, MemberId)> = self
            .by_member
            .iter()
            .filter(|(_, s)| s.connection == connection)
            .map(|(k, _)| k.clone())
            .collect();
        keys.into_iter()
            .filter_map(|k| {
                let s = self.by_member.remove(&k)?;
                Some((k, s.parts.iter().map(|c| c.part).collect()))
            })
            .collect()
    }

    /// How many sessions `group` has on this broker.
    #[must_use]
    pub(crate) fn of_group(&self, group: &GroupId) -> usize {
        self.by_member.keys().filter(|(g, _)| g == group).count()
    }

    /// The partitions of a member's session, if it has one.
    #[must_use]
    pub(crate) fn parts(&self, key: &(GroupId, MemberId)) -> Vec<Part> {
        self.by_member
            .get(key)
            .map(|s| s.parts.iter().map(|c| c.part).collect())
            .unwrap_or_default()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Uuid = Uuid::from_u128(1);

    fn key(m: &str) -> (GroupId, MemberId) {
        (GroupId::parse("g").unwrap(), MemberId::new(m).unwrap())
    }

    #[test]
    fn epochs_count_from_one_and_wrap_to_one() {
        assert_eq!(next_epoch(0), 1);
        assert_eq!(next_epoch(7), 8);
        assert_eq!(next_epoch(i32::MAX), 1);
        assert_eq!(next_epoch(-1), -1);
    }

    #[test]
    fn a_session_opens_at_zero_and_each_request_must_carry_the_next_epoch() {
        let mut s = Sessions::new(10);
        let parts = [(T, 0), (T, 1)];
        assert_eq!(s.fetch(key("m"), 0, (&parts, &[]), false, 1), Ok(Context::Full(parts.to_vec())));
        assert_eq!(s.fetch(key("m"), 2, (&[], &[]), false, 1), Err(E::InvalidShareSessionEpoch));
        assert_eq!(
            s.fetch(key("m"), 1, (&[(T, 2)], &[(T, 0)]), true, 1),
            Ok(Context::Incremental(vec![(T, 1), (T, 2)]))
        );
        assert_eq!(s.acknowledge(&key("m"), 2), Ok(false), "ShareAcknowledge moves the same epoch");
        assert_eq!(s.fetch(key("m"), 3, (&[], &[]), false, 1).map(|_| ()), Ok(()));
        assert_eq!(s.acknowledge(&key("m"), 0), Err(E::InvalidShareSessionEpoch));
        assert_eq!(s.acknowledge(&key("x"), 1), Err(E::ShareSessionNotFound));
        assert_eq!(s.fetch(key("x"), 5, (&[], &[]), false, 1), Err(E::ShareSessionNotFound));
        assert_eq!(s.fetch(key("m"), -1, (&[], &[]), false, 1), Ok(Context::Final));
        assert_eq!(s.fetch(key("x"), -1, (&[], &[]), false, 1), Err(E::ShareSessionNotFound));
        assert_eq!(s.fetch(key("m"), 0, (&[], &[]), true, 1), Err(E::InvalidRequest), "no acks at 0");
    }

    #[test]
    fn the_cache_is_full_at_its_limit_but_a_member_may_replace_its_own_session() {
        let mut s = Sessions::new(1);
        assert!(s.fetch(key("a"), 0, (&[], &[]), false, 1).is_ok());
        assert_eq!(s.fetch(key("b"), 0, (&[], &[]), false, 1), Err(E::ShareSessionLimitReached));
        assert!(s.fetch(key("a"), 0, (&[], &[]), false, 1).is_ok(), "a replacement");
        assert_eq!(s.close_connection(1), vec![(key("a"), vec![])]);
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn an_incremental_response_reports_new_partitions_once_and_errors_until_they_clear() {
        let mut s = Sessions::new(10);
        s.fetch(key("m"), 0, (&[(T, 0), (T, 1)], &[]), false, 1).unwrap();
        let k = key("m");
        assert_eq!(s.report(&k, &[((T, 0), false, false), ((T, 1), false, false)]), [(T, 0), (T, 1)]);
        assert!(s.report(&k, &[((T, 0), false, false), ((T, 1), false, false)]).is_empty());
        assert_eq!(s.report(&k, &[((T, 0), false, true), ((T, 1), true, false)]), [(T, 0), (T, 1)]);
        assert_eq!(s.parts(&k), [(T, 0), (T, 1)], "the one with records went to the back");
        assert_eq!(s.report(&k, &[((T, 0), false, false), ((T, 1), false, false)]), [(T, 0)], "reported again after its error");
    }
}
