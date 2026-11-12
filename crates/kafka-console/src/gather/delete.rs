//! Delete topic: what is in it and who uses it, the confirmation typed, and what is left afterwards.

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::forms;
use crate::backend::{Backend, requests::topics_write};
use crate::model::{DeleteView, Deleted, Refusal, Remaining};
use crate::{calls, refusal};

/// The most topics the list after a delete shows.
pub const LIST_MOST: usize = 100;

/// What a Delete-topic came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteResult {
    Done(Box<Deleted>),
    /// The broker refused: the dialog again, with its words.
    Refused(Box<DeleteView>, Refusal),
    /// The name typed is not the topic's: the dialog again, with the error.
    Invalid(Box<DeleteView>),
    NoTopic,
}

/// The dialog for a topic; `None` if there is no such topic.
pub async fn form(b: &Backend, topic: &str, typed: &str) -> Option<DeleteView> {
    let facts = forms::topic(b, topic).await?;
    Some(DeleteView {
        topic: topic.to_owned(),
        partitions: facts.partitions.len(),
        records: forms::records(b, topic, &facts.partitions).await,
        in_use: forms::in_use(b, topic, &facts.partitions).await,
        typed: typed.to_owned(),
        error: None,
        call: calls::delete_topics(b.version(ApiKey::DeleteTopics), topic),
    })
}

/// The topics that are left, as the list shows them.
async fn remaining(b: &Backend) -> (Vec<Remaining>, usize, usize) {
    let Ok(Response::Metadata(m)) = b.call(crate::backend::requests::metadata_all()).await else {
        return (Vec::new(), 0, 0);
    };
    let mut topics: Vec<_> = m.topics.into_iter().filter(|t| !t.is_internal).collect();
    topics.sort_by_key(|t| t.name.as_ref().map(|n| n.0.to_string()));
    let (all, parts) = (
        topics.len(),
        topics.iter().map(|t| t.partitions.len()).sum(),
    );
    let mut out = Vec::new();
    for t in topics.into_iter().take(LIST_MOST) {
        let Some(name) = t.name.map(|n| n.0.to_string()) else {
            continue;
        };
        let ps: Vec<i32> = t.partitions.iter().map(|p| p.partition_index).collect();
        let configs = forms::configs(b, &name, &["cleanup.policy"]).await;
        out.push(Remaining {
            partitions: ps.len(),
            cleanup: configs.get("cleanup.policy").cloned().unwrap_or_default(),
            records: forms::records(b, &name, &ps).await,
            last_write_ms: forms::last_write(b, &name, &ps).await,
            name,
        });
    }
    (out, all, parts)
}

/// Deletes the topic if `typed` is its name.
pub async fn delete(b: &Backend, topic: &str, typed: &str) -> DeleteResult {
    let Some(mut view) = form(b, topic, typed).await else {
        return DeleteResult::NoTopic;
    };
    if typed != topic {
        view.error = Some("Type the name of the topic to confirm.".to_owned());
        return DeleteResult::Invalid(Box::new(view));
    }
    let Ok(Response::DeleteTopics(r)) = b.call(topics_write::delete_topic(topic)).await else {
        return DeleteResult::NoTopic;
    };
    let Some(res) = r.responses.into_iter().next() else {
        return DeleteResult::NoTopic;
    };
    if res.error_code != 0 {
        let said = res.error_message.as_ref().map(ToString::to_string);
        return DeleteResult::Refused(
            Box::new(view),
            refusal::delete_topic(res.error_code, said.as_deref()),
        );
    }
    let (left, topics, partitions_left) = remaining(b).await;
    DeleteResult::Done(Box::new(Deleted {
        now_ms: crate::source::now_ms(),
        name: topic.to_owned(),
        records: view.records,
        partitions: view.partitions,
        in_use: view.in_use,
        topics,
        partitions_left,
        remaining: left,
    }))
}
