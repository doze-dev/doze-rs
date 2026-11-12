//! The Produce form: the topic as the broker has it, the record framed as a client frames it, and the broker's answer.

use kafka_wire::api::Response;
use kafka_wire::batch::{OneRecord, single_record};
use kafka_wire::messages::ApiKey;

use super::forms::{self, TopicFacts};
use crate::backend::{Backend, requests::topics_write};
use crate::calls::{self, RecordText};
use crate::model::{
    ProduceErrors, ProduceInput, ProduceRefused, ProduceResult, ProduceView, Produced, RecordRow,
    TopicHead, Visibility,
};
use crate::{partitioner, refusal, validate};

/// The configs the form reads from the topic.
const KEYS: [&str; 3] = ["cleanup.policy", "max.message.bytes", "retention.ms"];

/// A topic and the configs the form cares about.
struct Context {
    facts: TopicFacts,
    cleanup: String,
    limit: Option<i64>,
    retention_ms: Option<i64>,
}

async fn context(b: &Backend, topic: &str) -> Option<Context> {
    let facts = forms::topic(b, topic).await?;
    let configs = forms::configs(b, topic, &KEYS).await;
    Some(Context {
        cleanup: configs
            .get("cleanup.policy")
            .cloned()
            .unwrap_or_else(|| "delete".to_owned()),
        limit: configs
            .get("max.message.bytes")
            .and_then(|v| v.parse().ok()),
        retention_ms: configs.get("retention.ms").and_then(|v| v.parse().ok()),
        facts,
    })
}

fn input_view(
    b: &Backend,
    topic: &str,
    ctx: &Context,
    input: &ProduceInput,
    partition: i32,
    errors: ProduceErrors,
    transaction: Option<String>,
) -> ProduceView {
    let acks = if input.acks.trim().is_empty() {
        "-1"
    } else {
        input.acks.trim()
    };
    let rec = record_text(input);
    ProduceView {
        topic: topic.to_owned(),
        partitions: i32::try_from(ctx.facts.partitions.len()).unwrap_or(i32::MAX),
        compacts: ctx.cleanup.split(',').any(|p| p.trim() == "compact"),
        cleanup: ctx.cleanup.clone(),
        partition: input.partition.clone(),
        key: input.key.clone(),
        value: input.value.clone(),
        headers: input.headers.clone(),
        acks: acks.to_owned(),
        call: calls::produce(
            b.version(ApiKey::Produce),
            topic,
            partition,
            validate::acks(acks).unwrap_or(-1),
            &RecordText {
                key: rec.0.as_deref(),
                value: rec.1.as_deref(),
                headers: &rec.2,
            },
        ),
        errors,
        transaction,
    }
}

/// The key, value and named headers of an input, as the record would carry them: an empty key or value is none.
fn record_text(i: &ProduceInput) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    let headers = i
        .headers
        .iter()
        .filter(|(n, _)| !n.is_empty())
        .cloned()
        .collect();
    (some(&i.key), some(&i.value), headers)
}

fn chosen(ctx: &Context, input: &ProduceInput, key: Option<&str>) -> Result<i32, String> {
    let count = i32::try_from(ctx.facts.partitions.len()).unwrap_or(i32::MAX);
    Ok(validate::partition_choice(&input.partition, count)?
        .unwrap_or_else(|| partitioner::for_key(key.map(str::as_bytes), count)))
}

/// The dialog for a topic, with what was typed in it; `None` if there is no such topic.
pub async fn form(b: &Backend, topic: &str, input: &ProduceInput) -> Option<ProduceView> {
    let ctx = context(b, topic).await?;
    let tx = forms::transactions(b, topic)
        .await
        .into_iter()
        .next()
        .map(|(id, _)| id);
    let (key, ..) = record_text(input);
    let partition = chosen(&ctx, input, key.as_deref()).unwrap_or(0);
    Some(input_view(
        b,
        topic,
        &ctx,
        input,
        partition,
        ProduceErrors::default(),
        tx,
    ))
}

/// Frames the record, sends it, and says what the broker answered.
pub async fn send(b: &Backend, topic: &str, input: &ProduceInput) -> ProduceResult {
    let Some(ctx) = context(b, topic).await else {
        return ProduceResult::NoTopic;
    };
    let (key, value, headers) = record_text(input);
    let acks_text = if input.acks.trim().is_empty() {
        "-1"
    } else {
        input.acks.trim()
    };
    let (acks, partition) = (
        validate::acks(acks_text),
        chosen(&ctx, input, key.as_deref()),
    );
    let (Ok(acks_n), Ok(partition_n)) = (&acks, &partition) else {
        let errors = ProduceErrors {
            acks: acks.err(),
            partition: partition.err(),
            key: None,
        };
        return ProduceResult::Invalid(Box::new(input_view(
            b, topic, &ctx, input, 0, errors, None,
        )));
    };
    let (acks, partition) = (*acks_n, *partition_n);
    let pairs: Vec<(&str, &[u8])> = headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_bytes()))
        .collect();
    let now = crate::source::now_ms();
    let batch = single_record(&OneRecord {
        timestamp_ms: now,
        key: key.as_deref().map(str::as_bytes),
        value: value.as_deref().map(str::as_bytes),
        headers: &pairs,
    });
    let version = b.version(ApiKey::Produce);
    let sent = topics_write::produce(topic, ctx.facts.id, partition, acks, batch);
    let Ok(Response::Produce(r)) = b.call(sent).await else {
        return ProduceResult::NoTopic;
    };
    let Some(p) = r
        .responses
        .into_iter()
        .flat_map(|t| t.partition_responses)
        .next()
    else {
        return ProduceResult::NoTopic;
    };
    let view = input_view(
        b,
        topic,
        &ctx,
        input,
        partition,
        ProduceErrors::default(),
        None,
    );
    if p.error_code != 0 {
        let said = p.error_message.as_ref().map(ToString::to_string);
        let reading = refusal::produce(
            p.error_code,
            said.as_deref(),
            view.compacts,
            key.is_none(),
            ctx.limit,
        );
        let keyless_on_compact = p.error_code == 87 && view.compacts && key.is_none();
        let mut view = view;
        if keyless_on_compact {
            view.errors.key = Some("A compacted topic needs a key on every record.".to_owned());
        }
        return ProduceResult::Refused(Box::new(ProduceRefused {
            answer: calls::produce_answer(
                version,
                topic,
                partition,
                p.error_code,
                &reading.name,
                p.base_offset,
                &[],
            ),
            record_errors: p
                .record_errors
                .iter()
                .filter_map(|e| {
                    e.batch_index_error_message
                        .as_ref()
                        .map(ToString::to_string)
                })
                .take(3)
                .collect(),
            refusal: reading,
            partition,
            base_offset: p.base_offset,
            view,
        }));
    }
    ProduceResult::Sent(Box::new(
        sent_page(
            b,
            topic,
            &ctx,
            &p,
            (partition, now, acks),
            (&key, &value),
            version,
        )
        .await,
    ))
}

async fn sent_page(
    b: &Backend,
    topic: &str,
    ctx: &Context,
    p: &kafka_wire::messages::produce_response::PartitionProduceResponse,
    (partition, now, acks): (i32, i64, i16),
    (key, value): (&Option<String>, &Option<String>),
    version: i16,
) -> Produced {
    let all = &ctx.facts.partitions;
    let stable = forms::offsets(b, topic, all, -1, 1).await;
    let high = forms::offsets(b, topic, all, -1, 0).await;
    let transactions = forms::transactions(b, topic).await;
    let held_back = high
        .iter()
        .filter_map(|(q, h)| {
            let behind = h - stable.get(q).copied().unwrap_or(*h);
            (behind > 0).then_some((*q, behind))
        })
        .collect();
    let (lso, hw) = (
        stable.get(&partition).copied().unwrap_or(-1),
        high.get(&partition).copied().unwrap_or(-1),
    );
    let more = [
        ("log_append_time_ms", p.log_append_time_ms.to_string()),
        ("log_start_offset", p.log_start_offset.to_string()),
    ];
    Produced {
        topic: topic.to_owned(),
        partition,
        offset: p.base_offset,
        timestamp_ms: if p.log_append_time_ms >= 0 {
            p.log_append_time_ms
        } else {
            now
        },
        acks,
        log_append_time_ms: p.log_append_time_ms,
        log_start_offset: p.log_start_offset,
        head: TopicHead {
            name: topic.to_owned(),
            cleanup: ctx.cleanup.clone(),
            partitions: all.len(),
            retention_ms: ctx.retention_ms,
            records: forms::records(b, topic, all).await,
            transactions: transactions.clone(),
            held_back,
        },
        row: RecordRow {
            partition,
            offset: p.base_offset,
            timestamp_ms: now,
            key: key.clone().unwrap_or_default(),
            value: value.clone().unwrap_or_default(),
            size: value.as_ref().map_or(0, |v| v.len() as u64),
            committed: p.base_offset < lso,
        },
        others: Vec::new(),
        visibility: Visibility {
            last_stable: lso,
            high_watermark: hw,
            transaction_on_partition: transactions.iter().any(|(_, ps)| ps.contains(&partition)),
        },
        answer: calls::produce_answer(version, topic, partition, 0, "NONE", p.base_offset, &more),
    }
}
