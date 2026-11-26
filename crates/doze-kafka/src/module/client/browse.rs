//! Records read whole, headers and all, at the offsets a person browses to, and one record written with a key and
//! headers: what the console's tree, its value viewer and the produce form are made of.

use doze_module::Result;
use doze_module::proto::msg::error::{Error, ErrorKind};
use kafka_protocol::messages::fetch_request::{FetchPartition, FetchTopic};
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::{
    ApiKey, FetchRequest, FetchResponse, ProduceRequest, ProduceResponse,
};

use super::{Client, FETCH_BYTES, MOST, refused, topic_name, unavailable};

/// The most fetches one read makes, so a partition of tiny batches cannot keep it going for ever.
const MOST_FETCHES: usize = 64;

/// A record as the console shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) offset: i64,
    pub(crate) timestamp: i64,
    pub(crate) key: Option<Vec<u8>>,
    /// Its value, or none for a tombstone; left empty when the read was asked not to keep values.
    pub(crate) value: Option<Vec<u8>>,
    /// The value's size in bytes, kept or not.
    pub(crate) size: usize,
    /// Its headers in order; a header's value may be absent.
    pub(crate) headers: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

impl Client {
    /// The records of one partition from `from` up to (not including) `before`, oldest first, without waiting for new
    /// ones. With `values` false only their sizes are kept (a page of the tree needs no bodies). Transaction markers
    /// are not records anyone wrote and are left out.
    pub(crate) async fn read(
        &mut self,
        topic: &str,
        partition: i32,
        from: i64,
        before: i64,
        values: bool,
    ) -> Result<Vec<Record>> {
        let mut out = Vec::new();
        let mut at = from;
        for _ in 0..MOST_FETCHES {
            if at >= before {
                break;
            }
            let next = self
                .fetch_at(topic, partition, at, |r| {
                    if r.offset >= at && r.offset < before {
                        out.push(Record {
                            offset: r.offset,
                            timestamp: r.timestamp,
                            key: r.key.map(<[u8]>::to_vec),
                            value: r
                                .value
                                .map(|v| if values { v.to_vec() } else { Vec::new() }),
                            size: r.value.map_or(0, <[u8]>::len),
                            headers: r
                                .headers
                                .iter()
                                .map(|(k, v)| (k.to_vec(), v.map(<[u8]>::to_vec)))
                                .collect(),
                        });
                    }
                })
                .await?;
            if next <= at {
                break;
            }
            at = next;
        }
        Ok(out)
    }

    /// One fetch of a partition from `at`, answered at once; each record is shown to `look`. The offset after what
    /// came whole.
    async fn fetch_at(
        &mut self,
        topic: &str,
        partition: i32,
        at: i64,
        mut look: impl FnMut(&kafka_wire::batch::Looked<'_>),
    ) -> Result<i64> {
        let mut fp = FetchPartition::default();
        fp.partition = partition;
        fp.fetch_offset = at;
        fp.partition_max_bytes = FETCH_BYTES;
        let mut t = FetchTopic::default();
        t.topic = topic_name(topic);
        t.partitions.push(fp);
        let mut q = FetchRequest::default();
        q.replica_id = (-1).into();
        q.max_wait_ms = 0;
        q.min_bytes = 0;
        q.max_bytes = FETCH_BYTES;
        q.topics.push(t);
        let a: FetchResponse = self.call(ApiKey::Fetch, 4, &q).await?;
        let mut next = at;
        for p in a.responses.into_iter().flat_map(|t| t.partitions) {
            if p.error_code != 0 {
                return Err(refused(
                    topic,
                    &format!("partition {partition} cannot be read at offset {at}"),
                    p.error_code,
                    None,
                ));
            }
            let Some(records) = p.records else { continue };
            for batch in kafka_wire::batch::Batches::new(records).map_while(std::result::Result::ok)
            {
                if !batch.attributes().is_control() {
                    let looked = batch.each_record_full(MOST as u64, |r| {
                        look(r);
                        true
                    });
                    if let Err(e) = looked {
                        return Err(unavailable(&format!("topic {topic}"), e.reason));
                    }
                }
                if let Some(n) = batch.next_offset() {
                    next = next.max(n.get());
                }
            }
        }
        Ok(next)
    }

    /// Writes one record with its key and headers to a partition and waits for it to be stored; its offset.
    pub(crate) async fn produce_record(
        &mut self,
        topic: &str,
        partition: i32,
        key: Option<&[u8]>,
        value: &[u8],
        headers: &[(&str, &[u8])],
    ) -> Result<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let record = kafka_wire::batch::OneRecord {
            timestamp_ms: now,
            key,
            value: Some(value),
            headers,
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
}
