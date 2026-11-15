//! A client over a quota (KIP-219): its answer comes at once with the throttle time, and nothing more is read from it until the
//! throttle is over (behaviours QT-11). Time is paused, so the waits cost nothing.

use kafka_broker::{Caller, Outcome};
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::header::RequestHead;
use kafka_wire::messages::alter_client_quotas_request::{EntityData, EntryData, OpData};
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{
    AlterClientQuotasRequest, CreateTopicsRequest, ProduceRequest, ProduceResponse, TopicName,
};

use super::*;

fn head(key: ApiKey, version: i16) -> RequestHead {
    RequestHead {
        key,
        version,
        correlation_id: 1,
        client_id: None,
    }
}

/// A broker with topic `t` and a `producer_byte_rate` of 100 bytes a second for client id `p`.
async fn throttling() -> Context<SimFs> {
    let ctx = context(Timeouts::default());
    let console = Caller::anonymous(None);
    let mut topic = CreatableTopic::default();
    topic.name = TopicName(StrBytes::from_static_str("t"));
    topic.num_partitions = 1;
    topic.replication_factor = 1;
    let mut create = CreateTopicsRequest::default();
    create.topics.push(topic);
    let at = ctx.listener.clone();
    let made = ctx
        .broker
        .handle(
            &at,
            &console,
            &head(ApiKey::CreateTopics, 7),
            Request::CreateTopics(create),
        )
        .await;
    let Outcome::Respond(Response::CreateTopics(made)) = made else {
        panic!("not CreateTopics: {made:?}")
    };
    assert_eq!(made.topics[0].error_code, 0, "{made:?}");
    let mut part = EntityData::default();
    part.entity_type = StrBytes::from_static_str("client-id");
    part.entity_name = Some(StrBytes::from_static_str("p"));
    let mut op = OpData::default();
    op.key = StrBytes::from_static_str("producer_byte_rate");
    op.value = 100.0;
    let mut entry = EntryData::default();
    entry.entity.push(part);
    entry.ops.push(op);
    let mut quota = AlterClientQuotasRequest::default();
    quota.entries.push(entry);
    let set = ctx
        .broker
        .handle(
            &at,
            &console,
            &head(ApiKey::AlterClientQuotas, 1),
            Request::AlterClientQuotas(quota),
        )
        .await;
    let Outcome::Respond(Response::AlterClientQuotas(r)) = set else {
        panic!("not AlterClientQuotas: {set:?}")
    };
    assert_eq!(r.entries[0].error_code, 0);
    ctx
}

fn produce(client_id: &str) -> Bytes {
    let mut p = PartitionProduceData::default();
    p.index = 0;
    p.records = Some(BatchBuilder::new().records(200).build());
    let mut t = TopicProduceData::default();
    t.name = TopicName(StrBytes::from_static_str("t"));
    t.partition_data.push(p);
    let mut req = ProduceRequest::default();
    req.acks = 1;
    req.timeout_ms = 1000;
    req.topic_data.push(t);
    framed(&request_payload(
        ApiKey::Produce,
        9,
        7,
        Some(client_id),
        &req,
    ))
}

async fn read_produce(client: &mut DuplexStream) -> ProduceResponse {
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    client.read_exact(&mut payload).await.unwrap();
    let mut payload = Bytes::from(payload);
    let _ = payload.split_to(4);
    // Produce v9 is flexible: its response header has tagged fields after the correlation id.
    let _ = payload.split_to(1);
    ProduceResponse::decode(&mut payload, 9).unwrap()
}

#[tokio::test(start_paused = true)]
async fn a_throttled_client_is_not_read_from_until_its_throttle_is_over() {
    let ctx = throttling().await;
    let (mut client, _task, _stop) = start_on(ctx, 1 << 20);
    // The produce and an ApiVersions behind it, sent together.
    let begun = Instant::now();
    client
        .write_all(&[produce("p"), api_versions(8)].concat())
        .await
        .unwrap();
    let answer = read_produce(&mut client).await;
    let throttle = u64::try_from(answer.throttle_time_ms).unwrap();
    assert!(
        throttle > 1000,
        "a batch of 200 records over ten seconds is far past 100 B/s: {throttle}"
    );
    assert!(
        begun.elapsed() < Duration::from_millis(throttle),
        "the answer comes at once"
    );
    assert_eq!(
        answer.responses[0].partition_responses[0].error_code, 0,
        "the records are taken"
    );
    let (id, _, _) = read_answer(&mut client).await;
    assert_eq!(id, 8);
    assert!(
        begun.elapsed() >= Duration::from_millis(throttle),
        "the request behind it waited out the throttle: {:?} < {throttle} ms",
        begun.elapsed()
    );
}

#[tokio::test(start_paused = true)]
async fn another_client_id_is_not_held_to_it_and_a_stop_ends_the_wait() {
    let ctx = throttling().await;
    let (mut client, task, stop) = start_on(ctx, 1 << 20);
    client
        .write_all(&[produce("q"), api_versions(8)].concat())
        .await
        .unwrap();
    let begun = Instant::now();
    assert_eq!(read_produce(&mut client).await.throttle_time_ms, 0);
    assert_eq!(read_answer(&mut client).await.0, 8);
    assert!(begun.elapsed() < Duration::from_millis(100));
    client.write_all(&produce("p")).await.unwrap();
    assert!(read_produce(&mut client).await.throttle_time_ms > 0);
    stop.cancel();
    assert!(matches!(task.await.unwrap(), End::Stopped));
}
