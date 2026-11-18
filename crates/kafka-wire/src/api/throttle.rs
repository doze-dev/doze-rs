//! The throttle time a response tells its client, whichever API it answers.
//!
//! Kafka's `AbstractResponse.maybeSetThrottleTimeMs`: every response with a `ThrottleTimeMs` field carries the time the
//! broker throttled the request for. The field is ignorable at the versions that lack it (it is simply not written), so
//! setting it is safe at any version.

use super::Response;

impl Response {
    /// Sets the throttle time, in milliseconds, on a response that has the field; the two SASL answers have none.
    pub fn set_throttle_time_ms(&mut self, ms: i32) {
        let field = match self {
            Self::Produce(r) => &mut r.throttle_time_ms,
            Self::Fetch(r) => &mut r.throttle_time_ms,
            Self::ListOffsets(r) => &mut r.throttle_time_ms,
            Self::Metadata(r) => &mut r.throttle_time_ms,
            Self::OffsetCommit(r) => &mut r.throttle_time_ms,
            Self::OffsetFetch(r) => &mut r.throttle_time_ms,
            Self::FindCoordinator(r) => &mut r.throttle_time_ms,
            Self::JoinGroup(r) => &mut r.throttle_time_ms,
            Self::Heartbeat(r) => &mut r.throttle_time_ms,
            Self::LeaveGroup(r) => &mut r.throttle_time_ms,
            Self::SyncGroup(r) => &mut r.throttle_time_ms,
            Self::DescribeGroups(r) => &mut r.throttle_time_ms,
            Self::ListGroups(r) => &mut r.throttle_time_ms,
            Self::ApiVersions(r) => &mut r.throttle_time_ms,
            Self::CreateTopics(r) => &mut r.throttle_time_ms,
            Self::DeleteTopics(r) => &mut r.throttle_time_ms,
            Self::DeleteRecords(r) => &mut r.throttle_time_ms,
            Self::InitProducerId(r) => &mut r.throttle_time_ms,
            Self::AddPartitionsToTxn(r) => &mut r.throttle_time_ms,
            Self::AddOffsetsToTxn(r) => &mut r.throttle_time_ms,
            Self::EndTxn(r) => &mut r.throttle_time_ms,
            Self::TxnOffsetCommit(r) => &mut r.throttle_time_ms,
            Self::DescribeAcls(r) => &mut r.throttle_time_ms,
            Self::CreateAcls(r) => &mut r.throttle_time_ms,
            Self::DeleteAcls(r) => &mut r.throttle_time_ms,
            Self::DescribeConfigs(r) => &mut r.throttle_time_ms,
            Self::AlterConfigs(r) => &mut r.throttle_time_ms,
            Self::CreatePartitions(r) => &mut r.throttle_time_ms,
            Self::DeleteGroups(r) => &mut r.throttle_time_ms,
            Self::IncrementalAlterConfigs(r) => &mut r.throttle_time_ms,
            Self::OffsetDelete(r) => &mut r.throttle_time_ms,
            Self::DescribeCluster(r) => &mut r.throttle_time_ms,
            Self::DescribeProducers(r) => &mut r.throttle_time_ms,
            Self::DescribeTransactions(r) => &mut r.throttle_time_ms,
            Self::ListTransactions(r) => &mut r.throttle_time_ms,
            Self::ConsumerGroupHeartbeat(r) => &mut r.throttle_time_ms,
            Self::ConsumerGroupDescribe(r) => &mut r.throttle_time_ms,
            Self::DescribeUserScramCredentials(r) => &mut r.throttle_time_ms,
            Self::AlterUserScramCredentials(r) => &mut r.throttle_time_ms,
            Self::OffsetForLeaderEpoch(r) => &mut r.throttle_time_ms,
            Self::DescribeLogDirs(r) => &mut r.throttle_time_ms,
            Self::DescribeClientQuotas(r) => &mut r.throttle_time_ms,
            Self::AlterClientQuotas(r) => &mut r.throttle_time_ms,
            Self::GetTelemetrySubscriptions(r) => &mut r.throttle_time_ms,
            Self::PushTelemetry(r) => &mut r.throttle_time_ms,
            Self::ShareGroupHeartbeat(r) => &mut r.throttle_time_ms,
            Self::ShareGroupDescribe(r) => &mut r.throttle_time_ms,
            Self::ShareFetch(r) => &mut r.throttle_time_ms,
            Self::ShareAcknowledge(r) => &mut r.throttle_time_ms,
            Self::DescribeShareGroupOffsets(r) => &mut r.throttle_time_ms,
            Self::AlterShareGroupOffsets(r) => &mut r.throttle_time_ms,
            Self::DeleteShareGroupOffsets(r) => &mut r.throttle_time_ms,
            Self::SaslHandshake(_) | Self::SaslAuthenticate(_) => return,
        };
        *field = ms;
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use kafka_protocol::messages::{MetadataResponse, ProduceResponse};

    use crate::api::Response;

    #[test]
    fn the_throttle_is_written_where_the_version_has_it_and_left_out_where_it_has_not() {
        let mut meta = Response::Metadata(MetadataResponse::default());
        meta.set_throttle_time_ms(250);
        let mut v0 = BytesMut::new();
        meta.encode(0, &mut v0).unwrap();
        let mut v3 = BytesMut::new();
        meta.encode(3, &mut v3).unwrap();
        // v3 is v0 with the throttle in front (and the cluster id and controller, which are empty here).
        assert_eq!(&v3[..4], &250_i32.to_be_bytes());
        assert!(v0.len() < v3.len());
        let mut produce = Response::Produce(ProduceResponse::default());
        produce.set_throttle_time_ms(7);
        let Response::Produce(p) = produce else {
            unreachable!()
        };
        assert_eq!(p.throttle_time_ms, 7);
    }
}
