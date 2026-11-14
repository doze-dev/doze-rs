//! DescribeProducers: the producers a partition remembers, and the transaction each has open there.

use kafka_auth::authz::operation;
use kafka_meta::Topic;
use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::ids::{PartitionId, TopicName};
use kafka_wire::messages::describe_producers_response::{
    PartitionResponse, ProducerState, TopicResponse,
};
use kafka_wire::messages::{DescribeProducersRequest, DescribeProducersResponse, ProducerId};
use kafka_wire::{ResponseError, StrBytes};

use super::create_topics::name_message;
use crate::guard::TOPIC_DENIED;
use crate::{Broker, Caller, Outcome};

impl<F: Fs> Broker<F> {
    /// The caller must be allowed to READ the topic. Each topic is judged in Kafka's order, before any partition is
    /// looked for: a name that cannot be a topic's, then READ, then whether the topic is there, so a topic the caller
    /// may not read is 29 on each partition whether it exists or not (`KafkaApis.handleDescribeProducersRequest`).
    pub(crate) async fn describe_producers(
        &self,
        caller: &Caller,
        req: &DescribeProducersRequest,
    ) -> Outcome {
        let mut resp = DescribeProducersResponse::default();
        for t in &req.topics {
            let mut out = TopicResponse::default();
            out.name = t.name.clone();
            let refusal = self.describe_refusal(caller, &t.name.0);
            let topic = TopicName::parse(&t.name.0)
                .ok()
                .and_then(|n| self.meta.topic(&n));
            for &index in &t.partition_indexes {
                let mut p = PartitionResponse::default();
                p.partition_index = index;
                if let Some((code, message)) = &refusal {
                    p.error_code = code.code();
                    p.error_message = Some(StrBytes::from_string(message.clone()));
                } else {
                    self.producers_of(topic.as_deref(), index, &mut p).await;
                }
                out.partitions.push(p);
            }
            resp.topics.push(out);
        }
        Outcome::Respond(Response::DescribeProducers(resp))
    }

    /// Why every partition of the topic is refused, if it is: the name is invalid, or the caller may not READ it.
    fn describe_refusal(&self, caller: &Caller, raw: &str) -> Option<(ResponseError, String)> {
        if let Err(e) = TopicName::parse(raw) {
            return Some((ResponseError::InvalidTopicException, name_message(e, raw)));
        }
        (!self.topic_allows(caller, raw, operation::READ)).then(|| {
            (
                ResponseError::TopicAuthorizationFailed,
                TOPIC_DENIED.to_owned(),
            )
        })
    }

    /// One partition's producers, or why they cannot be told.
    async fn producers_of(&self, topic: Option<&Topic>, index: i32, p: &mut PartitionResponse) {
        let Some((topic, id)) = topic
            .zip(PartitionId::new(index))
            .filter(|(t, id)| id.get() < t.partitions.get())
        else {
            p.error_code = ResponseError::UnknownTopicOrPartition.code();
            return;
        };
        // A partition never written has no producers: it is not opened for this (opening makes its first segment, so a read
        // would write files).
        match self.partitions.get_unless_empty(topic, id).await {
            Ok(None) => {}
            Ok(Some(partition)) => match partition.txns().await {
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
                    for o in view
                        .open
                        .iter()
                        .filter(|o| !view.producers.iter().any(|(id, ..)| *id == o.producer))
                    {
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
    }
}
