//! The Produce form: the topic as the broker has it, the record framed as a client frames it, and the broker's answer.

use kafka_wire::api::Response;
use kafka_wire::batch::{OneRecord, single_record};
use kafka_wire::messages::ApiKey;
use kafka_wire::messages::produce_response::PartitionProduceResponse;

use super::forms::{self, TopicFacts};
use crate::backend::{Backend, requests::topics_write};
use crate::calls::{self, Outcome, RecordText};
use crate::model::write::{RecordRow, TopicHead};
use crate::model::{
    ProduceErrors, ProduceInput, ProduceRefused, ProduceResult, ProduceView, Produced, Visibility,
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

impl Context {
    fn compacts(&self) -> bool {
        self.cleanup.split(',').any(|p| p.trim() == "compact")
    }

    fn count(&self) -> i32 {
        i32::try_from(self.facts.partitions.len()).unwrap_or(i32::MAX)
    }
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

/// What a person typed, as the record would carry it: an empty key or value is none, and a header needs a name.
struct Typed {
    key: Option<String>,
    value: Option<String>,
    headers: Vec<(String, String)>,
}

impl Typed {
    fn of(i: &ProduceInput) -> Self {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        Self {
            key: some(&i.key),
            value: some(&i.value),
            headers: i
                .headers
                .iter()
                .filter(|(n, _)| !n.is_empty())
                .cloned()
                .collect(),
        }
    }
}

fn acks_text(i: &ProduceInput) -> &str {
    if i.acks.trim().is_empty() {
        "-1"
    } else {
        i.acks.trim()
    }
}

fn chosen(ctx: &Context, input: &ProduceInput, key: Option<&str>) -> Result<i32, String> {
    Ok(validate::partition_choice(&input.partition, ctx.count())?
        .unwrap_or_else(|| partitioner::for_key(key.map(str::as_bytes), ctx.count())))
}

/// The dialog's view-model for these values, with `partition` as the call draws it.
fn input_view(
    b: &Backend,
    topic: &str,
    ctx: &Context,
    input: &ProduceInput,
    partition: i32,
) -> ProduceView {
    let typed = Typed::of(input);
    ProduceView {
        topic: topic.to_owned(),
        partitions: ctx.count(),
        compacts: ctx.compacts(),
        cleanup: ctx.cleanup.clone(),
        partition: input.partition.clone(),
        key: input.key.clone(),
        value: input.value.clone(),
        headers: input.headers.clone(),
        acks: acks_text(input).to_owned(),
        call: calls::produce(
            b.version(ApiKey::Produce),
            topic,
            partition,
            validate::acks(acks_text(input)).unwrap_or(-1),
            &RecordText {
                key: typed.key.as_deref(),
                value: typed.value.as_deref(),
                headers: &typed.headers,
            },
        ),
        errors: ProduceErrors::default(),
        transaction: None,
    }
}

/// The dialog for a topic, with what was typed in it; `None` if there is no such topic.
pub async fn form(b: &Backend, topic: &str, input: &ProduceInput) -> Option<ProduceView> {
    let ctx = context(b, topic).await?;
    let typed = Typed::of(input);
    let partition = chosen(&ctx, input, typed.key.as_deref()).unwrap_or(0);
    let mut view = input_view(b, topic, &ctx, input, partition);
    view.transaction = forms::transactions(b, topic)
        .await
        .into_iter()
        .next()
        .map(|(id, _)| id);
    Some(view)
}

/// Frames the record, sends it, and says what the broker answered.
pub async fn send(b: &Backend, topic: &str, input: &ProduceInput) -> ProduceResult {
    let Some(ctx) = context(b, topic).await else {
        return ProduceResult::NoTopic;
    };
    let typed = Typed::of(input);
    let acks = validate::acks(acks_text(input));
    let partition = chosen(&ctx, input, typed.key.as_deref());
    let (Ok(acks), Ok(partition)) = (&acks, &partition) else {
        let mut view = input_view(b, topic, &ctx, input, 0);
        view.errors = ProduceErrors {
            acks: acks.err(),
            partition: partition.err(),
            key: None,
        };
        return ProduceResult::Invalid(Box::new(view));
    };
    let (acks, partition) = (*acks, *partition);
    let now = crate::source::now_ms();
    let pairs: Vec<(&str, &[u8])> = typed
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_bytes()))
        .collect();
    let batch = single_record(&OneRecord {
        timestamp_ms: now,
        key: typed.key.as_deref().map(str::as_bytes),
        value: typed.value.as_deref().map(str::as_bytes),
        headers: &pairs,
    });
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
    let view = input_view(b, topic, &ctx, input, partition);
    if p.error_code != 0 {
        return ProduceResult::Refused(Box::new(refused(b, view, &ctx, &typed, &p)));
    }
    let at = (partition, now, acks);
    ProduceResult::Sent(Box::new(sent_page(b, topic, &ctx, &p, at, &typed).await))
}

fn refused(
    b: &Backend,
    mut view: ProduceView,
    ctx: &Context,
    typed: &Typed,
    p: &PartitionProduceResponse,
) -> ProduceRefused {
    let said = p.error_message.as_ref().map(ToString::to_string);
    let keyless = typed.key.is_none();
    let reading = refusal::produce(
        p.error_code,
        said.as_deref(),
        view.compacts,
        keyless,
        ctx.limit,
    );
    if p.error_code == 87 && view.compacts && keyless {
        view.errors.key = Some("A compacted topic needs a key on every record.".to_owned());
    }
    let partition = p.index;
    let outcome = Outcome {
        code: p.error_code,
        name: &reading.name,
        base_offset: p.base_offset,
    };
    ProduceRefused {
        answer: calls::produce_answer(
            b.version(ApiKey::Produce),
            &view.topic,
            partition,
            outcome,
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
    }
}

async fn sent_page(
    b: &Backend,
    topic: &str,
    ctx: &Context,
    p: &PartitionProduceResponse,
    (partition, now, acks): (i32, i64, i16),
    typed: &Typed,
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
    let outcome = Outcome {
        code: 0,
        name: "NONE",
        base_offset: p.base_offset,
    };
    let value = typed.value.clone().unwrap_or_default();
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
            key: typed.key.clone().unwrap_or_default(),
            size: u64::try_from(value.len()).unwrap_or(0),
            value,
            committed: p.base_offset < lso,
        },
        others: Vec::new(),
        visibility: Visibility {
            last_stable: lso,
            high_watermark: hw,
            transaction_on_partition: transactions.iter().any(|(_, ps)| ps.contains(&partition)),
        },
        answer: calls::produce_answer(b.version(ApiKey::Produce), topic, partition, outcome, &more),
    }
}
