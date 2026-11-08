//! The broker: one decoded request in, one outcome out.

use kafka_meta::MetaStore;
use kafka_storage::Trust;
use kafka_storage::fs::{Fs, RealFs};
use kafka_storage::marker::{StopState, record_clean_stop};
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::ApiSet;

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
];

/// The broker, over a file system (the real one, or a simulated disk in tests).
#[derive(Debug)]
pub struct Broker<F: Fs = RealFs> {
    pub(crate) fs: F,
    pub(crate) config: Config,
    pub(crate) meta: MetaStore,
    pub(crate) partitions: Partitions<F>,
    /// Counts changes to the set of topics that a waiting fetch must notice (a deletion).
    pub(crate) topics_changed: tokio::sync::watch::Sender<u64>,
    /// What the last stop vouched for.
    stop: StopState,
}

impl<F: Fs> Broker<F> {
    /// Opens the broker's state under `config.data_dir`, making it if it is not there.
    ///
    /// # Errors
    /// The data directory or the metadata database cannot be made or read.
    pub fn open(fs: F, config: Config) -> Result<Self, BrokerError> {
        kafka_storage::fs::create_dirs_durably(&fs, &config.data_dir)?;
        let meta = MetaStore::open(&fs, &config.data_dir)?;
        // Read, and removed before anything is written: a marker that came back after a power cut would vouch for a
        // log this run was in the middle of writing.
        let stop = StopState::begin(&fs, &config.data_dir)?;
        let partitions = Partitions::new(fs.clone(), config.clone(), stop.clone());
        Ok(Self {
            fs,
            config,
            meta,
            partitions,
            topics_changed: tokio::sync::watch::channel(0).0,
            stop,
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
        // What this stop cannot vouch for: partition directories the last stop did not, and that this run did not
        // open and close cleanly.
        let not_covered: Vec<String> = blocking::run(|| self.fs.list_dir(data))?
            .into_iter()
            .filter(|e| e.is_dir)
            .map(|e| e.name)
            .filter(|n| self.stop.trust(n) == Trust::Unvouched && !closed.contains(n))
            .collect();
        blocking::run(|| record_clean_stop(&self.fs, data, &not_covered))?;
        Ok(())
    }

    /// The APIs this broker serves.
    #[must_use]
    pub fn served(&self) -> ApiSet {
        ApiSet::of(SERVED)
    }

    /// Handles one request. ApiVersions never gets here: the connection answers it from the version profile,
    /// before the connection is known to be allowed anything else.
    pub async fn handle(
        &self,
        listener: &Listener,
        head: &RequestHead,
        request: Request,
    ) -> Outcome {
        match request {
            Request::Metadata(r) => respond(Response::Metadata(blocking::run(|| {
                self.metadata(listener, &r, head.version)
            }))),
            Request::CreateTopics(r) => respond(Response::CreateTopics(blocking::run(|| {
                self.create_topics(&r)
            }))),
            Request::DeleteTopics(r) => {
                respond(Response::DeleteTopics(self.delete_topics(&r).await))
            }
            Request::Produce(r) => self.produce(head.version, r).await,
            Request::Fetch(r) => self.fetch(head.version, r).await,
            Request::ListOffsets(r) => respond(Response::ListOffsets(
                self.list_offsets(head.version, &r).await,
            )),
            Request::InitProducerId(r) => respond(Response::InitProducerId(blocking::run(|| {
                self.init_producer_id(&r)
            }))),
            // Not served yet. Each moves out of this arm into a handler when its phase lands.
            Request::ApiVersions(_)
            | Request::OffsetCommit(_)
            | Request::OffsetFetch(_)
            | Request::FindCoordinator(_)
            | Request::JoinGroup(_)
            | Request::Heartbeat(_)
            | Request::LeaveGroup(_)
            | Request::SyncGroup(_)
            | Request::DescribeGroups(_)
            | Request::ListGroups(_)
            | Request::SaslHandshake(_)
            | Request::DeleteRecords(_)
            | Request::AddPartitionsToTxn(_)
            | Request::AddOffsetsToTxn(_)
            | Request::EndTxn(_)
            | Request::TxnOffsetCommit(_)
            | Request::DescribeAcls(_)
            | Request::CreateAcls(_)
            | Request::DeleteAcls(_)
            | Request::DescribeConfigs(_)
            | Request::AlterConfigs(_)
            | Request::SaslAuthenticate(_)
            | Request::CreatePartitions(_)
            | Request::DeleteGroups(_)
            | Request::IncrementalAlterConfigs(_)
            | Request::OffsetDelete(_)
            | Request::DescribeCluster(_)
            | Request::DescribeProducers(_)
            | Request::DescribeTransactions(_)
            | Request::ListTransactions(_)
            | Request::ConsumerGroupHeartbeat(_)
            | Request::ConsumerGroupDescribe(_)
            | Request::GetTelemetrySubscriptions(_)
            | Request::PushTelemetry(_) => Outcome::Close,
        }
    }
}

fn respond(response: Response) -> Outcome {
    Outcome::Respond(response)
}
