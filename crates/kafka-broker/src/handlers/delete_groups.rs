//! DeleteGroups.

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::ids::GroupId;
use kafka_wire::messages::delete_groups_response::DeletableGroupResult;
use kafka_wire::messages::{DeleteGroupsRequest, DeleteGroupsResponse};

use crate::groups::{Deleted, kafka_order};
use crate::{Broker, Outcome, blocking};

impl<F: Fs> Broker<F> {
    /// Deletes each group named, in the order Kafka answers: a group with members is refused, an empty one goes with
    /// its committed offsets, and one that is neither there nor has offsets is not found.
    pub(crate) async fn delete_groups(&self, req: &DeleteGroupsRequest) -> Outcome {
        let mut resp = DeleteGroupsResponse::default();
        let asked: Vec<String> = req
            .groups_names
            .iter()
            .map(|g| g.0.as_str().to_owned())
            .collect();
        for name in kafka_order(&asked) {
            let mut result = DeletableGroupResult::default();
            result.group_id = kafka_wire::messages::GroupId(StrBytes::from_string(name.clone()));
            result.error_code = self.delete_group(&name).await.map_or(0, |e| e.code());
            resp.results.push(result);
        }
        Outcome::Respond(kafka_wire::api::Response::DeleteGroups(resp))
    }

    async fn delete_group(&self, name: &str) -> Option<ResponseError> {
        let Ok(group) = GroupId::parse(name) else {
            return Some(ResponseError::InvalidGroupId);
        };
        // Done with the registry held, so that a join cannot make the group again between the deleting and the forgetting.
        let (outcome, cleaned) = self
            .groups
            .delete_with(&group, || {
                blocking::run(|| {
                    let removed = self.meta.delete_group_offsets(&group)?;
                    let had_record = self.meta.group_life(&group)?.is_some();
                    self.meta.forget_group_life(&group)?;
                    Ok::<_, kafka_meta::MetaError>((removed, had_record))
                })
            })
            .await;
        match (outcome, cleaned) {
            (Deleted::NotEmpty, _) | (_, None) => Some(ResponseError::NonEmptyGroup),
            (_, Some(Err(e))) => {
                tracing::error!(%group, error = %e, "deleting a group's offsets failed");
                Some(ResponseError::KafkaStorageError)
            }
            // Gone from memory, recorded as having had members (the broker has started since), or with offsets: it was there.
            (outcome, Some(Ok((removed, had_record)))) => {
                (outcome != Deleted::Gone && !had_record && removed == 0)
                    .then_some(ResponseError::GroupIdNotFound)
            }
        }
    }
}
