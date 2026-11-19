//! The one table of every API this broker serves.
//!
//! `served_apis!` is the only place an API is listed. From each line it generates the [`Request`] and
//! [`Response`] enums, their decode and encode, and the [`API_TABLE`] the version profiles read. Adding an
//! API is one line here, and a handler for the new `Request` variant: a `match` in the broker that forgets
//! it does not compile.
//!
//! A row's versions are what a real broker of each Kafka major (1, 2, 3, 4) offers, less what is not
//! implemented here, and never more (held to a recording of real brokers by a test). The Kafka 4 column
//! starts where Kafka 4 starts: it removed the oldest versions of nine APIs (KIP-896). A version Kafka marks unstable
//! (`unstable from` on its line) is in the column but offered only as Kafka offers it, with `unstable.api.versions.enable`
//! ([`Features::unstable`](crate::profile::Features::unstable)); the recording holds the rest.
//!
//! One floor is deliberate. Produce starts at v3 and Fetch at v4, the first versions that carry record
//! batch format v2, the only format the log stores. SaslHandshake v0 means "raw tokens follow" (KIP-43 framing, served:
//! SEC-401). The Kafka 4 column is Kafka 4.4.0's, every version of every API served (debt K1, closed 2026-11-19).

use bytes::{Bytes, BytesMut};
use kafka_protocol::messages::{
    AddOffsetsToTxnRequest, AddOffsetsToTxnResponse, AddPartitionsToTxnRequest,
    AddPartitionsToTxnResponse, AlterConfigsRequest, AlterConfigsResponse,
    AlterUserScramCredentialsRequest, AlterUserScramCredentialsResponse, ApiKey,
    ApiVersionsRequest, ApiVersionsResponse, ConsumerGroupDescribeRequest,
    ConsumerGroupDescribeResponse, ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse,
    CreateAclsRequest, CreateAclsResponse, CreatePartitionsRequest, CreatePartitionsResponse,
    CreateTopicsRequest, CreateTopicsResponse, DeleteAclsRequest, DeleteAclsResponse,
    DeleteGroupsRequest, DeleteGroupsResponse, DeleteRecordsRequest, DeleteRecordsResponse,
    DeleteTopicsRequest, DeleteTopicsResponse, DescribeAclsRequest, DescribeAclsResponse,
    DescribeClusterRequest, DescribeClusterResponse, DescribeConfigsRequest,
    DescribeConfigsResponse, DescribeGroupsRequest, DescribeGroupsResponse,
    DescribeProducersRequest, DescribeProducersResponse, DescribeTransactionsRequest,
    DescribeTransactionsResponse, DescribeUserScramCredentialsRequest,
    DescribeUserScramCredentialsResponse, EndTxnRequest, EndTxnResponse, FetchRequest,
    FetchResponse, FindCoordinatorRequest, FindCoordinatorResponse,
    GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse, HeartbeatRequest,
    HeartbeatResponse, IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse,
    InitProducerIdRequest, InitProducerIdResponse, JoinGroupRequest, JoinGroupResponse,
    LeaveGroupRequest, LeaveGroupResponse, ListGroupsRequest, ListGroupsResponse,
    ListOffsetsRequest, ListOffsetsResponse, ListTransactionsRequest, ListTransactionsResponse,
    MetadataRequest, MetadataResponse, OffsetCommitRequest, OffsetCommitResponse,
    OffsetDeleteRequest, OffsetDeleteResponse, OffsetFetchRequest, OffsetFetchResponse,
    ProduceRequest, ProduceResponse, PushTelemetryRequest, PushTelemetryResponse,
    SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest, SaslHandshakeResponse,
    SyncGroupRequest, SyncGroupResponse, TxnOffsetCommitRequest, TxnOffsetCommitResponse,
};
use kafka_protocol::messages::{
    AlterClientQuotasRequest, AlterClientQuotasResponse, AlterShareGroupOffsetsRequest,
    AlterShareGroupOffsetsResponse, DeleteShareGroupOffsetsRequest,
    DeleteShareGroupOffsetsResponse, DescribeClientQuotasRequest, DescribeClientQuotasResponse,
    DescribeLogDirsRequest, DescribeLogDirsResponse, DescribeShareGroupOffsetsRequest,
    DescribeShareGroupOffsetsResponse, OffsetForLeaderEpochRequest, OffsetForLeaderEpochResponse,
    ShareAcknowledgeRequest, ShareAcknowledgeResponse, ShareFetchRequest, ShareFetchResponse,
    ShareGroupDescribeRequest, ShareGroupDescribeResponse, ShareGroupHeartbeatRequest,
    ShareGroupHeartbeatResponse,
};
use kafka_protocol::messages::{
    StreamsGroupDescribeRequest, StreamsGroupDescribeResponse, StreamsGroupHeartbeatRequest,
    StreamsGroupHeartbeatResponse, StreamsGroupTopologyDescriptionUpdateRequest,
    StreamsGroupTopologyDescriptionUpdateResponse,
};
use kafka_protocol::protocol::{Decodable, Encodable};

use crate::error::WireError;
use crate::profile::{Condition, NONE, Offered, v};

mod throttle;

/// One row of [`API_TABLE`].
#[derive(Debug, Clone, Copy)]
pub struct ApiRow {
    /// The API.
    pub key: ApiKey,
    /// What each of Kafka 1, 2, 3 and 4 offers.
    pub offered: [Offered; 4],
    /// What else the API waits for.
    pub when: Condition,
    /// The first version Kafka marks unstable (`latestVersionUnstable` in its message spec), if one is: offered only
    /// with [`Features::unstable`](crate::profile::Features::unstable), as Kafka offers it only with
    /// `unstable.api.versions.enable`.
    pub unstable_from: Option<i16>,
}

macro_rules! when {
    () => {
        Condition::Always
    };
    ($c:ident) => {
        Condition::$c
    };
}

macro_rules! unstable_from {
    () => {
        None
    };
    ($v:literal) => {
        Some($v)
    };
}

/// The type a request or response is held as: itself, or in a `Box` for one much larger than the rest (`in Box` on its
/// line), so that [`Request`] and [`Response`], which every request in flight is, stay the size of the common ones.
macro_rules! held {
    ($req:ident) => {
        $req
    };
    ($req:ident, $wrap:ident) => {
        $wrap<$req>
    };
}

macro_rules! served_apis {
    ($( $key:ident($req:ident $(in $wrap:ident)?, $resp:ident $(in $rwrap:ident)?) $offered:expr $(, when $when:ident)? $(, unstable from $unstable:literal)? ;)*) => {
        /// A request to one of the served APIs, decoded.
        #[derive(Debug)]
        pub enum Request { $( #[doc = concat!("`", stringify!($key), "`")] $key(held!($req $(, $wrap)?)), )* }

        /// A response from one of the served APIs, ready to encode.
        #[derive(Debug)]
        pub enum Response { $( #[doc = concat!("`", stringify!($key), "`")] $key(held!($resp $(, $rwrap)?)), )* }

        /// Every served API, with the versions each Kafka major offers.
        pub const API_TABLE: &[ApiRow] = &[
            $( ApiRow { key: ApiKey::$key, offered: $offered, when: when!($($when)?), unstable_from: unstable_from!($($unstable)?) }, )*
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
                        .map(|r| Self::$key(r.into()))
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
                    $( Self::$key(r) => Encodable::encode(std::borrow::Borrow::<$resp>::borrow(r), out, version)
                        .map_err(|e| WireError::encode(stringify!($resp), e)), )*
                }
            }

            /// How many bytes [`Response::encode`] writes at `version` (the records are counted, not copied).
            ///
            /// # Errors
            /// A response that cannot be written at that version.
            pub fn compute_size(&self, version: i16) -> Result<usize, WireError> {
                match self {
                    $( Self::$key(r) => Encodable::compute_size(std::borrow::Borrow::<$resp>::borrow(r), version)
                        .map_err(|e| WireError::encode(stringify!($resp), e)), )*
                }
            }
        }
    };
}

served_apis! {
    // Produce v13, TxnOffsetCommit v6, OffsetCommit v10 and OffsetFetch v10 name topics by id (KIP-516, KIP-848).
    Produce(ProduceRequest, ProduceResponse) [v(3, 5), v(3, 7), v(3, 11), v(3, 13)];
    Fetch(FetchRequest, FetchResponse) [v(4, 6), v(4, 10), v(4, 13), v(4, 18)];
    ListOffsets(ListOffsetsRequest, ListOffsetsResponse) [v(0, 2), v(0, 5), v(0, 7), v(1, 11)];
    Metadata(MetadataRequest, MetadataResponse) [v(0, 5), v(0, 8), v(0, 12), v(0, 13)];
    OffsetCommit(OffsetCommitRequest, OffsetCommitResponse) [v(0, 3), v(0, 6), v(0, 8), v(2, 10)];
    OffsetFetch(OffsetFetchRequest, OffsetFetchResponse) [v(0, 3), v(0, 5), v(0, 8), v(1, 10)];
    FindCoordinator(FindCoordinatorRequest, FindCoordinatorResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 6)];
    JoinGroup(JoinGroupRequest, JoinGroupResponse) [v(0, 2), v(0, 4), v(0, 7), v(0, 9)];
    Heartbeat(HeartbeatRequest, HeartbeatResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 4)];
    LeaveGroup(LeaveGroupRequest, LeaveGroupResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 5)];
    SyncGroup(SyncGroupRequest, SyncGroupResponse) [v(0, 1), v(0, 3), v(0, 4), v(0, 5)];
    DescribeGroups(DescribeGroupsRequest, DescribeGroupsResponse) [v(0, 1), v(0, 3), v(0, 5), v(0, 6)];
    ListGroups(ListGroupsRequest, ListGroupsResponse) [v(0, 1), v(0, 2), v(0, 4), v(0, 5)];
    SaslHandshake(SaslHandshakeRequest, SaslHandshakeResponse) [v(0, 1), v(0, 1), v(0, 1), v(0, 1)], when Sasl;
    ApiVersions(ApiVersionsRequest, ApiVersionsResponse) [v(0, 1), v(0, 2), v(0, 3), v(0, 5)];
    CreateTopics(CreateTopicsRequest, CreateTopicsResponse) [v(0, 2), v(0, 3), v(0, 7), v(2, 7)];
    DeleteTopics(DeleteTopicsRequest, DeleteTopicsResponse) [v(0, 1), v(0, 3), v(0, 5), v(1, 6)];
    DeleteRecords(DeleteRecordsRequest, DeleteRecordsResponse) [v(0, 0), v(0, 1), v(0, 2), v(0, 2)];
    // InitProducerId v6 (KIP-939, two-phase commit) is unstable in Kafka 4.4.0: offered only as `unstable.api.versions.enable` offers it.
    InitProducerId(InitProducerIdRequest, InitProducerIdResponse) [v(0, 0), v(0, 1), v(0, 5), v(0, 6)], unstable from 6;
    AddPartitionsToTxn(AddPartitionsToTxnRequest, AddPartitionsToTxnResponse) [v(0, 0), v(0, 1), v(0, 5), v(0, 5)];
    AddOffsetsToTxn(AddOffsetsToTxnRequest, AddOffsetsToTxnResponse) [v(0, 0), v(0, 1), v(0, 4), v(0, 4)];
    EndTxn(EndTxnRequest, EndTxnResponse) [v(0, 0), v(0, 1), v(0, 4), v(0, 5)];
    TxnOffsetCommit(TxnOffsetCommitRequest, TxnOffsetCommitResponse) [v(0, 0), v(0, 2), v(0, 4), v(0, 6)];
    DescribeAcls(DescribeAclsRequest, DescribeAclsResponse) [v(0, 0), v(0, 1), v(0, 3), v(1, 3)];
    CreateAcls(CreateAclsRequest, CreateAclsResponse) [v(0, 0), v(0, 1), v(0, 3), v(1, 3)];
    DeleteAcls(DeleteAclsRequest, DeleteAclsResponse) [v(0, 0), v(0, 1), v(0, 3), v(1, 3)];
    DescribeConfigs(DescribeConfigsRequest, DescribeConfigsResponse) [v(0, 1), v(0, 2), v(0, 4), v(1, 4)];
    AlterConfigs(AlterConfigsRequest, AlterConfigsResponse) [v(0, 0), v(0, 2), v(0, 2), v(0, 2)];
    SaslAuthenticate(SaslAuthenticateRequest, SaslAuthenticateResponse) [v(0, 0), v(0, 2), v(0, 2), v(0, 2)], when Sasl;
    CreatePartitions(CreatePartitionsRequest, CreatePartitionsResponse) [v(0, 0), v(0, 1), v(0, 3), v(0, 3)];
    DeleteGroups(DeleteGroupsRequest, DeleteGroupsResponse) [v(0, 0), v(0, 2), v(0, 2), v(0, 3)];
    IncrementalAlterConfigs(IncrementalAlterConfigsRequest, IncrementalAlterConfigsResponse) [NONE, v(0, 1), v(0, 1), v(0, 1)];
    OffsetDelete(OffsetDeleteRequest, OffsetDeleteResponse) [NONE, v(0, 0), v(0, 0), v(0, 0)];
    DescribeCluster(DescribeClusterRequest, DescribeClusterResponse) [NONE, NONE, v(0, 1), v(0, 2)];
    DescribeProducers(DescribeProducersRequest, DescribeProducersResponse) [NONE, NONE, v(0, 0), v(0, 0)];
    DescribeTransactions(DescribeTransactionsRequest, DescribeTransactionsResponse) [NONE, NONE, v(0, 0), v(0, 0)];
    ListTransactions(ListTransactionsRequest, ListTransactionsResponse) [NONE, NONE, v(0, 1), v(0, 2)];
    ConsumerGroupHeartbeat(ConsumerGroupHeartbeatRequest, ConsumerGroupHeartbeatResponse) [NONE, NONE, NONE, v(0, 1)];
    ConsumerGroupDescribe(ConsumerGroupDescribeRequest, ConsumerGroupDescribeResponse) [NONE, NONE, NONE, v(0, 1)];
    DescribeUserScramCredentials(DescribeUserScramCredentialsRequest, DescribeUserScramCredentialsResponse) [NONE, v(0, 0), v(0, 0), v(0, 0)];
    AlterUserScramCredentials(AlterUserScramCredentialsRequest, AlterUserScramCredentialsResponse) [NONE, v(0, 0), v(0, 0), v(0, 0)];
    OffsetForLeaderEpoch(OffsetForLeaderEpochRequest, OffsetForLeaderEpochResponse) [v(0, 0), v(0, 4), v(0, 4), v(2, 4)];
    DescribeLogDirs(DescribeLogDirsRequest, DescribeLogDirsResponse) [v(0, 0), v(0, 2), v(0, 4), v(1, 5)];
    DescribeClientQuotas(DescribeClientQuotasRequest, DescribeClientQuotasResponse) [NONE, v(0, 1), v(0, 1), v(0, 1)];
    AlterClientQuotas(AlterClientQuotasRequest, AlterClientQuotasResponse) [NONE, v(0, 1), v(0, 1), v(0, 1)];
    GetTelemetrySubscriptions(GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse) [NONE, NONE, NONE, v(0, 0)], when Telemetry;
    PushTelemetry(PushTelemetryRequest, PushTelemetryResponse) [NONE, NONE, NONE, v(0, 0)], when Telemetry;
    // Share groups (KIP-932): Kafka 4.1 made them generally available, at the versions 4.4.0 serves.
    ShareGroupHeartbeat(ShareGroupHeartbeatRequest, ShareGroupHeartbeatResponse) [NONE, NONE, NONE, v(1, 1)];
    ShareGroupDescribe(ShareGroupDescribeRequest, ShareGroupDescribeResponse) [NONE, NONE, NONE, v(1, 1)];
    ShareFetch(ShareFetchRequest, ShareFetchResponse) [NONE, NONE, NONE, v(1, 2)];
    ShareAcknowledge(ShareAcknowledgeRequest, ShareAcknowledgeResponse) [NONE, NONE, NONE, v(1, 2)];
    DescribeShareGroupOffsets(DescribeShareGroupOffsetsRequest, DescribeShareGroupOffsetsResponse) [NONE, NONE, NONE, v(0, 1)];
    AlterShareGroupOffsets(AlterShareGroupOffsetsRequest, AlterShareGroupOffsetsResponse) [NONE, NONE, NONE, v(0, 0)];
    DeleteShareGroupOffsets(DeleteShareGroupOffsetsRequest, DeleteShareGroupOffsetsResponse) [NONE, NONE, NONE, v(0, 0)];
    // Streams groups (KIP-1071), at the versions 4.4.0 serves; 93 is answered as a 4.4.0 broker with no topology
    // description plugin answers it (KIP-1331).
    StreamsGroupHeartbeat(StreamsGroupHeartbeatRequest in Box, StreamsGroupHeartbeatResponse in Box) [NONE, NONE, NONE, v(0, 1)];
    StreamsGroupDescribe(StreamsGroupDescribeRequest, StreamsGroupDescribeResponse) [NONE, NONE, NONE, v(0, 1)];
    StreamsGroupTopologyDescriptionUpdate(StreamsGroupTopologyDescriptionUpdateRequest, StreamsGroupTopologyDescriptionUpdateResponse) [NONE, NONE, NONE, v(0, 0)];
}
