//! The broker: one decoded request in, one outcome out.

use kafka_wire::api::Request;
use kafka_wire::header::RequestHead;
use kafka_wire::messages::ApiKey;
use kafka_wire::profile::ApiSet;

use crate::Outcome;

/// The APIs this broker has a handler for. ApiVersions advertises exactly these (and only those the Kafka
/// version being stood for offers), and a request for any other is not accepted. A phase adds the APIs it
/// serves here and a handler for each in [`Broker::handle`]; the `match` there has no wildcard, so a new API
/// that is neither handled nor listed as unserved does not compile.
const SERVED: &[ApiKey] = &[ApiKey::ApiVersions];

/// The broker. (State arrives with the handlers that need it.)
#[derive(Debug, Default)]
pub struct Broker {}

impl Broker {
    /// A broker.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }

    /// The APIs this broker serves.
    #[must_use]
    pub fn served(&self) -> ApiSet {
        ApiSet::of(SERVED)
    }

    /// Handles one request. ApiVersions never gets here: the connection answers it from the version profile,
    /// before the connection is known to be allowed anything else.
    pub async fn handle(&self, _head: &RequestHead, request: Request) -> Outcome {
        match request {
            // Not served yet. Each moves out of this arm into a handler when its phase lands.
            Request::ApiVersions(_)
            | Request::Produce(_)
            | Request::Fetch(_)
            | Request::ListOffsets(_)
            | Request::Metadata(_)
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
            | Request::CreateTopics(_)
            | Request::DeleteTopics(_)
            | Request::DeleteRecords(_)
            | Request::InitProducerId(_)
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
