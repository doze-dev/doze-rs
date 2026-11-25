//! Forced events (the console's Lab): a table's time-to-live sweep, now, by the function the region's timer runs.

use std::sync::Arc;
use std::time::Duration;

use aws_peers::{Authorizer, Mode, Need, Peers};
use aws_testkit::{Harness, TempDir};
use aws_wire::forced::{Done, Step, kind};
use aws_wire::service::{BoxFuture, Caller, Service};
use doze_runtime::{Ctx, ManualClock, Tasks};
use serde_json::{Value, json};

use super::World;
use crate::Dynamodb;
use crate::core::Request;
use crate::forced::Sweep;
use crate::lifecycle::SWEEP_BATCH;

const NOW_MS: i64 = 1_700_000_000_000;
const SECS: i64 = NOW_MS / 1000;

fn table(w: &mut World, name: &str, stream: bool) {
    let mut t = json!({"TableName": name, "BillingMode": "PAY_PER_REQUEST",
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
        "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]});
    if stream {
        t["StreamSpecification"] = json!({"StreamEnabled": true, "StreamViewType": "OLD_IMAGE"});
    }
    w.ok("CreateTable", t);
    w.ok(
        "UpdateTimeToLive",
        json!({"TableName": name, "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "exp"}}),
    );
}

fn put(w: &mut World, name: &str, pk: &str, expires_in: Option<i64>) {
    let mut item = json!({"pk": {"S": pk}});
    if let Some(s) = expires_in {
        item["exp"] = json!({"N": (SECS + s).to_string()});
    }
    w.put(name, item);
}

fn keys(w: &mut World, name: &str) -> Vec<String> {
    let mut found: Vec<String> = w.ok("Scan", json!({"TableName": name}))["Items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["pk"]["S"].as_str().unwrap().to_owned())
        .collect();
    found.sort();
    found
}

fn sweep(w: &mut World, name: &str) -> Result<Sweep, aws_wire::ApiError> {
    let answer = w.rt.block_on(doze_runtime::Service::handle(
        w.core.as_mut().unwrap(),
        Request::Sweep {
            table: name.to_owned(),
        },
    ));
    answer.swept.unwrap()
}

#[test]
fn the_sweep_removes_the_expired_items_and_not_the_live_ones() {
    let mut w = World::new();
    table(&mut w, "sessions", false);
    put(&mut w, "sessions", "soon", Some(60));
    put(&mut w, "sessions", "later", Some(3600));
    put(&mut w, "sessions", "never", None);
    // Time passes with no call: nothing has swept yet.
    w.clock.set((SECS + 61) * 1000);
    let Sweep::Ran {
        removed,
        elsewhere,
        left,
        appended,
    } = sweep(&mut w, "sessions").unwrap()
    else {
        panic!("it should have run");
    };
    assert_eq!((removed, elsewhere, left, appended), (1, 0, 0, false));
    assert_eq!(keys(&mut w, "sessions"), ["later", "never"]);
}

#[test]
fn it_is_what_the_timer_does() {
    let (mut forced, mut ticked) = (World::new(), World::new());
    for w in [&mut forced, &mut ticked] {
        table(w, "tab-a", true);
        table(w, "tab-b", false);
        for (t, pk, s) in [
            ("tab-a", "x", Some(10)),
            ("tab-a", "y", Some(900)),
            ("tab-b", "z", Some(20)),
            ("tab-b", "w", None),
        ] {
            put(w, t, pk, s);
        }
        w.clock.set((SECS + 30) * 1000);
    }
    let Sweep::Ran {
        removed,
        elsewhere,
        appended,
        ..
    } = sweep(&mut forced, "tab-a").unwrap()
    else {
        panic!("it should have run");
    };
    assert_eq!(
        (removed, elsewhere, appended),
        (1, 1, true),
        "tab-a's item, and tab-b's, as the region's sweep covers both"
    );
    let answer = ticked.rt.block_on(doze_runtime::Service::handle(
        ticked.core.as_mut().unwrap(),
        Request::Tick,
    ));
    assert!(answer.appended);
    for t in ["tab-a", "tab-b"] {
        assert_eq!(keys(&mut forced, t), keys(&mut ticked, t), "{t}");
    }
    assert_eq!(keys(&mut forced, "tab-a"), ["y"]);
    assert_eq!(keys(&mut forced, "tab-b"), ["w"]);
}

#[test]
fn with_nothing_expired_it_says_when_the_next_item_expires() {
    let mut w = World::new();
    table(&mut w, "sessions", false);
    assert!(matches!(
        sweep(&mut w, "sessions").unwrap(),
        Sweep::Nothing { next_ms: None }
    ));
    put(&mut w, "sessions", "a", Some(120));
    put(&mut w, "sessions", "b", Some(60));
    let Sweep::Nothing { next_ms } = sweep(&mut w, "sessions").unwrap() else {
        panic!("nothing is expired");
    };
    assert_eq!(next_ms, Some((SECS + 60) * 1000));
    assert_eq!(keys(&mut w, "sessions"), ["a", "b"], "nothing was removed");
    let Step::Nothing(why) = crate::forced::words_for_tests("sessions", Sweep::Nothing { next_ms })
    else {
        panic!("a refusal");
    };
    assert!(
        why.starts_with("No item of sessions has passed its time to live"),
        "{why}"
    );
    assert!(
        why.contains("The next item expires at 2023-11-14 22:14:20 UTC."),
        "{why}"
    );
    assert!(
        why.contains("typically deletes expired items within two days of expiration"),
        "{why}"
    );
}

#[test]
fn a_table_without_time_to_live_and_a_table_that_is_not_there_are_said() {
    let mut w = World::new();
    w.table("plain", false);
    assert!(matches!(sweep(&mut w, "plain").unwrap(), Sweep::Off));
    assert_eq!(
        sweep(&mut w, "nope").unwrap_err().code,
        "ResourceNotFoundException"
    );
}

#[test]
fn a_sweep_removes_one_batch_and_says_how_many_are_still_expired() {
    let mut w = World::new();
    table(&mut w, "big", false);
    let total = SWEEP_BATCH + 40;
    for chunk in (0..total).collect::<Vec<_>>().chunks(25) {
        let requests: Vec<Value> = chunk
            .iter()
            .map(|n| json!({"PutRequest": {"Item": {"pk": {"S": format!("k{n:04}")}, "exp": {"N": (SECS + 5).to_string()}}}}))
            .collect();
        w.ok("BatchWriteItem", json!({"RequestItems": {"big": requests}}));
    }
    w.clock.set((SECS + 10) * 1000);
    let Sweep::Ran { removed, left, .. } = sweep(&mut w, "big").unwrap() else {
        panic!("it should have run");
    };
    assert_eq!((removed, left), (SWEEP_BATCH, 40));
    let Step::Done(Done { sentence, .. }) = crate::forced::words_for_tests(
        "big",
        Sweep::Ran {
            removed,
            elsewhere: 0,
            left,
            appended: false,
        },
    ) else {
        panic!("it ran");
    };
    assert!(sentence.contains("40 more items are still expired: a sweep removes one batch, and the timer carries on with the rest."), "{sentence}");
}

struct Refuses(&'static str);

impl Authorizer for Refuses {
    fn authorize<'a>(
        &'a self,
        caller: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), aws_wire::ApiError>> {
        Box::pin(async move {
            if caller.access_key_id == self.0 {
                let what = needs
                    .first()
                    .map(|n| format!("{} on {}", n.action, n.resource));
                return Err(aws_wire::ApiError::new(
                    400,
                    "AccessDeniedException",
                    what.unwrap_or_default(),
                ));
            }
            Ok(())
        })
    }
}

fn caller(key: &str) -> Caller {
    Caller {
        access_key_id: key.into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one story: a service, a refusal, a sweep, a second sweep
async fn a_real_service_sweeps_and_a_caller_without_the_right_is_refused() {
    let dir = TempDir::new("ddb-forced");
    let clock = Arc::new(ManualClock::at(NOW_MS));
    let peers = Peers::new();
    peers.set_authorizer(Arc::new(Refuses("intruder")));
    peers.set_mode(Mode::Enforce);
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers;
    let tasks = Tasks::new();
    let svc = Dynamodb::new(Arc::new(Ctx {
        dir: dir.path().to_path_buf(),
        idle: Duration::from_secs(3600),
        tasks: tasks.clone(),
        clock: clock.clone(),
        ext: Some(ext),
    }));
    let h = Harness::new(Dynamodb::new(Arc::new(Ctx {
        dir: TempDir::new("ddb-forced-decode").path().to_path_buf(),
        idle: Duration::from_secs(60),
        tasks: Tasks::new(),
        clock: clock.clone(),
        ext: None,
    })));
    let call = |op: &str, body: Value| {
        let ix = h.op(op);
        let call = h.call(ix, &body).unwrap();
        let (svc, h) = (&svc, &h);
        async move {
            let mut call = call;
            call.caller = caller("console");
            h.json(ix, &svc.call(call).await.unwrap())
        }
    };
    call(
        "CreateTable",
        json!({"TableName": "sessions", "BillingMode": "PAY_PER_REQUEST",
            "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
            "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}),
    )
    .await;
    call(
        "UpdateTimeToLive",
        json!({"TableName": "sessions", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "exp"}}),
    )
    .await;
    for (pk, s) in [("old", 60), ("new", 7200)] {
        call(
            "PutItem",
            json!({"TableName": "sessions", "Item": {"pk": {"S": pk}, "exp": {"N": (SECS + s).to_string()}}}),
        )
        .await;
    }
    // Time passes, and the timer has not run (it is the timer's task, a minute away on the real clock).
    clock.set((SECS + 61) * 1000);
    let input = json!({"table": "sessions"});
    let forced = svc.forced().unwrap();
    let e = forced
        .force(&caller("intruder"), kind::SWEEP_TTL, &input)
        .await
        .unwrap_err();
    assert_eq!(e.code, "AccessDeniedException");
    assert!(
        e.message.contains(
            "dynamodb:DeleteItem on arn:aws:dynamodb:us-east-1:000000000000:table/sessions"
        ),
        "{}",
        e.message
    );

    let step = forced
        .force(&caller("console"), kind::SWEEP_TTL, &input)
        .await
        .unwrap();
    let Step::Done(d) = step else {
        panic!("it should have swept")
    };
    assert_eq!(d.subject, "sessions");
    assert!(
        d.sentence
            .starts_with("Ran the time-to-live sweep: removed 1 expired item of sessions."),
        "{}",
        d.sentence
    );
    let left = call("Scan", json!({"TableName": "sessions"})).await;
    assert_eq!(left["Count"], 1);
    assert_eq!(left["Items"][0]["pk"]["S"], "new");
    // Again: nothing is expired.
    let again = forced
        .force(&caller("console"), kind::SWEEP_TTL, &input)
        .await
        .unwrap();
    assert!(matches!(again, Step::Nothing(_)));
    tasks.shutdown().await;
}
