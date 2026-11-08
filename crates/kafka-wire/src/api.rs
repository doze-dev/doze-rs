//! The one table of every API this broker serves.
//!
//! `served_apis!` is the only place an API is listed. From each line it generates the [`Request`] and
//! [`Response`] enums, their decode and encode, and the [`API_TABLE`] the version profiles read. Adding an
//! API is one line here, and a handler for the new `Request` variant: a `match` in the broker that forgets
//! it does not compile.
//!
//! A row's versions are what a real broker of each Kafka major (1, 2, 3, 4) offers, less what is not
//! implemented here, and never more (held to a recording of real brokers by a test). The Kafka 4 column
//! starts where Kafka 4 starts: it removed the oldest versions of nine APIs (KIP-896).
//!
//! Two floors are deliberate. Produce starts at v3 and Fetch at v4, the first versions that carry record
//! batch format v2, the only format the log stores. OffsetCommit and OffsetFetch stop below v10, where the
//! wire form switches to topic ids. SaslHandshake is v1 only: v0 means "raw tokens follow".

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::{
    AddOffsetsToTxnRequest, AddOffsetsToTxnResponse, AddPartitionsToTxnRequest,
    AddPartitionsToTxnResponse, AlterConfigsRequest, AlterConfigsResponse, ApiKey,
    ApiVersionsRequest, ApiVersionsResponse, ConsumerGroupDescribeRequest,
    ConsumerGroupDescribeResponse, ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse,
    CreateAclsRequest, CreateAclsResponse, CreatePartitionsRequest, CreatePartitionsResponse,
    CreateTopicsRequest, CreateTopicsResponse, DeleteAclsRequest, DeleteAclsResponse,
    DeleteGroupsRequest, DeleteGroupsResponse, DeleteRecordsRequest, DeleteRecordsResponse,
    DeleteTopicsRequest, DeleteTopicsResponse, DescribeAclsRequest, DescribeAclsResponse,
    DescribeClusterRequest, DescribeClusterResponse, DescribeConfigsRequest,
    DescribeConfigsResponse, DescribeGroupsRequest, DescribeGroupsResponse,
    DescribeProducersRequest, DescribeProducersResponse, DescribeTransactionsRequest,
    DescribeTransactionsResponse, EndTxnRequest, EndTxnResponse, FetchRequest, FetchResponse,
    FindCoordinatorRequest, FindCoordinatorResponse, GetTelemetrySubscriptionsRequest,
    GetTelemetrySubscriptionsResponse, HeartbeatRequest, HeartbeatResponse,
    IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse, InitProducerIdRequest,
    InitProducerIdResponse, JoinGroupRequest, JoinGroupResponse, LeaveGroupRequest,
    LeaveGroupResponse, ListGroupsRequest, ListGroupsResponse, ListOffsetsRequest,
    ListOffsetsResponse, ListTransactionsRequest, ListTransactionsResponse, MetadataRequest,
    MetadataResponse, OffsetCommitRequest, OffsetCommitResponse, OffsetDeleteRequest,
    OffsetDeleteResponse, OffsetFetchRequest, OffsetFetchResponse, ProduceRequest, ProduceResponse,
    PushTelemetryRequest, PushTelemetryResponse, SaslAuthenticateRequest, SaslAuthenticateResponse,
    SaslHandshakeRequest, SaslHandshakeResponse, SyncGroupRequest, SyncGroupResponse,
    TxnOffsetCommitRequest, TxnOffsetCommitResponse,
};
use kafka_protocol::protocol::{Decodable, Encodable};

use crate::error::WireError;
use crate::profile::{Condition, NONE, Offered, v};

/// One row of [`API_TABLE`].
#[derive(Debug, Clone, Copy)]
pub struct ApiRow {
    /// The API.
    pub key: ApiKey,
    /// What each of Kafka 1, 2, 3 and 4 offers.
    pub offered: [Offered; 4],
    /// What else the API waits for.
    pub when: Condition,
}

macro_rules! when {
    () => {
        Condition::Always
    };
    ($c:ident) => {
        Condition::$c
    };
}

macro_rules! served_apis {
    ($( $key:ident($req:ident, $resp:ident) $offered:expr $(, when $when:ident)? ;)*) => {
        /// A request to one of the served APIs, decoded.
        #[derive(Debug)]
        pub enum Request { $( #[doc = concat!("`", stringify!($key), "`")] $key($req), )* }

        /// A response from one of the served APIs, ready to encode.
        #[derive(Debug)]
        pub enum Response { $( #[doc = concat!("`", stringify!($key), "`")] $key($resp), )* }

        /// Every served API, with the versions each Kafka major offers.
        pub const API_TABLE: &[ApiRow] = &[
            $( ApiRow { key: ApiKey::$key, offered: $offered, when: when!($($when)?) }, )*
        ];

        impl Request {
            /// Which API this request is to.
            #[must_use]
            pub fn api_key(&self) -> ApiKey {
                match self { $( Self::$key(_) => ApiKey::$key, )* }
            }

            /// Decodes a request body of `version`.
            ///
            /// # Errors
            /// A key this broker does not serve, or a body that does not decode.
            pub fn decode(key: ApiKey, version: i16, body: &mut Bytes) -> Result<Self, WireError> {
                match key {
                    $( ApiKey::$key => <$req as Decodable>::decode(body, version)
                        .map(Self::$key)
                        .map_err(|e| WireError::decode(stringify!($req), e)), )*
                    other => Err(WireError::NotServed(other)),
                }
            }
        }

        impl Response {
            /// Which API this response is from.
            #[must_use]
            pub fn api_key(&self) -> ApiKey {
                match self { $( Self::$key(_) => ApiKey::$key, )* }
            }

            /// Appends the encoded body at `version` to `out`.
            ///
            /// # Errors
            /// A response that cannot be written at that version.
            pub fn encode(&self, version: i16, out: &mut BytesMut) -> Result<(), WireError> {
                match self {
                    $( Self::$key(r) => Encodable::encode(r, out, version)
                        .map_err(|e| WireError::encode(stringify!($resp), e)), )*
                }
            }
        }
    };
}

served_apis! {
    Produce(ProduceRequest, ProduceResponse) [v(3, 5), v(3, 7), v(3, 9), v(3, 9)];
    Fetch(FetchRequest, FetchResponse) [v(4, 6), v(4, 10), v(4, 13), v(4, 13)];
    ListOffsets(ListOffsetsRequest, ListOffsetsResponse) [v(0, 2), v(0, 5), v(0, 7), v(1, 7)];
    Metadata(MetadataRequest, MetadataResponse) [v(0, 5), v(0, 8), v(0, 12), v(0, 12)];
    OffsetCommit(OffsetCommitRequest, OffsetCommitResponse) [v(0, 3), v(0, 6), v(0, 8), v(2, 9)];
    OffsetFetch(OffsetFetchRequest, OffsetFetchResponse) [v(0, 3), v(0, 5), v(0, 8), v(1, 9)];
    FindCoordinator(FindCoordinatorRequest, FindCoordinatorResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 4)];
    JoinGroup(JoinGroupRequest, JoinGroupResponse) [v(0, 2), v(0, 4), v(0, 7), v(0, 9)];
    Heartbeat(HeartbeatRequest, HeartbeatResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 4)];
    LeaveGroup(LeaveGroupRequest, LeaveGroupResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 5)];
    SyncGroup(SyncGroupRequest, SyncGroupResponse) [v(0, 1), v(0, 3), v(0, 4), v(0, 5)];
    DescribeGroups(DescribeGroupsRequest, DescribeGroupsResponse) [v(0, 1), v(0, 3), v(0, 5), v(0, 5)];
    ListGroups(ListGroupsRequest, ListGroupsResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 5)];
    SaslHandshake(SaslHandshakeRequest, SaslHandshakeResponse) [v(1, 1), v(1, 1), v(1, 1), v(1, 1)], when Sasl;
    ApiVersions(ApiVersionsRequest, ApiVersionsResponse) [v(0, 1), v(0, 2), v(0, 3), v(0, 4)];
    CreateTopics(CreateTopicsRequest, CreateTopicsResponse) [v(0, 2), v(0, 3), v(0, 7), v(2, 7)];
    DeleteTopics(DeleteTopicsRequest, DeleteTopicsResponse) [v(0, 1), v(0, 3), v(0, 5), v(1, 6)];
    DeleteRecords(DeleteRecordsRequest, DeleteRecordsResponse) [v(0, 0), v(0, 1), v(0, 2), v(0, 2)];
    InitProducerId(InitProducerIdRequest, InitProducerIdResponse) [v(0, 0), v(0, 1), v(0, 4), v(0, 4)];
    AddPartitionsToTxn(AddPartitionsToTxnRequest, AddPartitionsToTxnResponse) [v(0, 0), v(0, 1), v(0, 3), v(0, 3)];
    AddOffsetsToTxn(AddOffsetsToTxnRequest, AddOffsetsToTxnResponse) [v(0, 0), v(0, 1), v(0, 3), v(0, 3)];
    EndTxn(EndTxnRequest, EndTxnResponse) [v(0, 0), v(0, 1), v(0, 3), v(0, 3)];
    TxnOffsetCommit(TxnOffsetCommitRequest, TxnOffsetCommitResponse) [v(0, 0), v(0, 2), v(0, 3), v(0, 3)];
    DescribeAcls(DescribeAclsRequest, DescribeAclsResponse) [v(0, 0), v(0, 1), v(0, 3), v(1, 3)];
    CreateAcls(CreateAclsRequest, CreateAclsResponse) [v(0, 0), v(0, 1), v(0, 3), v(1, 3)];
    DeleteAcls(DeleteAclsRequest, DeleteAclsResponse) [v(0, 0), v(0, 1), v(0, 3), v(1, 3)];
    DescribeConfigs(DescribeConfigsRequest, DescribeConfigsResponse) [v(0, 1), v(0, 2), v(0, 4), v(1, 4)];
    AlterConfigs(AlterConfigsRequest, AlterConfigsResponse) [v(0, 0), v(0, 2), v(0, 2), v(0, 2)];
    SaslAuthenticate(SaslAuthenticateRequest, SaslAuthenticateResponse) [v(0, 0), v(0, 2), v(0, 2), v(0, 2)], when Sasl;
    CreatePartitions(CreatePartitionsRequest, CreatePartitionsResponse) [v(0, 0), v(0, 1), v(0, 3), v(0, 3)];
    DeleteGroups(DeleteGroupsRequest, DeleteGroupsResponse) [v(0, 0), v(0, 2), v(0, 2), v(0, 2)];
    IncrementalAlterConfigs(IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse) [NONE, v(0, 1), v(0, 1), v(0, 1)];
    OffsetDelete(OffsetDeleteRequest, OffsetDeleteResponse) [NONE, v(0, 0), v(0, 0), v(0, 0)];
    DescribeCluster(DescribeClusterRequest, DescribeClusterResponse) [NONE, NONE, v(0, 1), v(0, 2)];
    DescribeProducers(DescribeProducersRequest, DescribeProducersResponse) [NONE, NONE, v(0, 0), v(0, 0)];
    DescribeTransactions(DescribeTransactionsRequest, DescribeTransactionsResponse) [NONE, NONE, v(0, 0), v(0, 0)];
    ListTransactions(ListTransactionsRequest, ListTransactionsResponse) [NONE, NONE, v(0, 0), v(0, 0)];
    ConsumerGroupHeartbeat(ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse) [NONE, NONE, NONE, v(0, 1)];
    ConsumerGroupDescribe(ConsumerGroupDescribeRequest, ConsumerGroupDescribeResponse) [NONE, NONE, NONE, v(0, 1)];
    GetTelemetrySubscriptions(GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse) [NONE, NONE, NONE, v(0, 0)], when Telemetry;
    PushTelemetry(PushTelemetryRequest, PushTelemetryResponse) [NONE, NONE, NONE, v(0, 0)], when Telemetry;
}

#[cfg(test)]
mod tests;
