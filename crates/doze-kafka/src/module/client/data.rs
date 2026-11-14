//! The data side of the client: offsets, consumer groups and their commits, one record written, records read. What the
//! console's numbers, the produce action and the tail are made of.

use std::collections::BTreeMap;

use doze_module::Result;
use doze_module::proto::msg::error::{Error, ErrorKind};
use kafka_protocol::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_protocol::messages::list_offsets_request::{ListOffsetsPartition, ListOffsetsTopic};
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::{
    ApiKey, DescribeGroupsRequest, DescribeGroupsResponse, FetchRequest, FetchResponse, GroupId,
    ListGroupsRequest, ListGroupsResponse, ListOffsetsRequest, ListOffsetsResponse,
    OffsetFetchRequest, OffsetFetchResponse, ProduceRequest, ProduceResponse,
};

use super::{
    Client, FETCH_BYTES, FETCH_WAIT, Fetched, GroupInfo, MOST, TopicInfo, refused, text,
    topic_name, unavailable,
};

impl Client {
    /// Each partition's offset at `timestamp` (-1: the end, the next offset to be written; -2: the earliest kept).
    pub(crate) async fn offsets(
        &mut self,
        topics: &[TopicInfo],
        timestamp: i64,
    ) -> Result<BTreeMap<(String, i32), i64>> {
        let mut q = ListOffsetsRequest::default();
        q.replica_id = (-1).into();
        for t in topics {
            let mut lt = ListOffsetsTopic::default();
            lt.name = topic_name(&t.name);
            for p in &t.partitions {
                let mut lp = ListOffsetsPartition::default();
                lp.partition_index = *p;
                lp.timestamp = timestamp;
                lt.partitions.push(lp);
            }
            q.topics.push(lt);
        }
        if q.topics.is_empty() {
            return Ok(BTreeMap::new());
        }
        let a: ListOffsetsResponse = self.call(ApiKey::ListOffsets, 1, &q).await?;
        let mut out = BTreeMap::new();
        for t in a.topics {
            for p in t.partitions.into_iter().filter(|p| p.error_code == 0) {
                out.insert((t.name.0.to_string(), p.partition_index), p.offset);
            }
        }
        Ok(out)
    }

    /// The consumer groups, described.
    pub(crate) async fn groups(&mut self) -> Result<Vec<GroupInfo>> {
        let listed: ListGroupsResponse = self
            .call(ApiKey::ListGroups, 0, &ListGroupsRequest::default())
            .await?;
        let mut q = DescribeGroupsRequest::default();
        q.groups = listed.groups.into_iter().map(|g| g.group_id).collect();
        if q.groups.is_empty() {
            return Ok(Vec::new());
        }
        let a: DescribeGroupsResponse = self.call(ApiKey::DescribeGroups, 0, &q).await?;
        let mut out: Vec<GroupInfo> = a
            .groups
            .into_iter()
            .filter(|g| g.error_code == 0)
            .map(|g| GroupInfo {
                name: g.group_id.0.to_string(),
                state: g.group_state.to_string(),
                members: g.members.len(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// A group's committed offsets, by topic and partition.
    pub(crate) async fn committed(&mut self, group: &str) -> Result<BTreeMap<(String, i32), i64>> {
        let mut q = OffsetFetchRequest::default();
        q.group_id = GroupId(text(group));
        q.topics = None;
        let a: OffsetFetchResponse = self.call(ApiKey::OffsetFetch, 2, &q).await?;
        let mut out = BTreeMap::new();
        for t in a.topics {
            for p in t.partitions {
                if p.error_code == 0 && p.committed_offset >= 0 {
                    out.insert(
                        (t.name.0.to_string(), p.partition_index),
                        p.committed_offset,
                    );
                }
            }
        }
        Ok(out)
    }

    /// Writes one record to a partition and waits for it to be stored; its offset.
    pub(crate) async fn produce(
        &mut self,
        topic: &str,
        partition: i32,
        value: &[u8],
    ) -> Result<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let record = kafka_wire::batch::OneRecord {
            timestamp_ms: now,
            key: None,
            value: Some(value),
            headers: &[],
        };
        let mut p = PartitionProduceData::default();
        p.index = partition;
        p.records = Some(kafka_wire::batch::single_record(&record));
        let mut t = TopicProduceData::default();
        t.name = topic_name(topic);
        t.partition_data.push(p);
        let mut q = ProduceRequest::default();
        q.acks = -1;
        q.timeout_ms = 30_000;
        q.topic_data.push(t);
        let a: ProduceResponse = self.call(ApiKey::Produce, 3, &q).await?;
        let r = a
            .responses
            .first()
            .and_then(|t| t.partition_responses.first())
            .ok_or_else(|| Error::new(ErrorKind::Internal, "Produce answered nothing"))?;
        if r.error_code != 0 {
            return Err(refused(
                topic,
                "the message was not written",
                r.error_code,
                r.error_message.as_ref(),
            ));
        }
        Ok(r.base_offset)
    }

    /// The records from `from` (by partition) on, waiting up to `FETCH_WAIT` for some to arrive; `from` is moved past
    /// what was read.
    pub(crate) async fn fetch(
        &mut self,
        topic: &str,
        from: &mut BTreeMap<i32, i64>,
    ) -> Result<Vec<Fetched>> {
        let mut t = FetchTopic::default();
        t.topic = topic_name(topic);
        for (p, offset) in from.iter() {
            let mut fp = FetchPartition::default();
            fp.partition = *p;
            fp.fetch_offset = *offset;
            fp.partition_max_bytes = FETCH_BYTES;
            t.partitions.push(fp);
        }
        let mut q = FetchRequest::default();
        q.replica_id = (-1).into();
        q.max_wait_ms = i32::try_from(FETCH_WAIT.as_millis()).unwrap_or(500);
        q.min_bytes = 1;
        q.max_bytes = FETCH_BYTES;
        q.topics.push(t);
        let a: FetchResponse = self.call(ApiKey::Fetch, 4, &q).await?;
        let mut out = Vec::new();
        for p in a.responses.into_iter().flat_map(|t| t.partitions) {
            if p.error_code != 0 {
                return Err(refused(
                    topic,
                    &format!("partition {} cannot be read", p.partition_index),
                    p.error_code,
                    None,
                ));
            }
            let Some(records) = p.records else { continue };
            let start = from.get(&p.partition_index).copied().unwrap_or(0);
            let mut next = start;
            // A fetch may end with a batch cut short (it hit the size asked for): what came whole is read, the rest
            // is fetched again next time.
            for batch in kafka_wire::batch::Batches::new(records).map_while(std::result::Result::ok)
            {
                let looked = batch.each_record_full(MOST as u64, |r| {
                    if r.offset >= start {
                        out.push(Fetched {
                            partition: p.partition_index,
                            offset: r.offset,
                            timestamp: r.timestamp,
                            key: r.key.unwrap_or_default().to_vec(),
                            value: r.value.unwrap_or_default().to_vec(),
                        });
                    }
                    true
                });
                if let Err(e) = looked {
                    return Err(unavailable(&format!("topic {topic}"), e.reason));
                }
                if let Some(n) = batch.next_offset() {
                    next = next.max(n.get());
                }
            }
            from.insert(p.partition_index, next);
        }
        Ok(out)
    }
}
