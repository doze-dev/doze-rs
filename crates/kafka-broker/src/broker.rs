//! The broker: one decoded request in, one outcome out.

use std::sync::Arc;

use kafka_meta::MetaStore;
use kafka_storage::Trust;
use kafka_storage::fs::{Fs, RealFs};
use kafka_storage::marker::{StopState, record_clean_stop};
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::ApiSet;

use crate::groups::{ConsumerConfig, Groups, Settings, member_ids};
use crate::partitions::Partitions;
use crate::{BrokerError, Config, Listener, Outcome, blocking};

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
    ApiKey::DescribeProducers,
    ApiKey::AddPartitionsToTxn,
    ApiKey::AddOffsetsToTxn,
    ApiKey::EndTxn,
    ApiKey::TxnOffsetCommit,
    ApiKey::DescribeTransactions,
    ApiKey::ListTransactions,
];

/// The broker, over a file system (the real one, or a simulated disk in tests).
#[derive(Debug)]
pub struct Broker<F: Fs = RealFs> {
    pub(crate) fs: F,
    pub(crate) config: Config,
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
}

impl<F: Fs> Broker<F> {
    /// Opens the broker's state under `config.data_dir`, making it if it is not there.
    ///
    /// # Errors
    /// The data directory or the metadata database cannot be made or read.
    pub fn open(fs: F, config: Config) -> Result<Self, BrokerError> {
        kafka_storage::fs::create_dirs_durably(&fs, &config.data_dir)?;
        let meta = Arc::new(MetaStore::open(&fs, &config.data_dir)?);
        // Read, and removed before anything is written: a marker that came back after a power cut would vouch for a
        // log this run was in the middle of writing.
        let stop = StopState::begin(&fs, &config.data_dir)?;
        let partitions =
            Partitions::new(fs.clone(), config.clone(), stop.clone(), Arc::clone(&meta));
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
            meta,
            partitions,
            groups,
            txns: crate::txn::Txns::load(meta_txns),
            started_ms: crate::clock::now().get(),
            topics_changed: tokio::sync::watch::channel(0).0,
            stop,
            topic_admin: tokio::sync::Mutex::new(()),
        })
    }

    /// Stops cleanly: every open partition is flushed and closed, and the stop is recorded so that the next start
    /// need not distrust what it finds. The caller has stopped the server first, so nothing new is opening.
    ///
    /// # Errors
    /// The record of the stop could not be written; the next start then distrusts every partition, which is slower
    /// and safe.
    pub async fn shutdown(&self) -> Result<(), BrokerError> {
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

    /// Handles one request. ApiVersions never gets here: the connection answers it from the version profile,
    /// before the connection is known to be allowed anything else.
    #[expect(
        clippy::cognitive_complexity,
        reason = "an exhaustive match over every API, with no wildcard, so that an API with no handler does not compile"
    )]
    pub async fn handle(
        &self,
        listener: &Listener,
        head: &RequestHead,
        request: Request,
    ) -> Outcome {
        match request {
            Request::Metadata(r) => {
                let (answer, wanted) = blocking::run(|| self.metadata(listener, &r, head.version));
                self.create_wanted(wanted).await;
                respond(Response::Metadata(answer))
            }
            Request::CreateTopics(r) => {
                respond(Response::CreateTopics(self.create_topics(&r).await))
            }
            Request::DeleteTopics(r) => {
                respond(Response::DeleteTopics(self.delete_topics(&r).await))
            }
            Request::Produce(r) => self.produce(head.version, r).await,
            Request::Fetch(r) => self.fetch(head.version, r).await,
            Request::ListOffsets(r) => respond(Response::ListOffsets(
                self.list_offsets(head.version, &r).await,
            )),
            Request::FindCoordinator(r) => respond(Response::FindCoordinator(
                self.find_coordinator(head.version, listener, &r),
            )),
            Request::JoinGroup(r) => {
                self.join_group(head.version, head.client_id.as_deref().unwrap_or(""), &r)
                    .await
            }
            Request::SyncGroup(r) => self.sync_group(&r).await,
            Request::Heartbeat(r) => self.heartbeat(&r).await,
            Request::LeaveGroup(r) => self.leave_group(head.version, &r).await,
            Request::OffsetCommit(r) => self.offset_commit(head.version, &r).await,
            Request::OffsetFetch(r) => self.offset_fetch(&r).await,
            Request::OffsetDelete(r) => self.offset_delete(&r).await,
            Request::DescribeGroups(r) => self.describe_groups(&r).await,
            Request::ListGroups(r) => self.list_groups(&r).await,
            Request::DeleteGroups(r) => self.delete_groups(&r).await,
            Request::ConsumerGroupHeartbeat(r) => {
                self.consumer_group_heartbeat(
                    head.version,
                    head.client_id.as_deref().unwrap_or(""),
                    &r,
                )
                .await
            }
            Request::ConsumerGroupDescribe(r) => self.consumer_group_describe(&r).await,
            Request::DescribeProducers(r) => self.describe_producers(&r).await,
            Request::InitProducerId(r) => {
                respond(Response::InitProducerId(self.init_producer_id(&r).await))
            }
            Request::AddPartitionsToTxn(r) => self.add_partitions_to_txn(head.version, &r).await,
            Request::AddOffsetsToTxn(r) => self.add_offsets_to_txn(head.version, &r).await,
            Request::EndTxn(r) => self.end_txn_request(head.version, &r).await,
            Request::TxnOffsetCommit(r) => self.txn_offset_commit(&r).await,
            Request::DescribeTransactions(r) => self.describe_transactions(&r).await,
            Request::ListTransactions(r) => self.list_transactions(&r).await,
            // Not served yet. Each moves out of this arm into a handler when its phase lands.
            Request::ApiVersions(_)
            | Request::SaslHandshake(_)
            | Request::DeleteRecords(_)
            | Request::DescribeAcls(_)
            | Request::CreateAcls(_)
            | Request::DeleteAcls(_)
            | Request::DescribeConfigs(_)
            | Request::AlterConfigs(_)
            | Request::SaslAuthenticate(_)
            | Request::CreatePartitions(_)
            | Request::IncrementalAlterConfigs(_)
            | Request::DescribeCluster(_)
            | Request::GetTelemetrySubscriptions(_)
            | Request::PushTelemetry(_) => Outcome::Close,
        }
    }
}

fn respond(response: Response) -> Outcome {
    Outcome::Respond(response)
}
