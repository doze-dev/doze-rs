//! The Produce dialog over the larder, and a stand-in for the broker's answer to a produce.

use super::{SENT_MS, topic};
use crate::calls::{self, Outcome, RecordText};
use crate::model::{
    ProduceErrors, ProduceInput, ProduceRefused, ProduceResult, ProduceView, Produced, RecordRow,
    TopicHead, Visibility,
};
use crate::{partitioner, refusal, validate};

const VERSION: i16 = 9;
/// The topic's `max.message.bytes` in the larder (Kafka's default).
const LIMIT: i64 = 1_048_588;

fn texts(i: &ProduceInput) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    (
        some(&i.key),
        some(&i.value),
        i.headers
            .iter()
            .filter(|(n, _)| !n.is_empty())
            .cloned()
            .collect(),
    )
}

/// The dialog for a topic of the larder, with what was typed.
#[must_use]
pub fn produce_form(name: &str, input: &ProduceInput) -> Option<ProduceView> {
    let t = topic(name)?;
    let (key, value, headers) = texts(input);
    let partition = validate::partition_choice(&input.partition, t.partitions)
        .ok()
        .flatten()
        .unwrap_or_else(|| partitioner::for_key(key.as_deref().map(str::as_bytes), t.partitions));
    let acks = if input.acks.trim().is_empty() {
        "-1"
    } else {
        input.acks.trim()
    };
    Some(ProduceView {
        topic: t.name.to_owned(),
        partitions: t.partitions,
        compacts: t.cleanup.contains("compact"),
        cleanup: t.cleanup.to_owned(),
        partition: input.partition.clone(),
        key: input.key.clone(),
        value: input.value.clone(),
        headers: input.headers.clone(),
        acks: acks.to_owned(),
        errors: ProduceErrors::default(),
        call: calls::produce(
            VERSION,
            t.name,
            partition,
            validate::acks(acks).unwrap_or(-1),
            &RecordText {
                key: key.as_deref(),
                value: value.as_deref(),
                headers: &headers,
            },
        ),
        transaction: (t.name == "orders").then(|| "orders-eos".to_owned()),
    })
}

fn others() -> Vec<RecordRow> {
    [(16_103, 0, "ord-88418", "c-3092", 91), (16_102, -1, "ord-88417", "c-5518", 88), (16_101, -2, "ord-88416", "c-7710", 86), (16_100, -2, "ord-88415", "c-2204", 97)]
        .into_iter()
        .map(|(offset, delta, key, customer, size)| RecordRow {
            partition: 0,
            offset,
            timestamp_ms: SENT_MS - 19_437 + delta,
            key: key.to_owned(),
            value: format!("{{\"order_id\":\"{key}\",\"customer\":\"{customer}\",\"items\":[{{\"sku\":\"FV-TOM-V\",\"qty\":1}}],\"total\":7.40}}"),
            size,
            committed: false,
        })
        .collect()
}

/// The refusal the larder's broker gives this record, if it gives one.
fn refusal_of(
    view: &ProduceView,
    key: Option<&String>,
    partition: i32,
    size: i64,
) -> Option<crate::model::Refusal> {
    if view.compacts && key.is_none() {
        let said = format!(
            "Compacted topic cannot accept message without key in topic partition {}-{partition}",
            view.topic
        );
        Some(refusal::produce(87, Some(&said), true, true, Some(LIMIT)))
    } else if size > LIMIT {
        Some(refusal::produce(
            10,
            None,
            view.compacts,
            key.is_none(),
            Some(LIMIT),
        ))
    } else {
        None
    }
}

/// What Produce answers in the larder.
#[must_use]
pub fn produce_send(name: &str, input: &ProduceInput) -> ProduceResult {
    let Some(mut view) = produce_form(name, input) else {
        return ProduceResult::NoTopic;
    };
    let (key, value, _) = texts(input);
    let acks = match validate::acks(&view.acks) {
        Ok(a) => a,
        Err(e) => {
            view.errors.acks = Some(e);
            return ProduceResult::Invalid(Box::new(view));
        }
    };
    let partition = validate::partition_choice(&input.partition, view.partitions)
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            partitioner::for_key(key.as_deref().map(str::as_bytes), view.partitions)
        });
    let size = value
        .as_ref()
        .map_or(0, |v| i64::try_from(v.len()).unwrap_or(i64::MAX));
    if let Some(reading) = refusal_of(&view, key.as_ref(), partition, size) {
        if reading.code == 87 {
            view.errors.key = Some("A compacted topic needs a key on every record.".to_owned());
        }
        let outcome = Outcome {
            code: reading.code,
            name: &reading.name,
            base_offset: -1,
        };
        return ProduceResult::Refused(Box::new(ProduceRefused {
            answer: calls::produce_answer(VERSION, &view.topic, partition, outcome, &[]),
            record_errors: Vec::new(),
            refusal: reading,
            partition,
            base_offset: -1,
            view,
        }));
    }
    let row = RecordRow {
        partition,
        offset: 16_058,
        timestamp_ms: SENT_MS,
        key: key.unwrap_or_default(),
        value: value.unwrap_or_default(),
        size: u64::try_from(size).unwrap_or(0),
        committed: partition != 0 && partition != 1,
    };
    ProduceResult::Sent(Box::new(sent(view, row, partition, acks)))
}

fn sent(view: ProduceView, row: RecordRow, partition: i32, acks: i16) -> Produced {
    let more = [
        ("log_append_time_ms", "-1".to_owned()),
        ("log_start_offset", "0".to_owned()),
    ];
    let outcome = Outcome {
        code: 0,
        name: "NONE",
        base_offset: 16_058,
    };
    Produced {
        topic: view.topic.clone(),
        partition,
        offset: 16_058,
        timestamp_ms: SENT_MS,
        acks,
        log_append_time_ms: -1,
        log_start_offset: 0,
        head: TopicHead {
            name: view.topic.clone(),
            cleanup: view.cleanup.clone(),
            partitions: usize::try_from(view.partitions).unwrap_or(0),
            retention_ms: Some(604_800_000),
            records: 48_214,
            transactions: vec![("orders-eos".to_owned(), vec![0, 1])],
            held_back: vec![(0, 6)],
        },
        others: others(),
        visibility: Visibility {
            last_stable: 16_059,
            high_watermark: 16_059,
            transaction_on_partition: !row.committed,
        },
        answer: calls::produce_answer(VERSION, &view.topic, partition, outcome, &more),
        row,
    }
}
