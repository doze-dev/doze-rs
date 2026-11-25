//! Forced events (the console's Lab): the in-flight messages of a queue come back now, by the function `ChangeMessageVisibility` runs.

use std::time::Duration;

use aws_peers::{Authorizer, Mode, Need, Peers};
use aws_wire::forced::{Done, Step, kind};
use aws_wire::service::{BoxFuture, Caller, Service};
use doze_runtime::{Clock, Ctx, Tasks};

use super::*;
use crate::Sqs;

fn received(w: &mut World, url: &str, max: u32) -> Vec<Value> {
    let got = w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "MaxNumberOfMessages": max, "AttributeNames": ["All"]}),
    );
    got["Messages"].as_array().cloned().unwrap_or_default()
}

fn done(step: Step) -> Done {
    match step {
        Step::Done(d) => d,
        Step::Nothing(why) => panic!("it did nothing: {why}"),
    }
}

fn nothing(step: Step) -> String {
    match step {
        Step::Nothing(why) => why,
        Step::Done(d) => panic!("it did something: {}", d.sentence),
    }
}

#[test]
fn a_forced_timeout_brings_in_flight_messages_back_with_the_receive_count_they_had() {
    let mut w = World::new();
    let url = w.queue("orders");
    for body in ["a", "b"] {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": body}),
        );
    }
    let first = received(&mut w, &url, 10);
    assert_eq!(first.len(), 2);
    // 5 s of the queue's 30 s have passed: both are hidden for 25 s more.
    w.clock.advance(5_000);
    assert!(received(&mut w, &url, 10).is_empty(), "still in flight");

    let step = w.core.release_in_flight("orders").unwrap();
    let d = done(step);
    assert_eq!(d.subject, "orders");
    assert!(
        d.sentence
            .starts_with("Brought back 2 messages that were in flight in orders"),
        "{}",
        d.sentence
    );

    // Receivable at once, by the clock that has not moved, and each has been received once before.
    let again = received(&mut w, &url, 10);
    assert_eq!(again.len(), 2);
    for m in &again {
        assert_eq!(m["Attributes"]["ApproximateReceiveCount"], "2", "{m}");
    }
    // The first receipt handle still names its message: a consumer that was slow can still delete it.
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": url, "ReceiptHandle": first[0]["ReceiptHandle"]}),
    );
    assert_eq!(
        w.ok(
            Operation::GetQueueAttributes,
            &json!({"QueueUrl": url, "AttributeNames": ["ApproximateNumberOfMessages", "ApproximateNumberOfMessagesNotVisible"]})
        )["Attributes"],
        json!({"ApproximateNumberOfMessages": "0", "ApproximateNumberOfMessagesNotVisible": "1"})
    );
}

#[test]
fn it_is_what_a_consumer_does_with_a_visibility_timeout_of_zero() {
    // Two identical queues: one has its message released by the forced event, the other by the call a consumer would make.
    let (mut forced, mut called) = (World::new(), World::new());
    let mut handles = Vec::new();
    for w in [&mut forced, &mut called] {
        let url = w.queue("q");
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": "x"}),
        );
        let got = received(w, &url, 1);
        handles.push((url, got[0]["ReceiptHandle"].as_str().unwrap().to_owned()));
        w.clock.advance(7_000);
    }
    forced.core.release_in_flight("q").unwrap();
    let (url, handle) = &handles[1];
    called.ok(
        Operation::ChangeMessageVisibility,
        &json!({"QueueUrl": url, "ReceiptHandle": handle, "VisibilityTimeout": 0}),
    );
    let state = |w: &World| {
        let m = &w.core.queues["q"].messages[0];
        (
            m.visible_at_ms,
            m.receive_count,
            m.handles.len(),
            m.received_ms,
        )
    };
    assert_eq!(
        state(&forced),
        state(&called),
        "the same state, field for field"
    );
}

#[test]
fn only_the_queue_asked_for_comes_back_and_a_timeout_that_was_not_forced_still_holds() {
    let mut w = World::new();
    let (one, two) = (w.queue("one"), w.queue("two"));
    for url in [&one, &two] {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": "x"}),
        );
        assert_eq!(received(&mut w, url, 1).len(), 1);
    }
    w.core.release_in_flight("one").unwrap();
    assert_eq!(received(&mut w, &one, 1).len(), 1);
    assert!(
        received(&mut w, &two, 1).is_empty(),
        "two keeps its timeout"
    );
    w.clock.advance(29_000);
    assert!(received(&mut w, &two, 1).is_empty(), "29 s of 30");
    w.clock.advance(1_000);
    assert_eq!(
        received(&mut w, &two, 1).len(),
        1,
        "the clock still ends it"
    );
}

#[test]
fn with_nothing_in_flight_it_says_what_the_queue_holds_instead() {
    let mut w = World::new();
    let url = w.queue("orders");
    let why = nothing(w.core.release_in_flight("orders").unwrap());
    assert!(why.starts_with("orders has no message in flight"), "{why}");
    assert!(!why.contains("can be received now"), "{why}");

    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "ready"}),
    );
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "later", "DelaySeconds": 60}),
    );
    let why = nothing(w.core.release_in_flight("orders").unwrap());
    assert!(why.contains("1 message can be received now."), "{why}");
    assert!(
        why.contains("1 message waiting out a delivery delay"),
        "{why}"
    );
    // The delayed message is not touched: it is still delayed.
    assert!(w.core.queues["orders"].messages[1].delayed(w.clock.now_ms()));
    assert_eq!(
        w.core.release_in_flight("missing").unwrap_err().code,
        "QueueDoesNotExist"
    );
}

#[test]
fn a_message_received_as_often_as_the_dead_letter_policy_allows_is_said_to_move_on_the_next_receive()
 {
    let mut w = World::new();
    let dead = w.queue("dead");
    let arn = "arn:aws:sqs:us-east-1:000000000000:dead";
    let url = w.ok(
        Operation::CreateQueue,
        &json!({"QueueName": "jobs", "Attributes": {"RedrivePolicy": json!({"deadLetterTargetArn": arn, "maxReceiveCount": 1}).to_string()}}),
    )["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "poison"}),
    );
    assert_eq!(received(&mut w, &url, 1).len(), 1);
    let d = done(w.core.release_in_flight("jobs").unwrap());
    assert!(
        d.sentence
            .ends_with("1 has reached the queue's maxReceiveCount: the next receive moves it to the dead-letter queue."),
        "{}",
        d.sentence
    );
    // The next receive does what it does when the timeout runs out: the message goes to the dead-letter queue.
    assert!(received(&mut w, &url, 1).is_empty());
    assert_eq!(received(&mut w, &dead, 1).len(), 1);
}

/// An authorizer that refuses one access key everything.
struct Refuses(&'static str);

impl Authorizer for Refuses {
    fn authorize<'a>(
        &'a self,
        caller: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            if caller.access_key_id == self.0 {
                let what = needs
                    .first()
                    .map(|n| format!("{} on {}", n.action, n.resource));
                return Err(ApiError::new(403, "AccessDenied", what.unwrap_or_default()));
            }
            Ok(())
        })
    }
}

fn service_with_authorizer() -> Sqs {
    let peers = Peers::new();
    peers.set_authorizer(Arc::new(Refuses("intruder")));
    peers.set_mode(Mode::Enforce);
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers;
    Sqs::new(Arc::new(Ctx {
        dir: Box::leak(Box::new(TempDir::new())).0.clone(),
        idle: Duration::from_secs(3600),
        tasks: Tasks::new(),
        clock: Arc::new(doze_runtime::SystemClock),
        ext: Some(ext),
    }))
}

fn caller(key: &str) -> Caller {
    Caller {
        access_key_id: key.to_owned(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

#[tokio::test]
async fn a_real_service_brings_a_message_back_and_a_caller_without_the_right_is_refused() {
    let sqs = service_with_authorizer();
    let patterns = Patterns::new(&TABLES);
    let call = |op, body: Value| call_in("us-east-1", &patterns, op, &body).unwrap();
    let json_of = |op: Operation, out: aws_wire::service::Output| output_json(op, &out);

    sqs.call(call(Operation::CreateQueue, json!({"QueueName": "orders"})))
        .await
        .unwrap();
    let url = "http://localhost:4566/000000000000/orders";
    sqs.call(call(
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "one"}),
    ))
    .await
    .unwrap();
    let rec = || {
        call(
            Operation::ReceiveMessage,
            json!({"QueueUrl": url, "AttributeNames": ["All"]}),
        )
    };
    let got = json_of(Operation::ReceiveMessage, sqs.call(rec()).await.unwrap());
    assert_eq!(got["Messages"][0]["Body"], "one");
    let hidden = json_of(Operation::ReceiveMessage, sqs.call(rec()).await.unwrap());
    assert_eq!(hidden, json!({}), "in flight for 30 s");

    let input = json!({"queue": "orders"});
    let forced = sqs.forced().unwrap();
    let refused = forced
        .force(&caller("intruder"), kind::EXPIRE_VISIBILITY, &input)
        .await
        .unwrap_err();
    assert_eq!(
        (refused.status, refused.code.as_ref()),
        (403, "AccessDenied")
    );
    assert!(
        refused
            .message
            .contains("sqs:ChangeMessageVisibility on arn:aws:sqs:us-east-1:000000000000:orders")
    );
    assert_eq!(
        json_of(Operation::ReceiveMessage, sqs.call(rec()).await.unwrap()),
        json!({}),
        "the refusal changed nothing"
    );

    let step = forced
        .force(&caller("console"), kind::EXPIRE_VISIBILITY, &input)
        .await
        .unwrap();
    assert!(
        done(step)
            .sentence
            .starts_with("Brought back 1 message that was in flight in orders")
    );
    let again = json_of(Operation::ReceiveMessage, sqs.call(rec()).await.unwrap());
    assert_eq!(again["Messages"][0]["Body"], "one");
    assert_eq!(
        again["Messages"][0]["Attributes"]["ApproximateReceiveCount"],
        "2"
    );
}

#[tokio::test]
async fn a_forced_event_the_service_does_not_have_and_a_missing_queue_are_refused_in_words() {
    let sqs = service_with_authorizer();
    let forced = sqs.forced().unwrap();
    let me = caller("console");
    let e = forced
        .force(&me, "sweep-ttl", &json!({"queue": "q"}))
        .await
        .unwrap_err();
    assert!(
        e.message.contains("SQS has no forced event sweep-ttl"),
        "{}",
        e.message
    );
    let e = forced
        .force(&me, kind::EXPIRE_VISIBILITY, &json!({}))
        .await
        .unwrap_err();
    assert_eq!(e.message, "The forced event needs a queue.");
    let e = forced
        .force(&me, kind::EXPIRE_VISIBILITY, &json!({"queue": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "QueueDoesNotExist");
}

#[tokio::test]
async fn a_long_poll_that_waits_is_woken_by_the_messages_that_come_back() {
    let sqs = Arc::new(service_with_authorizer());
    let patterns = Patterns::new(&TABLES);
    let call = |op, body: Value| call_in("us-east-1", &patterns, op, &body).unwrap();
    sqs.call(call(Operation::CreateQueue, json!({"QueueName": "q"})))
        .await
        .unwrap();
    let url = "http://localhost:4566/000000000000/q";
    sqs.call(call(
        Operation::SendMessage,
        json!({"QueueUrl": url, "MessageBody": "x"}),
    ))
    .await
    .unwrap();
    sqs.call(call(Operation::ReceiveMessage, json!({"QueueUrl": url})))
        .await
        .unwrap();
    let waiting = {
        let sqs = Arc::clone(&sqs);
        let poll = call(
            Operation::ReceiveMessage,
            json!({"QueueUrl": url, "WaitTimeSeconds": 20}),
        );
        tokio::spawn(async move { sqs.call(poll).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let step = sqs
        .forced()
        .unwrap()
        .force(
            &caller("console"),
            kind::EXPIRE_VISIBILITY,
            &json!({"queue": "q"}),
        )
        .await
        .unwrap();
    assert!(matches!(step, Step::Done(_)));
    let out = tokio::time::timeout(Duration::from_secs(3), waiting)
        .await
        .unwrap_or_else(|_| panic!("the poll was woken, not left to wait its 20 s"))
        .unwrap()
        .unwrap();
    assert_eq!(
        output_json(Operation::ReceiveMessage, &out)["Messages"][0]["Body"],
        "x"
    );
}
