//! What the Lab asks of the broker besides requests.
//!
//! [`Broker::topic_name`] gives the name behind a topic id (a fault rule that names a topic is matched against a Fetch or Produce from
//! v13, which name their topics by id). With the `inspect` feature, [`ConsoleLab`] is the console's write surface for the three
//! rehearsals that are not rules: a member's session expires, a group rebalances, a compacted partition is cleaned now. Each
//! goes through the machinery the broker uses for the same thing by itself (the group's own task and machine, the cleaner's own
//! round), so what it leaves behind is what the broker would have left. Every method checks a right first, as a protocol request does.

use kafka_storage::fs::Fs;
use kafka_wire::ids::TopicId;

use crate::Broker;

impl<F: Fs> Broker<F> {
    /// The name of the topic with this id, if there is one.
    #[must_use]
    pub fn topic_name(&self, id: kafka_wire::Uuid) -> Option<String> {
        let topic = self.meta.topic_by_id(TopicId::from_uuid(id)?)?;
        Some(topic.name.as_ref().to_owned())
    }
}

#[cfg(feature = "inspect")]
pub use inspect::{CleanOutcome, ConsoleLab, LabOutcome};

#[cfg(feature = "inspect")]
mod inspect {
    use kafka_auth::authz::{operation, resource};
    use kafka_wire::ids::{GroupId, MemberId, PartitionId, TopicName};

    use super::{Broker, Fs};
    use crate::groups::Force;
    use crate::log_notes::RoundNote;
    use crate::{Caller, clock, topic_config};

    /// What came of forcing a step on a group.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LabOutcome {
        /// It was taken.
        Done,
        /// The caller may not.
        Denied,
        /// There is no such group in memory.
        NoSuchGroup,
        /// The group has no such member.
        NoSuchMember,
        /// The group has nobody to rebalance.
        Empty,
        /// A rebalance is already running.
        AlreadyRebalancing,
    }

    /// What came of asking the cleaner for a round now.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CleanOutcome {
        /// A round ran and was put in place.
        Done(RoundNote),
        /// The caller may not.
        Denied,
        /// There is no such partition.
        NoSuchPartition,
        /// The topic does not compact.
        NotCompacted,
        /// The planner finds nothing due.
        NothingDue,
        /// The round failed, in the broker's words.
        Failed(String),
    }

    /// The console's write surface for the Lab's rehearsals.
    #[derive(Debug)]
    pub struct ConsoleLab<'a, F: Fs> {
        broker: &'a Broker<F>,
    }

    impl<F: Fs> Broker<F> {
        /// The Lab's write surface.
        #[must_use]
        pub fn console_lab(&self) -> ConsoleLab<'_, F> {
            ConsoleLab { broker: self }
        }
    }

    impl<F: Fs> ConsoleLab<'_, F> {
        /// Runs out the session of one member of a group now.
        pub async fn expire_member(&self, caller: &Caller, group: &str, member: &str) -> LabOutcome {
            let Some(id) = self.group_id(caller, group) else {
                return self.refused(caller, group);
            };
            let Some(member) = MemberId::new(member) else {
                return LabOutcome::NoSuchMember;
            };
            self.force(&id, Force::Expire(member)).await
        }

        /// Starts a rebalance of a group now (a new epoch for the new protocol).
        pub async fn force_rebalance(&self, caller: &Caller, group: &str) -> LabOutcome {
            let Some(id) = self.group_id(caller, group) else {
                return self.refused(caller, group);
            };
            self.force(&id, Force::Rebalance).await
        }

        fn group_id(&self, caller: &Caller, group: &str) -> Option<GroupId> {
            self.broker
                .authz
                .allows(caller, resource::GROUP, group, operation::DELETE)
                .then(|| GroupId::parse(group).ok())
                .flatten()
        }

        /// Why there is no id: the caller may not, or the name is no group's.
        fn refused(&self, caller: &Caller, group: &str) -> LabOutcome {
            if self
                .broker
                .authz
                .allows(caller, resource::GROUP, group, operation::DELETE)
            {
                LabOutcome::NoSuchGroup
            } else {
                LabOutcome::Denied
            }
        }

        async fn force(&self, id: &GroupId, force: Force) -> LabOutcome {
            use crate::groups::Forced;
            match self.broker.groups.force(id, force).await {
                Some(Forced::Done) => LabOutcome::Done,
                Some(Forced::NoSuchMember) => LabOutcome::NoSuchMember,
                Some(Forced::Empty) => LabOutcome::Empty,
                Some(Forced::AlreadyRebalancing) => LabOutcome::AlreadyRebalancing,
                Some(Forced::NoSuchGroup) | None => LabOutcome::NoSuchGroup,
            }
        }

        /// Runs the cleaner's round on one partition now, if its planner finds one due.
        pub async fn clean_now(&self, caller: &Caller, topic: &str, partition: i32) -> CleanOutcome {
            let b = self.broker;
            if !b.topic_allows(caller, topic, operation::ALTER) {
                return CleanOutcome::Denied;
            }
            let (Ok(name), Some(id)) = (TopicName::parse(topic), PartitionId::new(partition)) else {
                return CleanOutcome::NoSuchPartition;
            };
            let Some(t) = b.meta.topic(&name) else {
                return CleanOutcome::NoSuchPartition;
            };
            if partition >= t.partitions.get() {
                return CleanOutcome::NoSuchPartition;
            }
            if !topic_config::compacts(&t.config) {
                return CleanOutcome::NotCompacted;
            }
            let began = clock::now().get();
            if let Err(e) = b.clean_partition(&t, id).await {
                let why = e.to_string();
                b.log_notes.failed(t.id, partition, clock::now().get(), &why);
                return CleanOutcome::Failed(why);
            }
            match b.log_notes.cleaner_of(t.id, partition).and_then(|n| n.last) {
                Some(round) if round.at_ms >= began => CleanOutcome::Done(round),
                _ => CleanOutcome::NothingDue,
            }
        }
    }
}
