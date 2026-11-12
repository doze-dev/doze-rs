//! Delete records: where a partition's log starts and ends, who has committed on it, and the broker's answer.

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::forms;
use crate::backend::{Backend, requests::topics_write};
use crate::model::{DeleteRecordsRefused, DeleteRecordsView, RecordsDeleted};
use crate::{calls, refusal, validate};

/// What a Delete-records came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordsResult {
    Done(RecordsDeleted),
    Refused(Box<DeleteRecordsRefused>),
    /// The offset is not a number: the dialog again, with the error.
    Invalid(Box<DeleteRecordsView>),
    NoTopic,
}

/// The dialog for a partition; `None` if the topic or the partition is not there. `offset` defaults to the log start.
pub async fn form(
    b: &Backend,
    topic: &str,
    partition: i32,
    offset: Option<&str>,
) -> Option<DeleteRecordsView> {
    let facts = forms::topic(b, topic).await?;
    if !facts.partitions.contains(&partition) {
        return None;
    }
    let one = [partition];
    let start = forms::offsets(b, topic, &one, -2, 0)
        .await
        .get(&partition)
        .copied()
        .unwrap_or(0);
    let end = forms::offsets(b, topic, &one, -1, 0)
        .await
        .get(&partition)
        .copied()
        .unwrap_or(start);
    let groups = forms::committed_on(b, topic, &one)
        .await
        .into_iter()
        .map(|(g, _, o)| (g, o))
        .collect();
    let typed = offset.map_or_else(|| start.to_string(), str::to_owned);
    Some(DeleteRecordsView {
        topic: topic.to_owned(),
        partition,
        log_start: start,
        high_watermark: end,
        call: calls::delete_records(b.version(ApiKey::DeleteRecords), topic, partition, &typed),
        offset: typed,
        error: None,
        groups,
    })
}

/// Asks the broker to move the partition's log start to `offset` (-1: the high watermark).
pub async fn delete(b: &Backend, topic: &str, partition: i32, offset: &str) -> RecordsResult {
    let Some(mut view) = form(b, topic, partition, Some(offset)).await else {
        return RecordsResult::NoTopic;
    };
    let Ok(to) = validate::delete_before(offset) else {
        view.error = validate::delete_before(offset).err();
        return RecordsResult::Invalid(Box::new(view));
    };
    let Ok(Response::DeleteRecords(r)) = b
        .call(topics_write::delete_records(topic, partition, to))
        .await
    else {
        return RecordsResult::NoTopic;
    };
    let Some(p) = r
        .topics
        .into_iter()
        .flat_map(|t| t.partitions)
        .find(|p| p.partition_index == partition)
    else {
        return RecordsResult::NoTopic;
    };
    if p.error_code == 0 {
        return RecordsResult::Done(RecordsDeleted {
            topic: topic.to_owned(),
            partition,
            low_watermark: p.low_watermark,
        });
    }
    let reading = refusal::delete_records(p.error_code, Some(view.high_watermark));
    let cleanup = forms::configs(b, topic, &["cleanup.policy"]).await;
    RecordsResult::Refused(Box::new(DeleteRecordsRefused {
        topic: topic.to_owned(),
        cleanup: cleanup.get("cleanup.policy").cloned().unwrap_or_default(),
        partition,
        log_start: view.log_start,
        answer: calls::delete_records_answer(
            b.version(ApiKey::DeleteRecords),
            topic,
            partition,
            p.low_watermark,
            &reading,
        ),
        refusal: reading,
    }))
}
