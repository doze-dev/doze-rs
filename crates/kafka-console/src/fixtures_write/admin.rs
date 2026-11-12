//! Add partitions, delete records and delete topic over the larder: the dialogs, and stand-ins for the broker's answers.

use super::{NOW_MS, TOPICS, topic};
use crate::gather::{AddResult, DeleteResult, RecordsResult};
use crate::model::{
    AddView, DeleteRecordsRefused, DeleteRecordsView, DeleteView, Deleted, InUse, RecordsDeleted,
    Remaining,
};
use crate::{calls, refusal, validate};

const MOST: i32 = 1000;

fn add_view(name: &str, have: i32, count: &str) -> AddView {
    AddView {
        topic: name.to_owned(),
        have,
        count: count.to_owned(),
        most: Some(MOST),
        error: None,
        refusal: None,
        call: calls::create_partitions(3, name, count, false),
        answer: None,
    }
}

/// The Add-partitions dialog for a topic of the larder; `count` defaults to one more.
#[must_use]
pub fn add_form(name: &str, count: Option<&str>) -> Option<AddView> {
    let t = topic(name)?;
    let suggested = (t.partitions + 1).to_string();
    Some(add_view(name, t.partitions, count.unwrap_or(&suggested)))
}

/// What CreatePartitions answers in the larder.
#[must_use]
pub fn add_partitions(name: &str, count: &str) -> AddResult {
    let Some(t) = topic(name) else {
        return AddResult::NoTopic;
    };
    let mut v = add_view(name, t.partitions, count);
    let Ok(wanted) = validate::partitions(count, None) else {
        v.error = validate::partitions(count, None).err();
        return AddResult::Invalid(Box::new(v));
    };
    let words = if wanted == t.partitions {
        Some(format!("Topic already has {} partition(s).", t.partitions))
    } else if wanted < t.partitions {
        Some(format!(
            "The topic {name} currently has {} partition(s); {wanted} would not be an increase.",
            t.partitions
        ))
    } else if wanted > MOST {
        Some(format!(
            "{wanted} partitions is above this broker's limit of {MOST} (--max-partitions)"
        ))
    } else {
        None
    };
    let Some(words) = words else {
        return AddResult::Done {
            topic: name.to_owned(),
            from: t.partitions,
            to: wanted,
        };
    };
    let reading = refusal::add_partitions(37, Some(&words), t.partitions, Some(MOST));
    v.error = validate::not_above(wanted, t.partitions);
    v.answer = Some(calls::create_partitions_answer(3, name, &reading));
    v.refusal = Some(reading);
    AddResult::Refused(Box::new(v))
}

/// The Delete-records dialog for a partition: `orders-0` runs from 0 to 16,104 and `payments-workers` is at 16,092.
#[must_use]
pub fn delete_records_form(
    name: &str,
    partition: i32,
    offset: Option<&str>,
) -> Option<DeleteRecordsView> {
    let t = topic(name)?;
    if partition < 0 || partition >= t.partitions {
        return None;
    }
    let typed = offset.map_or_else(|| "16000".to_owned(), str::to_owned);
    Some(DeleteRecordsView {
        topic: name.to_owned(),
        partition,
        log_start: 0,
        high_watermark: 16_104,
        call: calls::delete_records(2, name, partition, &typed),
        offset: typed,
        error: None,
        groups: vec![("payments-workers".to_owned(), 16_092)],
    })
}

/// What DeleteRecords answers in the larder.
#[must_use]
pub fn delete_records(name: &str, partition: i32, offset: &str) -> RecordsResult {
    let Some(mut v) = delete_records_form(name, partition, Some(offset)) else {
        return RecordsResult::NoTopic;
    };
    let Ok(to) = validate::delete_before(offset) else {
        v.error = validate::delete_before(offset).err();
        return RecordsResult::Invalid(Box::new(v));
    };
    let code = match topic(name) {
        Some(t) if t.cleanup == "compact" => 44,
        _ if to > v.high_watermark => 1,
        _ => {
            let low = if to == -1 { v.high_watermark } else { to };
            return RecordsResult::Done(RecordsDeleted {
                topic: name.to_owned(),
                partition,
                low_watermark: low,
            });
        }
    };
    let reading = refusal::delete_records(code, Some(v.high_watermark));
    RecordsResult::Refused(Box::new(DeleteRecordsRefused {
        topic: name.to_owned(),
        cleanup: topic(name).map_or_else(String::new, |t| t.cleanup.to_owned()),
        partition,
        log_start: 0,
        answer: calls::delete_records_answer(2, name, partition, -1, &reading),
        refusal: reading,
    }))
}

fn use_of(name: &str) -> InUse {
    if name == "orders" {
        InUse {
            groups: vec![("payments-workers".to_owned(), "Stable".to_owned(), 3)],
            transactions: vec!["orders-eos".to_owned()],
        }
    } else {
        InUse::default()
    }
}

/// The Delete-topic dialog for a topic of the larder.
#[must_use]
pub fn delete_form(name: &str, typed: &str) -> Option<DeleteView> {
    let t = topic(name)?;
    Some(DeleteView {
        topic: name.to_owned(),
        partitions: usize::try_from(t.partitions).unwrap_or(0),
        records: t.records,
        in_use: use_of(name),
        typed: typed.to_owned(),
        error: None,
        call: calls::delete_topics(6, name),
    })
}

/// What DeleteTopics answers in the larder: the topic and its partitions are gone, and the list is what is left.
#[must_use]
pub fn delete_topic(name: &str, typed: &str) -> DeleteResult {
    let Some(mut v) = delete_form(name, typed) else {
        return DeleteResult::NoTopic;
    };
    if typed != name {
        v.error = Some("Type the name of the topic to confirm.".to_owned());
        return DeleteResult::Invalid(Box::new(v));
    }
    let left: Vec<Remaining> = TOPICS
        .iter()
        .filter(|t| t.name != name)
        .map(|t| Remaining {
            name: t.name.to_owned(),
            partitions: usize::try_from(t.partitions).unwrap_or(0),
            cleanup: t.cleanup.to_owned(),
            records: t.records,
            last_write_ms: t.written_ago_ms.map(|ago| NOW_MS - ago),
        })
        .collect();
    DeleteResult::Done(Box::new(Deleted {
        now_ms: NOW_MS,
        name: name.to_owned(),
        records: v.records,
        partitions: v.partitions,
        in_use: v.in_use,
        topics: left.len(),
        partitions_left: left.iter().map(|t| t.partitions).sum(),
        remaining: left,
    }))
}
