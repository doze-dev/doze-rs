//! Erase everything: the broker as a new one would be, but for who may use it. Every topic goes (as DeleteTopics deletes
//! one: clients see it vanish and are told so), with the committed offsets on it; then every group that has no members;
//! then every transactional id whose transaction is over. Partition directories that belong to no topic go too.
//!
//! Kept: the ACLs, the users made over the API, the broker's dynamic configs, and the producer id counter (an id is never
//! handed out twice, so a client that remembers one cannot be mistaken for a new one). A group with members connected now
//! stays a group (its members are there), with nothing committed. The schema registry is not the broker's: whoever holds
//! both erases it beside this (the console, `doze-kafka data erase`).

use std::collections::BTreeSet;

use kafka_auth::authz::Operation;
use kafka_meta::TxnState;
use kafka_storage::fs::Fs;
use kafka_storage::topic_dirs::{partition_dir, remove_stray_dirs};
use kafka_storage::usage::{dir_bytes, walk_data_dir};
use kafka_wire::ResponseError;

use super::DataRefusal;
use crate::{Broker, Caller, blocking};

/// What erasing everything did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Erased {
    pub topics: usize,
    /// The bytes the deleted topics' directories (and any that belonged to no topic) held.
    pub freed_bytes: u64,
    /// Groups deleted with their committed offsets.
    pub groups: usize,
    /// Groups with members connected now, which stay (with no committed offsets).
    pub groups_kept: Vec<String>,
    /// Transactional ids forgotten.
    pub transactional_ids: usize,
}

impl<F: Fs> Broker<F> {
    /// Erases every topic, group and finished transactional id. The caller must be allowed to ALTER the cluster. Refused,
    /// before anything is touched, while any transaction is under way.
    ///
    /// # Errors
    /// [`DataRefusal`]: no right, a transaction under way, or the metadata store refused a deletion (what went before it
    /// stays gone).
    pub async fn erase_everything(&self, caller: &Caller) -> Result<Erased, DataRefusal> {
        if !self.cluster_allows(caller, Operation::Alter) {
            return Err(DataRefusal::Denied {
                right: "ALTER on the cluster".to_owned(),
                cluster: true,
            });
        }
        if let Some(id) = self.txn_under_way(None).await {
            return Err(DataRefusal::TxnOpen {
                topic: None,
                id: Some(id),
            });
        }
        let mut out = Erased::default();
        // Looked for first: a group whose only trace is its commits loses them with the topics.
        let groups = self.group_names().await;
        self.erase_topics(&mut out).await?;
        self.erase_groups(groups, &mut out).await;
        self.erase_txn_ids(&mut out).await;
        tracing::info!(
            topics = out.topics,
            freed_bytes = out.freed_bytes,
            groups = out.groups,
            groups_kept = out.groups_kept.len(),
            transactional_ids = out.transactional_ids,
            "erased everything"
        );
        Ok(out)
    }

    /// Every topic, as DeleteTopics deletes one (the registry first, then the open partitions close, then the files go),
    /// with no topic made meanwhile; then the directories no topic owns.
    async fn erase_topics(&self, out: &mut Erased) -> Result<(), DataRefusal> {
        let data = self.config.data_dir.clone();
        {
            let _one_change_at_a_time = self.topic_admin.lock().await;
            for topic in self.meta.topics() {
                let held: u64 = topic
                    .partitions
                    .ids()
                    .map(|id| {
                        blocking::run(|| {
                            dir_bytes(&self.fs, &partition_dir(&data, &topic.name, id))
                        })
                    })
                    .sum();
                match blocking::run(|| self.meta.delete_topic(&topic.name)) {
                    Ok(Some(_)) => {}
                    Ok(None) => continue,
                    Err(e) => {
                        return Err(DataRefusal::Storage {
                            what: format!("the topic {}", topic.name),
                            message: e.to_string(),
                        });
                    }
                }
                self.partitions.close_topic(topic.id).await;
                self.shares.forget_topic(topic.id.to_uuid());
                blocking::run(|| self.clear_topic_data(&topic));
                out.topics += 1;
                out.freed_bytes += held;
            }
            out.freed_bytes += blocking::run(|| self.clear_strays());
        }
        // Fetches waiting on a topic have their answer, and the groups that read them rebalance.
        self.topics_changed.send_modify(|n| *n += 1);
        self.groups.topics_changed().await;
        Ok(())
    }

    /// Removes the partition directories that belong to no topic; says how many bytes they held.
    fn clear_strays(&self) -> u64 {
        let data = &self.config.data_dir;
        let Ok(walked) = walk_data_dir(&self.fs, data) else {
            return 0;
        };
        let mut freed = 0;
        let mut names = BTreeSet::new();
        for dir in walked.partitions {
            let owned = kafka_wire::ids::TopicName::parse(&dir.topic)
                .ok()
                .and_then(|n| self.meta.topic(&n))
                .is_some();
            if !owned {
                freed += dir.bytes;
                names.insert(dir.topic);
            }
        }
        for name in names {
            let Ok(topic) = kafka_wire::ids::TopicName::parse(&name) else {
                continue;
            };
            if let Err(e) = remove_stray_dirs(&self.fs, data, &topic) {
                tracing::warn!(topic = %name, error = %e, "could not remove partition directories that belonged to no topic");
            }
        }
        freed
    }

    /// Every group the broker knows of: in memory, with committed offsets, or with a record of its members.
    async fn group_names(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = self
            .groups
            .list()
            .await
            .into_iter()
            .map(|(g, _)| g.as_str().to_owned())
            .collect();
        if let Ok(stored) = blocking::run(|| self.meta.groups_with_offsets()) {
            names.extend(stored.into_iter().map(|g| g.as_str().to_owned()));
        }
        if let Ok(lives) = blocking::run(|| self.meta.group_lives()) {
            names.extend(lives.into_iter().map(|(g, _)| g.as_str().to_owned()));
        }
        names
    }

    /// The groups found before the topics went: those with no members go with their offsets, as DeleteGroups deletes them
    /// (one already gone with its topics' offsets is gone all the same); those with members stay.
    async fn erase_groups(&self, names: BTreeSet<String>, out: &mut Erased) {
        for name in names {
            match self.delete_group(&name).await {
                None | Some(ResponseError::GroupIdNotFound) => out.groups += 1,
                Some(ResponseError::NonEmptyGroup) => out.groups_kept.push(name),
                Some(e) => {
                    tracing::warn!(group = %name, error = ?e, "erase: a group could not be deleted");
                    out.groups_kept.push(name);
                }
            }
        }
    }

    /// Every transactional id whose transaction is over (or never began): its record goes first, then its entry, as when it
    /// expires. One whose transaction began meanwhile stays.
    async fn erase_txn_ids(&self, out: &mut Erased) {
        for entry in self.txns.all() {
            let mut e = entry.write().await;
            let over = matches!(
                e.rec.state,
                TxnState::Empty | TxnState::CompleteCommit | TxnState::CompleteAbort
            );
            if e.retired || !over {
                continue;
            }
            match blocking::run(|| self.meta.delete_txn(&e.rec.id)) {
                Ok(()) => {
                    e.retired = true;
                    self.txns.remove(&e.rec.id, &entry);
                    out.transactional_ids += 1;
                }
                Err(error) => {
                    tracing::warn!(transactional_id = %e.rec.id, %error, "erase: a transactional id could not be forgotten");
                }
            }
        }
    }
}
