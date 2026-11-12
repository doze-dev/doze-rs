//! Add partitions: the count now, the new total typed, and the broker's answer to it.

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::forms;
use crate::backend::{Backend, requests::topics_write};
use crate::model::AddView;
use crate::{calls, refusal, validate};

/// What an Add-partitions came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddResult {
    /// The topic has this many partitions now.
    Done {
        topic: String,
        from: i32,
        to: i32,
    },
    /// The broker refused: the dialog again, with its words.
    Refused(Box<AddView>),
    /// The count is not a number: the dialog again, with the error.
    Invalid(Box<AddView>),
    NoTopic,
}

fn view(b: &Backend, topic: &str, have: i32, count: &str) -> AddView {
    AddView {
        topic: topic.to_owned(),
        have,
        count: count.to_owned(),
        most: b.limits().map(|l| l.max_partitions),
        error: None,
        refusal: None,
        call: calls::create_partitions(b.version(ApiKey::CreatePartitions), topic, count, false),
        answer: None,
    }
}

async fn partitions_now(b: &Backend, topic: &str) -> Option<i32> {
    let facts = forms::topic(b, topic).await?;
    i32::try_from(facts.partitions.len()).ok()
}

/// The dialog: the topic's count, and `count` (default: one more) in the field. `None` if there is no such topic.
pub async fn form(b: &Backend, topic: &str, count: Option<&str>) -> Option<AddView> {
    let have = partitions_now(b, topic).await?;
    let suggested = have.saturating_add(1).to_string();
    Some(view(b, topic, have, count.unwrap_or(&suggested)))
}

/// Asks the broker to raise the topic to `count` partitions.
pub async fn add(b: &Backend, topic: &str, count: &str) -> AddResult {
    let Some(have) = partitions_now(b, topic).await else {
        return AddResult::NoTopic;
    };
    let mut v = view(b, topic, have, count);
    let Ok(wanted) = validate::partitions(count, None) else {
        v.error = validate::partitions(count, None).err();
        return AddResult::Invalid(Box::new(v));
    };
    let version = b.version(ApiKey::CreatePartitions);
    let Ok(Response::CreatePartitions(r)) = b
        .call(topics_write::create_partitions(topic, wanted, false))
        .await
    else {
        return AddResult::NoTopic;
    };
    let Some(res) = r.results.into_iter().next() else {
        return AddResult::NoTopic;
    };
    if res.error_code == 0 {
        return AddResult::Done {
            topic: topic.to_owned(),
            from: have,
            to: wanted,
        };
    }
    let said = res.error_message.as_ref().map(ToString::to_string);
    let reading = refusal::add_partitions(res.error_code, said.as_deref(), have, v.most);
    v.error = (res.error_code == 37)
        .then(|| validate::not_above(wanted, have))
        .flatten();
    v.answer = Some(calls::create_partitions_answer(version, topic, &reading));
    v.refusal = Some(reading);
    AddResult::Refused(Box::new(v))
}
