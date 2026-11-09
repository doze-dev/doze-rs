//! DescribeProducers: the producers a partition remembers, and the transaction each has open there.

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::api::Response;
use kafka_wire::ids::{PartitionId, TopicName};
use kafka_wire::messages::describe_producers_response::{
    PartitionResponse, ProducerState, TopicResponse,
};
use kafka_wire::messages::{DescribeProducersRequest, DescribeProducersResponse, ProducerId};

use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn describe_producers(&self, req: &DescribeProducersRequest) -> Outcome {
        let mut resp = DescribeProducersResponse::default();
        for t in &req.topics {
            let mut out = TopicResponse::default();
            out.name = t.name.clone();
            let topic = TopicName::parse(&t.name.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            for &index in &t.partition_indexes {
                let mut p = PartitionResponse::default();
                p.partition_index = index;
                let Some((topic, id)) = topic
                    .as_deref()
                    .zip(PartitionId::new(index))
                    .filter(|(t, id)| id.get() < t.partitions.get())
                else {
                    p.error_code = ResponseError::UnknownTopicOrPartition.code();
                    out.partitions.push(p);
                    continue;
                };
                match self.partitions.get(topic, id).await {
                    Ok(partition) => match partition.txns().await {
                        Ok(view) => {
                            p.active_producers = view
                                .producers
                                .iter()
                                .map(|(producer, epoch, last_sequence, last_used)| {
                                    let mut s = ProducerState::default();
                                    s.producer_id = ProducerId(*producer);
                                    s.producer_epoch = i32::from(*epoch);
                                    s.last_sequence = *last_sequence;
                                    s.last_timestamp = *last_used;
                                    s.coordinator_epoch = 0;
                                    s.current_txn_start_offset = view
                                        .open
                                        .iter()
                                        .find(|o| o.producer == *producer)
                                        .map_or(-1, |o| o.first);
                                    s
                                })
                                .collect();
                            // Producers with an open transaction but no remembered batches here are listed too.
                            for o in view.open.iter().filter(|o| {
                                !view.producers.iter().any(|(id, ..)| *id == o.producer)
                            }) {
                                let mut s = ProducerState::default();
                                s.producer_id = ProducerId(o.producer);
                                s.producer_epoch = i32::from(o.epoch);
                                s.last_sequence = -1;
                                s.last_timestamp = -1;
                                s.current_txn_start_offset = o.first;
                                p.active_producers.push(s);
                            }
                            p.active_producers.sort_by_key(|s| s.producer_id.0);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "describing producers could not read a partition");
                            p.error_code = ResponseError::KafkaStorageError.code();
                        }
                    },
                    Err(e) => {
                        tracing::error!(error = %e, "describing producers could not open a partition");
                        p.error_code = ResponseError::KafkaStorageError.code();
                    }
                }
                out.partitions.push(p);
            }
            resp.topics.push(out);
        }
        Outcome::Respond(Response::DescribeProducers(resp))
    }
}
