//! A queue another Region of the stack has: the error a client of the wrong Region gets keeps its code and status and says where the
//! queue is.

use std::time::Duration;

use aws_wire::service::Service;
use doze_runtime::{Ctx, Tasks};

use super::*;
use crate::Sqs;

fn service() -> Sqs {
    let ctx = Ctx {
        dir: Box::leak(Box::new(TempDir::new())).0.clone(),
        idle: Duration::from_secs(3600),
        tasks: Tasks::new(),
        clock: Arc::new(doze_runtime::SystemClock),
        ext: None,
    };
    Sqs::new(Arc::new(ctx))
}

async fn call(sqs: &Sqs, region: &str, op: Operation, request: Value) -> Result<(), ApiError> {
    let patterns = Patterns::new(&TABLES);
    let call = call_in(region, &patterns, op, &request).unwrap();
    sqs.call(call).await.map(|_| ())
}

#[tokio::test]
async fn a_queue_of_another_region_is_named_in_the_error_and_nothing_else_changes() {
    let sqs = service();
    call(
        &sqs,
        "eu-west-2",
        Operation::CreateQueue,
        json!({"QueueName": "pick-lists"}),
    )
    .await
    .unwrap();
    // The Region that has it finds it, by name and by URL.
    let by_name = json!({"QueueName": "pick-lists"});
    call(&sqs, "eu-west-2", Operation::GetQueueUrl, by_name.clone())
        .await
        .unwrap();
    let url = json!({"QueueUrl": "http://localhost:4566/000000000000/pick-lists"});
    call(
        &sqs,
        "eu-west-2",
        Operation::GetQueueAttributes,
        url.clone(),
    )
    .await
    .unwrap();
    // Another Region does not, and is told where it is, however it asked.
    for (op, request) in [
        (Operation::GetQueueUrl, by_name),
        (Operation::GetQueueAttributes, url.clone()),
        (
            Operation::SendMessage,
            json!({"QueueUrl": "http://localhost:4566/000000000000/pick-lists", "MessageBody": "x"}),
        ),
    ] {
        let e = call(&sqs, "us-east-1", op, request).await.unwrap_err();
        assert_eq!(
            (e.code.as_ref(), e.status),
            ("QueueDoesNotExist", 400),
            "{op:?}"
        );
        assert!(
            e.message.starts_with("The specified queue does not exist.")
                && e.message.contains(
                    "\"pick-lists\" exists in eu-west-2; this client asked for us-east-1"
                )
                && e.message
                    .contains("set `region` on the aws block or the client's region"),
            "{op:?}: {}",
            e.message
        );
    }
    // A queue no Region has gets the plain answer.
    let e = call(
        &sqs,
        "us-east-1",
        Operation::GetQueueUrl,
        json!({"QueueName": "nobody"}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.message, "The specified queue does not exist.");
}
