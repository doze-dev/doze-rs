#![allow(clippy::unwrap_used)]
use aws_wire::service::Caller;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::json;

use super::*;

fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost".into(),
        service: None,
        session_token: None,
    }
}

fn bus(dir: &std::path::Path) -> Arc<Peers> {
    let peers = Peers::new();
    let ctx = Ctx {
        dir: dir.to_path_buf(),
        idle: std::time::Duration::from_secs(60),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    };
    peers.register(Arc::new(aws_kms::Kms::new(Arc::new(ctx))));
    peers
}

#[tokio::test]
async fn a_call_goes_through_the_target_model_and_back() {
    let dir = std::env::temp_dir().join(format!("doze-aws-peers-{}", std::process::id()));
    let peers = bus(&dir);
    let made = peers
        .call("test", &caller(), "kms", "CreateKey", &json!({}))
        .await
        .unwrap();
    assert_eq!(made["KeyMetadata"]["KeyState"], "Enabled");
    // The target's own checks apply to the body, as they would to a request from the wire.
    let bad = peers
        .call(
            "test",
            &caller(),
            "kms",
            "ScheduleKeyDeletion",
            &json!({"PendingWindowInDays": 9}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad.code, "ValidationException");
    // An operation the target refuses by name is refused here too.
    let refused = peers
        .call("test", &caller(), "kms", "CreateCustomKeyStore", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 501);
    assert!(
        peers
            .call("test", &caller(), "nope", "X", &json!({}))
            .await
            .unwrap_err()
            .message
            .contains("no service nope")
    );
    assert!(
        peers
            .call("test", &caller(), "kms", "Nope", &json!({}))
            .await
            .unwrap_err()
            .message
            .contains("no operation Nope")
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[tokio::test]
async fn state_is_kept_apart_by_the_region_a_call_was_signed_for() {
    let dir = std::env::temp_dir().join(format!("doze-aws-regions-{}", std::process::id()));
    let peers = bus(&dir);
    let in_region = |region: &str| Caller {
        region: region.into(),
        ..caller()
    };
    let made = peers
        .call(
            "test",
            &in_region("eu-west-1"),
            "kms",
            "CreateKey",
            &json!({}),
        )
        .await
        .unwrap();
    let id = made["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
    let seen = |region: &str| {
        let (peers, caller) = (Arc::clone(&peers), in_region(region));
        async move {
            peers
                .call("test", &caller, "kms", "ListKeys", &json!({}))
                .await
                .unwrap()["Keys"]
                .as_array()
                .unwrap()
                .len()
        }
    };
    assert_eq!((seen("eu-west-1").await, seen("us-east-1").await), (1, 0));
    let elsewhere = peers
        .call(
            "test",
            &in_region("us-east-1"),
            "kms",
            "DescribeKey",
            &json!({"KeyId": id}),
        )
        .await
        .unwrap_err();
    assert_eq!(elsewhere.code, "NotFoundException");
    // The region the files are kept in for itself is the home region's: the others go in a directory of their own.
    assert!(dir.join("regions").join("eu-west-1").exists());
    drop(std::fs::remove_dir_all(&dir));
}

#[derive(Default)]
struct Seen(std::sync::Mutex<Vec<String>>);

impl CallWatcher for Seen {
    fn called(&self, c: &BusCall<'_>) {
        let cause = c.cause.map_or(String::from("-"), |c| {
            format!(
                "{}|{}",
                c.request.as_deref().unwrap_or("-"),
                c.name.as_deref().unwrap_or("-")
            )
        });
        self.0.lock().unwrap().push(format!(
            "{} {} {} {}={} {} {} {}",
            c.acting, c.signing_name, c.operation, c.member, c.resource, c.status, c.code, cause
        ));
    }
}

#[tokio::test]
async fn a_watcher_is_told_each_bus_call_with_what_it_acted_on_and_what_caused_it() {
    let dir = std::env::temp_dir().join(format!("doze-aws-watched-{}", std::process::id()));
    let peers = bus(&dir);
    // Nobody watching: nothing to tell, nothing kept.
    let made = peers
        .call("test", &caller(), "kms", "CreateKey", &json!({}))
        .await
        .unwrap();
    let id = made["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
    let seen = Arc::new(Seen::default());
    peers.watch_calls(seen.clone());
    let body = json!({"KeyId": id});
    seen::within_request(
        "req-1",
        peers.call("sns", &caller(), "kms", "DescribeKey", &body),
    )
    .await
    .unwrap();
    seen::named(
        "rule nightly",
        peers.call_as(&caller(), "kms", "DescribeKey", &json!({"KeyId": "nope"})),
    )
    .await
    .unwrap_err();
    peers
        .call("ssm", &caller(), "kms", "ListKeys", &json!({}))
        .await
        .unwrap();
    let told = seen.0.lock().unwrap().clone();
    assert_eq!(
        told,
        [
            format!("sns kms DescribeKey KeyId={id} 200  req-1|-"),
            " kms DescribeKey KeyId=nope 400 NotFoundException -|rule nightly".to_owned(),
            "ssm kms ListKeys = 200  -".to_owned(),
        ]
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn background_polls_are_known_by_service_and_operation() {
    assert!(seen::is_poll("sqs", "ReceiveMessage"));
    assert!(seen::is_poll("kinesis", "GetRecords"));
    assert!(!seen::is_poll("sqs", "SendMessage"));
    assert!(!seen::is_poll("lambda", "Invoke"));
}

#[tokio::test]
async fn work_a_person_asked_for_is_known_as_asked() {
    assert!(!seen::is_asked());
    assert!(seen::asked(async { seen::is_asked() }).await);
    assert!(!seen::is_asked());
}

#[tokio::test]
async fn an_operation_that_does_not_stream_is_not_called_as_a_stream() {
    let dir = std::env::temp_dir().join(format!("doze-aws-peers-stream-{}", std::process::id()));
    let peers = bus(&dir);
    // (`err().unwrap()`: a stream is not `Debug`.)
    let e = peers
        .call_stream_as(&caller(), "kms", "ListKeys", &json!({}))
        .await
        .err()
        .unwrap();
    assert!(e.message.contains("does not stream"), "{}", e.message);
    drop(std::fs::remove_dir_all(&dir));
}
