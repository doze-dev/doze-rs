//! One request in, one outcome out: the exhaustive match over every API.

use kafka_storage::fs::Fs;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;

use super::Broker;
use crate::{Caller, Listener, Outcome, blocking};

impl<F: Fs> Broker<F> {
    /// Handles one request from `caller`. ApiVersions never gets here: the connection answers it from the version
    /// profile, before the connection is known to be allowed anything else.
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "an exhaustive match over every API, with no wildcard, so that an API with no handler does not compile"
    )]
    pub async fn handle(
        &self,
        listener: &Listener,
        caller: &Caller,
        head: &RequestHead,
        request: Request,
    ) -> Outcome {
        match request {
            Request::Metadata(r) => {
                let (answer, wanted) =
                    blocking::run(|| self.metadata(listener, caller, &r, head.version));
                self.create_wanted(wanted).await;
                respond(Response::Metadata(answer))
            }
            Request::CreateTopics(r) => {
                respond(Response::CreateTopics(self.create_topics(caller, &r).await))
            }
            Request::DeleteTopics(r) => {
                respond(Response::DeleteTopics(self.delete_topics(caller, &r).await))
            }
            Request::Produce(r) => self.produce(head.version, caller, r).await,
            Request::Fetch(r) => self.fetch(head.version, caller, r).await,
            Request::ListOffsets(r) => respond(Response::ListOffsets(
                self.list_offsets(head.version, caller, &r).await,
            )),
            Request::FindCoordinator(r) => respond(Response::FindCoordinator(
                self.find_coordinator(head.version, listener, caller, &r),
            )),
            Request::JoinGroup(r) => {
                self.join_group(head.version, caller, &client_of(listener, head), &r)
                    .await
            }
            Request::SyncGroup(r) => self.sync_group(head.version, caller, &r).await,
            Request::Heartbeat(r) => self.heartbeat(caller, &r).await,
            Request::LeaveGroup(r) => self.leave_group(head.version, caller, &r).await,
            Request::OffsetCommit(r) => self.offset_commit(head.version, caller, &r).await,
            Request::OffsetFetch(r) => self.offset_fetch(head.version, caller, &r).await,
            Request::OffsetDelete(r) => self.offset_delete(caller, &r).await,
            Request::DescribeGroups(r) => self.describe_groups(head.version, caller, &r).await,
            Request::ListGroups(r) => self.list_groups(caller, &r).await,
            Request::DeleteGroups(r) => self.delete_groups(caller, &r).await,
            Request::ConsumerGroupHeartbeat(r) => {
                self.consumer_group_heartbeat(head.version, caller, &client_of(listener, head), &r)
                    .await
            }
            Request::ConsumerGroupDescribe(r) => self.consumer_group_describe(caller, &r).await,
            Request::DeleteRecords(r) => self.delete_records(caller, &r).await,
            Request::CreatePartitions(r) => self.create_partitions(caller, &r).await,
            Request::DescribeConfigs(r) => self.describe_configs(caller, &r),
            Request::AlterConfigs(r) => self.alter_configs(caller, &r).await,
            Request::IncrementalAlterConfigs(r) => self.incremental_alter_configs(caller, &r).await,
            Request::DescribeProducers(r) => self.describe_producers(caller, &r).await,
            Request::InitProducerId(r) => respond(Response::InitProducerId(
                self.init_producer_id(head.version, caller, &r).await,
            )),
            Request::AddPartitionsToTxn(r) => {
                self.add_partitions_to_txn(head.version, caller, &r).await
            }
            Request::AddOffsetsToTxn(r) => self.add_offsets_to_txn(head.version, caller, &r).await,
            Request::EndTxn(r) => self.end_txn_request(head.version, caller, &r).await,
            Request::TxnOffsetCommit(r) => self.txn_offset_commit(caller, &r).await,
            Request::DescribeTransactions(r) => self.describe_transactions(caller, &r).await,
            Request::ListTransactions(r) => self.list_transactions(caller, &r).await,
            Request::DescribeAcls(r) => self.describe_acls(caller, &r),
            Request::CreateAcls(r) => self.create_acls(caller, &r).await,
            Request::DeleteAcls(r) => self.delete_acls(caller, &r).await,
            Request::DescribeUserScramCredentials(r) => {
                self.describe_user_scram_credentials(caller, &r)
            }
            Request::AlterUserScramCredentials(r) => {
                self.alter_user_scram_credentials(caller, &r).await
            }
            // Not served yet (or answered by the connection). Each moves out of this arm into a handler when its
            // phase lands.
            Request::ApiVersions(_)
            | Request::SaslHandshake(_)
            | Request::SaslAuthenticate(_)
            | Request::DescribeCluster(_)
            | Request::GetTelemetrySubscriptions(_)
            | Request::PushTelemetry(_) => Outcome::Close,
        }
    }
}

fn respond(response: Response) -> Outcome {
    Outcome::Respond(response)
}

/// The client of a request: its own name, and where it connected from.
fn client_of<'a>(listener: &'a Listener, head: &'a RequestHead) -> crate::handlers::Client<'a> {
    crate::handlers::Client {
        id: head.client_id.as_deref().unwrap_or(""),
        host: &listener.client_host,
    }
}
