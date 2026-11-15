//! ShareAcknowledge (KIP-932): a member's acknowledgements without a fetch (what a share consumer sends on a commit, and on
//! close at epoch -1). Kafka 4.4.0's order (`KafkaApis.handleShareAcknowledgeRequest`): the checks ShareFetch makes first,
//! then the session's epoch (which it never opens), then each partition as ShareFetch's acknowledgements are taken.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::StrBytes;
use kafka_wire::ids::{GroupId, MemberId};
use kafka_wire::messages::share_acknowledge_response::{
    PartitionData, ShareAcknowledgeTopicResponse,
};
use kafka_wire::messages::{ShareAcknowledgeRequest, ShareAcknowledgeResponse};

use super::share_acks::Asked;
use super::share_group_heartbeat::member_id_valid;
use crate::share::partition::AckBatch;
use crate::{Broker, Caller, Outcome};

fn top_level(error: E) -> Outcome {
    let mut resp = ShareAcknowledgeResponse::default();
    resp.error_code = error.code();
    Outcome::Respond(kafka_wire::api::Response::ShareAcknowledge(resp))
}

impl<F: Fs> Broker<F> {
    pub(crate) async fn share_acknowledge(
        &self,
        version: i16,
        caller: &Caller,
        req: &ShareAcknowledgeRequest,
    ) -> Outcome {
        let Some(group_name) = req.group_id.as_ref().map(|g| g.0.as_str()) else {
            return top_level(E::InvalidRequest);
        };
        if !self.guard(caller).group(Operation::Read, group_name) {
            return top_level(E::GroupAuthorizationFailed);
        }
        let member_text = req.member_id.as_ref().map(StrBytes::as_str);
        if !member_id_valid(member_text) {
            return top_level(E::InvalidRequest);
        }
        let (Ok(group), Some(member)) = (
            GroupId::parse(group_name),
            member_text.and_then(MemberId::new),
        ) else {
            return top_level(E::InvalidRequest);
        };
        let closes = match self
            .shares
            .sessions()
            .acknowledge(&(group.clone(), member.clone()), req.share_session_epoch)
        {
            Ok(closes) => closes,
            Err(e) => return top_level(e),
        };
        let asked: Vec<Asked> = req
            .topics
            .iter()
            .flat_map(|t| {
                t.partitions.iter().map(move |p| Asked {
                    topic: t.topic_id,
                    partition: p.partition_index,
                    batches: p
                        .acknowledgement_batches
                        .iter()
                        .map(|b| AckBatch {
                            first: b.first_offset,
                            last: b.last_offset,
                            types: b.acknowledge_types.clone(),
                        })
                        .collect(),
                })
            })
            .collect();
        let renew = version >= 2 && req.is_renew_ack;
        let acked =
            self.share_acknowledge_all(caller, (&group, member.as_str()), &asked, (version, renew));
        let mut resp = ShareAcknowledgeResponse::default();
        resp.acquisition_lock_timeout_ms =
            i32::try_from(self.share_settings(&group).lock_ms).unwrap_or(i32::MAX);
        for a in acked {
            let i = match resp.responses.iter().position(|t| t.topic_id == a.topic) {
                Some(i) => i,
                None => {
                    let mut t = ShareAcknowledgeTopicResponse::default();
                    t.topic_id = a.topic;
                    resp.responses.push(t);
                    resp.responses.len() - 1
                }
            };
            let mut d = PartitionData::default();
            d.partition_index = a.partition;
            d.error_code = a.error.map_or(0, |e| e.code());
            d.error_message = a.message.map(StrBytes::from_string);
            resp.responses[i].partitions.push(d);
        }
        if closes {
            self.close_share_session(&group, &member);
        }
        Outcome::Respond(kafka_wire::api::Response::ShareAcknowledge(resp))
    }
}
