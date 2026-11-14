//! Time to live without traffic: a timer, running only while an item has a time to live, deletes it when it is due and its stream
//! gets the REMOVE record AWS writes for the service. And Contributor Insights, stored with the statuses AWS reports.

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::{Harness, TempDir};
use aws_wire::service::Service;
use doze_runtime::{Ctx, ManualClock, Tasks};
use serde_json::{Value, json};

use super::{World, message};
use crate::Dynamodb;
use crate::core::Request;

const NOW_MS: i64 = 1_700_000_000_000;

fn table_with_stream() -> Value {
    json!({"TableName": "sessions", "BillingMode": "PAY_PER_REQUEST",
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
        "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}],
        "StreamSpecification": {"StreamEnabled": true, "StreamViewType": "OLD_IMAGE"}})
}

#[test]
fn the_deadline_is_the_first_expiry_and_a_tick_deletes_what_is_due() {
    let mut w = World::new();
    w.ok("CreateTable", table_with_stream());
    w.ok(
        "UpdateTimeToLive",
        json!({"TableName": "sessions", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "exp"}}),
    );
    let core = w.core.as_mut().unwrap();
    assert_eq!(core.ttl_due(), None, "no item has a time to live: no timer");
    let secs = NOW_MS / 1000;
    w.put(
        "sessions",
        json!({"pk": {"S": "late"}, "exp": {"N": (secs + 120).to_string()}}),
    );
    w.put(
        "sessions",
        json!({"pk": {"S": "soon"}, "exp": {"N": (secs + 60).to_string()}}),
    );
    w.put("sessions", json!({"pk": {"S": "never"}}));
    assert_eq!(w.core.as_mut().unwrap().ttl_due(), Some((secs + 60) * 1000));

    w.clock.set((secs + 61) * 1000);
    let answer = w.rt.block_on(doze_runtime::Service::handle(
        w.core.as_mut().unwrap(),
        Request::Tick,
    ));
    assert!(answer.appended, "the deletion is a stream record");
    assert_eq!(answer.next_due, Some((secs + 120) * 1000));
    let left = w.ok("Scan", json!({"TableName": "sessions"}));
    assert_eq!(left["Count"], 2);

    w.clock.set((secs + 121) * 1000);
    let answer = w.rt.block_on(doze_runtime::Service::handle(
        w.core.as_mut().unwrap(),
        Request::Tick,
    ));
    assert_eq!(answer.next_due, None, "nothing left with a time to live");
}

#[tokio::test(start_paused = true)]
async fn an_expired_item_goes_with_no_call_and_its_remove_record_names_the_service() {
    let dir = TempDir::new("ddb-ttl");
    let clock = Arc::new(ManualClock::at(NOW_MS));
    let tasks = Tasks::new();
    let svc = Dynamodb::new(Arc::new(Ctx {
        dir: dir.path().to_path_buf(),
        idle: Duration::from_secs(3600),
        tasks: tasks.clone(),
        clock: clock.clone(),
        ext: None,
    }));
    let h = Harness::new(Dynamodb::new(Arc::new(Ctx {
        dir: TempDir::new("ddb-decode").path().to_path_buf(),
        idle: Duration::from_secs(60),
        tasks: Tasks::new(),
        clock: clock.clone(),
        ext: None,
    })));
    let call = |op: &str, body: Value| {
        let ix = h.op(op);
        let call = h.call(ix, &body).unwrap();
        let svc = &svc;
        let h = &h;
        async move { h.json(ix, &svc.call(call).await.unwrap()) }
    };
    let made = call("CreateTable", table_with_stream()).await;
    let arn = made["TableDescription"]["LatestStreamArn"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        "UpdateTimeToLive",
        json!({"TableName": "sessions", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "exp"}}),
    )
    .await;
    assert!(!svc.expiry_armed("us-east-1"), "no item has a time to live");
    let secs = NOW_MS / 1000;
    call(
        "PutItem",
        json!({"TableName": "sessions", "Item": {"pk": {"S": "a"}, "exp": {"N": (secs + 60).to_string()}}}),
    )
    .await;
    assert!(svc.expiry_armed("us-east-1"));

    clock.set((secs + 61) * 1000);
    tokio::time::sleep(Duration::from_secs(61)).await;
    // The tick's write is the writer thread's, in real time: give it a moment.
    for _ in 0..2000 {
        if !svc.expiry_armed("us-east-1") {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
        tokio::task::yield_now().await;
    }
    assert!(
        !svc.expiry_armed("us-east-1"),
        "deleted, and nothing left to wait for"
    );
    let batch = svc.stream_records("us-east-1", &arn, 0, 10).await.unwrap();
    let remove = batch.records.last().unwrap().to_event("us-east-1");
    assert_eq!(remove["eventName"], "REMOVE");
    assert_eq!(
        remove["userIdentity"],
        json!({"type": "Service", "principalId": "dynamodb.amazonaws.com"})
    );
    tasks.shutdown().await;
}

#[test]
fn contributor_insights_are_stored_with_the_statuses_aws_reports() {
    let mut w = World::new();
    w.ok("CreateTable", table_with_stream());
    let d = w.ok(
        "DescribeContributorInsights",
        json!({"TableName": "sessions"}),
    );
    assert_eq!(
        d,
        json!({"TableName": "sessions", "ContributorInsightsStatus": "DISABLED"})
    );
    let on = w.ok(
        "UpdateContributorInsights",
        json!({"TableName": "sessions", "ContributorInsightsAction": "ENABLE"}),
    );
    assert_eq!(on["ContributorInsightsStatus"], "ENABLING");
    let d = w.ok(
        "DescribeContributorInsights",
        json!({"TableName": "sessions"}),
    );
    assert_eq!(d["ContributorInsightsStatus"], "ENABLED");
    assert_eq!(d["ContributorInsightsMode"], "ACCESSED_AND_THROTTLED_KEYS");
    assert!(d.get("LastUpdateDateTime").is_some());
    assert!(
        d.get("ContributorInsightsRuleList").is_none(),
        "no rule exists here"
    );

    let listed = w.ok("ListContributorInsights", json!({}));
    assert_eq!(
        listed["ContributorInsightsSummaries"],
        json!([{"TableName": "sessions", "ContributorInsightsStatus": "ENABLED", "ContributorInsightsMode": "ACCESSED_AND_THROTTLED_KEYS"}])
    );
    w.restart();
    let off = w.ok(
        "UpdateContributorInsights",
        json!({"TableName": "sessions", "ContributorInsightsAction": "DISABLE"}),
    );
    assert_eq!(off["ContributorInsightsStatus"], "DISABLING");
    assert_eq!(
        w.ok(
            "DescribeContributorInsights",
            json!({"TableName": "sessions"})
        )["ContributorInsightsStatus"],
        "DISABLED"
    );

    let e = w.err(
        "UpdateContributorInsights",
        json!({"TableName": "sessions", "IndexName": "nope", "ContributorInsightsAction": "ENABLE"}),
    );
    assert_eq!(e.code, "ResourceNotFoundException");
    let e = w.err(
        "DescribeContributorInsights",
        json!({"TableName": "missing"}),
    );
    assert_eq!(
        message(&e),
        "Requested resource not found: Table: missing not found"
    );

    w.ok("DeleteTable", json!({"TableName": "sessions"}));
    assert_eq!(
        w.ok("ListContributorInsights", json!({}))["ContributorInsightsSummaries"],
        json!([]),
        "a table that goes takes its settings with it"
    );
}

#[test]
fn the_streams_api_lists_a_tables_stream_with_its_shard() {
    let mut w = World::new();
    w.ok("CreateTable", table_with_stream());
    w.put("sessions", json!({"pk": {"S": "a"}}));
    let core = w.core.as_mut().unwrap();
    let streams = core
        .list_streams("us-east-1", "111122223333", None)
        .unwrap();
    assert_eq!(streams.len(), 1);
    let s = &streams[0];
    assert!(
        s.arn
            .starts_with("arn:aws:dynamodb:us-east-1:111122223333:table/sessions/stream/")
    );
    assert_eq!(
        (s.view.as_str(), s.last_seq, s.enabled),
        ("OLD_IMAGE", 1, true)
    );
    assert_eq!(s.created_ms, NOW_MS);
    assert_eq!(s.key_schema, vec![("pk".to_owned(), "HASH")]);
    assert!(s.shard_id.starts_with("shardId-00000001700000000000-"));
    assert_eq!(s.shard_id.len(), "shardId-".len() + 20 + 1 + 8);
    assert!(
        core.list_streams("us-east-1", "111122223333", Some("other"))
            .unwrap()
            .is_empty()
    );
}

/// The windows a host passes (`doze-aws --dynamodb-create-ms`) reach every region's actor.
#[tokio::test]
async fn the_host_s_windows_reach_the_actor() {
    let dir = TempDir::new("ddb-windows");
    let clock = Arc::new(ManualClock::at(NOW_MS));
    let tasks = Tasks::new();
    let svc = Dynamodb::with_windows(
        Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(60),
            tasks: tasks.clone(),
            clock: clock.clone(),
            ext: None,
        }),
        crate::Windows {
            create_ms: 2_000,
            update_ms: 0,
            delete_ms: 0,
        },
    );
    let h = Harness::new(Dynamodb::new(Arc::new(Ctx {
        dir: TempDir::new("ddb-decode-w").path().to_path_buf(),
        idle: Duration::from_secs(60),
        tasks: Tasks::new(),
        clock: clock.clone(),
        ext: None,
    })));
    let call = |op: &str, body: Value| {
        let ix = h.op(op);
        let call = h.call(ix, &body).unwrap();
        let svc = &svc;
        let h = &h;
        async move { h.json(ix, &svc.call(call).await.unwrap()) }
    };
    let made = call("CreateTable", table_with_stream()).await;
    assert_eq!(made["TableDescription"]["TableStatus"], "CREATING");
    clock.advance(2_000);
    let described = call("DescribeTable", json!({"TableName": "sessions"})).await;
    assert_eq!(described["Table"]["TableStatus"], "ACTIVE");
    tasks.shutdown().await;
}
