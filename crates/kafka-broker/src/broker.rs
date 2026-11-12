//! The broker: one decoded request in, one outcome out.

use std::sync::Arc;

use kafka_meta::{FileMode, MetaStore};
use kafka_storage::Trust;
use kafka_storage::fs::{Fs, RealFs};
use kafka_storage::marker::{StopState, record_clean_stop};
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::ApiSet;

use crate::authorizer::Authorizer;
use crate::groups::{ConsumerConfig, Groups, Settings, member_ids};
use crate::partitions::Partitions;
use crate::{BrokerError, Config, blocking};

mod dispatch;

/// The APIs this broker has a handler for. ApiVersions advertises exactly these (and only those the Kafka
/// version being stood for offers), and a request for any other is not accepted. A phase adds the APIs it
/// serves here and a handler for each in [`Broker::handle`]; the `match` there has no wildcard, so a new API
/// that is neither handled nor listed as unserved does not compile.
const SERVED: &[ApiKey] = &[
    ApiKey::ApiVersions,
    ApiKey::Metadata,
    ApiKey::CreateTopics,
    ApiKey::DeleteTopics,
    ApiKey::Produce,
    ApiKey::InitProducerId,
    ApiKey::Fetch,
    ApiKey::ListOffsets,
    ApiKey::FindCoordinator,
    ApiKey::JoinGroup,
    ApiKey::SyncGroup,
    ApiKey::Heartbeat,
    ApiKey::LeaveGroup,
    ApiKey::OffsetCommit,
    ApiKey::OffsetFetch,
    ApiKey::OffsetDelete,
    ApiKey::DescribeGroups,
    ApiKey::ListGroups,
    ApiKey::DeleteGroups,
    ApiKey::ConsumerGroupHeartbeat,
    ApiKey::ConsumerGroupDescribe,
    ApiKey::DeleteRecords,
    ApiKey::CreatePartitions,
    ApiKey::DescribeConfigs,
    ApiKey::AlterConfigs,
    ApiKey::IncrementalAlterConfigs,
    ApiKey::DescribeProducers,
    ApiKey::AddPartitionsToTxn,
    ApiKey::AddOffsetsToTxn,
    ApiKey::EndTxn,
    ApiKey::TxnOffsetCommit,
    ApiKey::DescribeTransactions,
    ApiKey::ListTransactions,
    ApiKey::DescribeAcls,
    ApiKey::CreateAcls,
    ApiKey::DeleteAcls,
    ApiKey::DescribeUserScramCredentials,
    ApiKey::AlterUserScramCredentials,
];

/// The broker, over a file system (the real one, or a simulated disk in tests).
#[derive(Debug)]
pub struct Broker<F: Fs = RealFs> {
    pub(crate) fs: F,
    pub(crate) config: Config,
    /// The broker-wide defaults in force: the flags, as AlterConfigs on the broker has changed them.
    pub(crate) defaults: crate::Live,
    pub(crate) meta: Arc<MetaStore>,
    pub(crate) partitions: Partitions<F>,
    pub(crate) groups: Groups,
    /// The transactional ids.
    pub(crate) txns: crate::txn::Txns,
    /// When this run began, in milliseconds since the epoch.
    pub(crate) started_ms: i64,
    /// Counts changes to the set of topics that a waiting fetch must notice (a deletion).
    pub(crate) topics_changed: tokio::sync::watch::Sender<u64>,
    /// What the last stop vouched for.
    stop: StopState,
    /// Held for the whole of a topic's creation or deletion: a name freed by a delete is not made again until the
    /// delete has cleared the old topic's partitions, which are named the same.
    pub(crate) topic_admin: tokio::sync::Mutex<()>,
    /// Held for the whole of a change to the configs of a topic or of the broker: what is changed is worked out from what
    /// is there, so two changes at once would each be worked out from the same old configs and one would undo the other.
    pub(crate) config_admin: tokio::sync::Mutex<()>,
    /// What the ACLs allow each caller to do: the set in force, and the rules to apply it with.
    pub(crate) authz: Authorizer,
    /// Held for the whole of a change to the ACLs, from the set it starts from to the one swapped in: a change is
    /// worked out from the ACLs there are, so two at once would each start from the same set and one would be lost.
    pub(crate) acl_admin: tokio::sync::Mutex<()>,
    /// The users that can log in: the ones configured at start, and the SCRAM credentials stored over the API.
    pub(crate) users: Arc<kafka_auth::Users>,
    /// Held for the whole of a change to the stored credentials, from the snapshot it starts from to the one swapped
    /// in, for the reason `acl_admin` is.
    pub(crate) user_admin: tokio::sync::Mutex<()>,
    /// Set when the broker is stopping: a round of cleaning that is running gives up (it is looked at between batches).
    pub(crate) stop_cleaning: Arc<std::sync::atomic::AtomicBool>,
}

impl<F: Fs> Broker<F> {
    /// Opens the broker's state under `config.data_dir`, making it if it is not there.
    ///
    /// # Errors
    /// The data directory or the metadata database cannot be made or read.
    pub fn open(fs: F, config: Config) -> Result<Self, BrokerError> {
        kafka_storage::fs::create_dirs_durably(&fs, &config.data_dir)?;
        let meta = Arc::new(MetaStore::open(&fs, &config.data_dir)?);
        match meta.file_mode() {
            FileMode::Private => {}
            FileMode::Narrowed => tracing::info!(
                "the metadata database (it holds SCRAM keys) was readable by group or others and is now readable by its owner only"
            ),
            FileMode::Wide => tracing::warn!(
                "the metadata database (it holds SCRAM keys) is readable by group or others and could not be narrowed: run chmod 600 on meta.redb"
            ),
        }
        // Read, and removed before anything is written: a marker that came back after a power cut would vouch for a
        // log this run was in the middle of writing.
        let stop = StopState::begin(&fs, &config.data_dir)?;
        let defaults = crate::Live::new(config.defaults, stored_configs(&meta, config.node_id)?);
        let partitions = Partitions::new(
            fs.clone(),
            config.clone(),
            defaults.clone(),
            stop.clone(),
            Arc::clone(&meta),
        );
        let authz = Authorizer::new(
            meta.acls()?,
            config.acl_default_allow,
            config.super_users.clone(),
        );
        let users = Arc::clone(&config.users);
        crate::users::load(&meta, &users)?;
        let millis = |d: std::time::Duration| u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        let meta_txns = meta.txns()?;
        let groups = Groups::new(
            Settings {
                initial_delay_ms: millis(config.group_initial_rebalance_delay),
                consumer: ConsumerConfig {
                    session_ms: millis(config.consumer_session_timeout),
                    assign_interval_ms: millis(config.consumer_assignment_interval),
                },
                ids: member_ids,
            },
            Arc::clone(&meta),
        );
        Ok(Self {
            fs,
            config,
            defaults,
            meta,
            partitions,
            groups,
            txns: crate::txn::Txns::load(meta_txns),
            started_ms: crate::clock::now().get(),
            topics_changed: tokio::sync::watch::channel(0).0,
            stop,
            topic_admin: tokio::sync::Mutex::new(()),
            config_admin: tokio::sync::Mutex::new(()),
            authz,
            acl_admin: tokio::sync::Mutex::new(()),
            users,
            user_admin: tokio::sync::Mutex::new(()),
            stop_cleaning: Arc::default(),
        })
    }

    /// Stops cleanly: every open partition is flushed and closed, and the stop is recorded so that the next start
    /// need not distrust what it finds. The caller has stopped the server first, so nothing new is opening.
    ///
    /// # Errors
    /// The record of the stop could not be written; the next start then distrusts every partition, which is slower
    /// and safe.
    pub async fn shutdown(&self) -> Result<(), BrokerError> {
        self.stop_cleaning
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let closed = self.partitions.close_all().await;
        let data = &self.config.data_dir;
        // What this stop cannot vouch for. A partition this run opened is as its last close left it, whatever the
        // last stop said of it; one this run never opened is as the last stop left it.
        let not_covered: Vec<String> = blocking::run(|| self.fs.list_dir(data))?
            .into_iter()
            .filter(|e| e.is_dir)
            .map(|e| e.name)
            .filter(|n| {
                closed.get(n).copied().unwrap_or_else(|| self.stop.trust(n)) == Trust::Unvouched
            })
            .collect();
        blocking::run(|| record_clean_stop(&self.fs, data, &not_covered))?;
        Ok(())
    }

    /// How many partitions have their files open now (never more than `max_open_partitions` plus those a request
    /// is holding).
    #[must_use]
    pub fn open_partitions(&self) -> usize {
        self.partitions.open_count()
    }

    /// The APIs this broker serves.
    #[must_use]
    pub fn served(&self) -> ApiSet {
        ApiSet::of(SERVED)
    }
}

/// The broker configs set before the last stop: for all brokers, and for this one.
fn stored_configs(meta: &MetaStore, node_id: i32) -> Result<crate::configs::Dynamic, BrokerError> {
    let mut out = crate::configs::Dynamic::default();
    for ((scope, name), value) in meta.broker_configs()? {
        if scope.is_empty() {
            out.cluster.insert(name, value);
        } else if scope == node_id.to_string() {
            out.own.insert(name, value);
        }
    }
    Ok(out)
}
