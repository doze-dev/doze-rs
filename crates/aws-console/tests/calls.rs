//! The Live record as pages (`GET /api/v1/calls`), answered by the router: a cursor, a limit, what a reader missed, and when it
//! must start over. The keys of each answer are the keys the generated TypeScript type declares.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use aws_gateway::{Did, Finished};
use std::time::Duration;

use aws_peers::BusCall;
use aws_peers::summary::{Kind, Summary};
use serde_json::Value;
use support::{app, get, json, keys, sqs_call, state, ts_keys};

#[tokio::test]
async fn calls_are_paged_by_a_cursor_oldest_first() {
    let s = state(8, &[]);
    let app = app(&s);
    let (status, empty) = json(&app, get("/api/v1/calls")).await;
    assert_eq!(status, 200, "{empty}");
    assert_eq!(keys(&empty), ts_keys("CallsPage"));
    assert_eq!(empty["calls"], serde_json::json!([]));
    assert_eq!(
        (&empty["reset"], &empty["missed"], &empty["more"]),
        (&false.into(), &0.into(), &false.into())
    );
    assert_eq!(keys(&empty["ring"]), ts_keys("RingFill"));
    assert_eq!(
        empty["ring"],
        serde_json::json!({"capacity": 8, "held": 0, "recorded": 0, "bytes": 0})
    );
    let start = empty["cursor"].as_str().unwrap().to_owned();

    s.recorder.record(&sqs_call("CreateQueue", 200, ""), 1_000);
    s.recorder.record(
        &sqs_call(
            "GetQueueUrl",
            400,
            "AWS.SimpleQueueService.NonExistentQueue",
        ),
        2_000,
    );
    s.recorder.record(
        &Finished {
            signing_name: "",
            sdk_id: "",
            action: "",
            access_key_id: "",
            region: "",
            ..sqs_call("", 403, "MissingAuthenticationTokenException")
        },
        3_000,
    );
    let (_, page) = json(&app, get(&format!("/api/v1/calls?since={start}"))).await;
    let calls = page["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3, "{page}");
    assert_eq!(keys(&calls[0]), ts_keys("CallRecord"));
    assert_eq!(
        calls[0],
        serde_json::json!({
            "seq": 1, "at_ms": 1_000, "service": "sqs", "api": "SQS", "action": "CreateQueue", "resource": null,
            "member": null, "caller": "test", "region": "us-east-1", "status": 200, "error_code": null, "outcome": "ok",
            "would_be_denied": false, "duration_us": 1_500, "request_id": "0c6f3a52-7a8b-4c1d-9e2f-000000000000",
            "via": null, "sdk": "boto3 1.34.69", "message": null, "function_error": null, "answer_bytes": null, "summary": null,
        })
    );
    assert_eq!(
        calls[1]["error_code"],
        "AWS.SimpleQueueService.NonExistentQueue"
    );
    assert_eq!(calls[1]["outcome"], "error");
    // A request that named no service: nothing is made up for it.
    for key in [
        "service", "api", "action", "caller", "region", "resource", "member", "via",
    ] {
        assert!(calls[2][key].is_null(), "{key}: {}", calls[2]);
    }
    assert_eq!(page["ring"]["held"], 3);
    assert!(page["ring"]["bytes"].as_u64().unwrap() > 3 * 100, "{page}");
}

#[tokio::test]
async fn a_page_has_a_limit_and_hands_back_the_cursor_of_the_next() {
    let s = state(8, &[]);
    let app = app(&s);
    let (_, empty) = json(&app, get("/api/v1/calls")).await;
    let start = empty["cursor"].as_str().unwrap().to_owned();
    for action in ["CreateQueue", "GetQueueUrl", "SendMessage"] {
        s.recorder.record(&sqs_call(action, 200, ""), 1_000);
    }
    // The newest, when no cursor is given; a limit; and the rest from the cursor a page hands back.
    let (_, newest) = json(&app, get("/api/v1/calls?limit=2")).await;
    let seqs = |p: &Value| -> Vec<u64> {
        p["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["seq"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(seqs(&newest), [2, 3]);
    let (_, first) = json(&app, get(&format!("/api/v1/calls?since={start}&limit=2"))).await;
    assert_eq!(seqs(&first), [1, 2]);
    assert_eq!(first["more"], true);
    let next = first["cursor"].as_str().unwrap();
    let (_, rest) = json(&app, get(&format!("/api/v1/calls?since={next}&limit=2"))).await;
    assert_eq!(seqs(&rest), [3]);
    assert_eq!(rest["more"], false);
    let end = rest["cursor"].as_str().unwrap();
    let (_, nothing) = json(&app, get(&format!("/api/v1/calls?since={end}"))).await;
    assert_eq!(seqs(&nothing), [0_u64; 0]);
    assert_eq!(nothing["cursor"], end);
}

#[tokio::test]
async fn a_reader_is_told_what_it_missed_and_when_to_start_over() {
    let s = state(4, &[]);
    let app = app(&s);
    let (_, empty) = json(&app, get("/api/v1/calls")).await;
    let start = empty["cursor"].as_str().unwrap().to_owned();
    for n in 1..=10 {
        s.recorder.record(&sqs_call("SendMessage", 200, ""), n);
    }
    let (_, page) = json(&app, get(&format!("/api/v1/calls?since={start}"))).await;
    assert_eq!(
        page["calls"].as_array().unwrap().len(),
        4,
        "the ring holds four"
    );
    assert_eq!(
        (&page["missed"], &page["reset"]),
        (&6.into(), &false.into())
    );
    assert_eq!(page["ring"]["recorded"], 10);
    // A cursor of another run of doze-aws, one ahead of this run, and text that is no cursor: start over with the newest.
    for since in ["1.3", &format!("{}.999", s.epoch), "nonsense", ""] {
        let (status, page) = json(&app, get(&format!("/api/v1/calls?since={since}&limit=2"))).await;
        assert_eq!(status, 200, "{since}");
        assert_eq!(page["reset"], true, "{since}: {page}");
        assert_eq!(page["calls"].as_array().unwrap().len(), 2, "{since}");
        assert_eq!(page["missed"], 0, "{since}");
    }
    // A limit is held to the ring.
    let (_, all) = json(&app, get("/api/v1/calls?limit=100000")).await;
    assert_eq!(all["calls"].as_array().unwrap().len(), 4);
    let (_, one) = json(&app, get("/api/v1/calls?limit=0")).await;
    assert_eq!(one["calls"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_s3_call_names_its_object_and_a_refusal_says_which_kind() {
    let s = state(8, &[]);
    let app = app(&s);
    s.recorder.record(
        &Finished {
            signing_name: "s3",
            sdk_id: "S3",
            action: "GetObject",
            resource: "uploads/a/b.txt",
            ..sqs_call("", 403, "AccessDenied")
        },
        1,
    );
    s.recorder
        .record(&sqs_call("AddPermission", 501, "NotImplemented"), 2);
    let (_, page) = json(&app, get("/api/v1/calls")).await;
    let calls = page["calls"].as_array().unwrap();
    assert_eq!(calls[0]["resource"], "uploads/a/b.txt");
    assert_eq!(
        (&calls[0]["service"], &calls[0]["outcome"]),
        (&"s3".into(), &"denied".into())
    );
    assert_eq!(calls[1]["outcome"], "not-built");
}

#[tokio::test]
async fn a_call_soft_iam_let_through_says_so() {
    let s = state(8, &[]);
    let app = app(&s);
    s.recorder.record(
        &Finished {
            would_be_denied: true,
            ..sqs_call("SendMessage", 200, "")
        },
        1,
    );
    let (_, page) = json(&app, get("/api/v1/calls")).await;
    let call = &page["calls"][0];
    assert_eq!(
        (&call["outcome"], &call["would_be_denied"]),
        (&"ok".into(), &true.into())
    );
}

/// A summary is told with the call: counted words, the text some carry, and none for a call that failed.
#[tokio::test]
async fn a_call_says_what_it_did_in_a_few_words() {
    let s = state(16, &[]);
    let app = app(&s);
    let did = |kind, n, failed, detail| Did {
        summary: Summary::of(kind, n, failed),
        detail,
    };
    for (action, d) in [
        ("ReceiveMessage", did(Kind::Messages, 3, 0, "")),
        ("SendMessage", did(Kind::Sent, 53, 0, "")),
        ("AssumeRole", did(Kind::Session, 0, 0, "report-job")),
        ("DeleteMessage", Did::NONE),
    ] {
        s.recorder.record(
            &Finished {
                did: d,
                ..sqs_call(action, 200, "")
            },
            1,
        );
    }
    // What a failed call carried is not said: the gateway tells none, and the words of an error are an error's.
    s.recorder.record(
        &Finished {
            message: "The specified queue does not exist.",
            did: did(Kind::Messages, 1, 0, "left over"),
            ..sqs_call("ReceiveMessage", 400, "QueueDoesNotExist")
        },
        2,
    );
    let (_, page) = json(&app, get("/api/v1/calls")).await;
    let said: Vec<(&str, &Value, &Value)> = page["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["action"].as_str().unwrap(), &c["summary"], &c["message"]))
        .collect();
    assert_eq!(
        said[0],
        ("ReceiveMessage", &"3 messages".into(), &Value::Null)
    );
    assert_eq!(said[1], ("SendMessage", &"sent 53 B".into(), &Value::Null));
    assert_eq!(
        said[2],
        ("AssumeRole", &"session report-job".into(), &Value::Null)
    );
    assert_eq!(said[3], ("DeleteMessage", &Value::Null, &Value::Null));
    assert_eq!(said[4].2, "The specified queue does not exist.");
}

fn delivery<'a>(request: &'a aws_peers::Cause, status: u16, code: &'a str) -> BusCall<'a> {
    BusCall {
        acting: "sns",
        signing_name: "sqs",
        sdk_id: "SQS",
        operation: "SendMessage",
        member: "QueueUrl",
        resource: "http://localhost/000000000000/orders",
        access_key_id: "test",
        region: "us-east-1",
        status,
        code,
        took: Duration::from_micros(10),
        cause: Some(request),
    }
}

/// A publish says how many deliveries SNS made for it over the bus, from the calls recorded for its request.
#[tokio::test]
async fn a_publish_counts_the_deliveries_made_for_it() {
    let s = state(32, &[]);
    let app = app(&s);
    let cause = |request: &str| aws_peers::Cause {
        request: Some(request.into()),
        name: None,
    };
    let mine = cause("0c6f3a52-7a8b-4c1d-9e2f-000000000000");
    let other = cause("another-request");
    for (cause, status, code) in [
        (&mine, 200, ""),
        (&other, 200, ""),
        (&mine, 200, ""),
        (&mine, 404, "QueueDoesNotExist"),
    ] {
        s.recorder.record_bus(&delivery(cause, status, code), 1);
    }
    let publish = |action, request_id| Finished {
        request_id,
        signing_name: "sns",
        sdk_id: "SNS",
        action,
        did: Did {
            summary: Summary::of(Kind::Delivered, 0, 0),
            detail: "",
        },
        ..sqs_call(action, 200, "")
    };
    s.recorder.record(
        &publish("Publish", "0c6f3a52-7a8b-4c1d-9e2f-000000000000"),
        2,
    );
    s.recorder
        .record(&publish("Publish", "a-publish-nobody-delivered-for"), 3);
    let (_, page) = json(&app, get("/api/v1/calls")).await;
    let said: Vec<&Value> = page["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["action"] == "Publish")
        .map(|c| &c["summary"])
        .collect();
    assert_eq!(said, ["delivered to 2, 1 failed", "no deliveries"]);
}
