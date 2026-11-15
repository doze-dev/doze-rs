//! `doze-aws data` on SQS: a queue's size is its messages, emptying keeps the queue, an older-than prune keeps the newer messages,
//! and the file shrinks once it is done.

use aws_wire::storage::{Emptied, Prune, StorageError};

use doze_runtime::Clock;

use super::*;
use crate::persist::Persist;
use crate::storage::{DataAsk, DataReply};

fn on_disk(dir: &std::path::Path) -> World {
    let mut w = World::new();
    w.core = Core::restore(w.clock.clone(), Some(Persist::open(dir).unwrap()));
    w
}

fn fill(w: &mut World, url: &str, count: usize) {
    let body = "x".repeat(8 << 10);
    for _ in 0..count {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": body}),
        );
    }
}

fn emptied(reply: DataReply) -> Result<Emptied, StorageError> {
    match reply {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_queue_is_measured_emptied_and_its_space_given_back() {
    let dir = TempDir::new();
    let mut w = on_disk(&dir.0);
    let jobs = w.queue("jobs");
    let dlq = w.queue("jobs-dlq");
    w.ok(
        Operation::SetQueueAttributes,
        &json!({"QueueUrl": jobs, "Attributes": {"RedrivePolicy": json!({"deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:jobs-dlq", "maxReceiveCount": 3}).to_string()}}),
    );
    fill(&mut w, &jobs, 300);
    fill(&mut w, &dlq, 2);

    let DataReply::Usage(resources, file) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!()
    };
    let of = |name: &str| resources.iter().find(|r| r.name == name).unwrap().clone();
    assert_eq!(of("jobs").items, 300);
    assert!(of("jobs").bytes > 300 * (8 << 10), "{:?}", of("jobs"));
    assert!(of("jobs").notes.is_empty());
    assert_eq!(
        of("jobs-dlq").notes.len(),
        1,
        "a dead-letter queue that holds messages says so"
    );
    assert!(file.bytes >= of("jobs").bytes);

    // A dry run counts and removes nothing.
    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    let would = emptied(w.core.data(
        "us-east-1",
        DataAsk::Empty {
            name: "jobs".into(),
            prune: dry,
        },
    ))
    .unwrap();
    assert_eq!((would.items, would.freed), (300, 0));
    assert_eq!(w.core.queues["jobs"].messages.len(), 300);

    // Older than a time: the newer messages stay.
    w.clock.advance(60_000);
    let cut = w.clock.now_ms();
    fill(&mut w, &jobs, 5);
    let older = Prune {
        before_ms: Some(cut),
        ..Prune::default()
    };
    let done = emptied(w.core.data(
        "us-east-1",
        DataAsk::Empty {
            name: "jobs".into(),
            prune: older,
        },
    ))
    .unwrap();
    assert_eq!(done.items, 300);
    assert!(done.freed > 1 << 20, "the file shrank: {done:?}");
    assert_eq!(w.core.queues["jobs"].messages.len(), 5);

    // The queue is still there and works; what is left survives a restart.
    fill(&mut w, &jobs, 1);
    let old = std::mem::replace(&mut w.core, Core::new(w.clock.clone()));
    old.persist.unwrap().close();
    w.core = Core::restore(w.clock.clone(), Some(Persist::open(&dir.0).unwrap()));
    assert_eq!(w.core.queues["jobs"].messages.len(), 6);

    // Ways a queue cannot be pruned, and a queue that is not there.
    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        emptied(w.core.data(
            "us-east-1",
            DataAsk::Empty {
                name: "jobs".into(),
                prune: prefix
            }
        )),
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        emptied(w.core.data(
            "us-east-1",
            DataAsk::Empty {
                name: "nope".into(),
                prune: Prune::default()
            }
        )),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_reset_removes_every_queue_and_survives_a_restart() {
    let dir = TempDir::new();
    let mut w = on_disk(&dir.0);
    let jobs = w.queue("jobs");
    fill(&mut w, &jobs, 50);
    let done = emptied(w.core.data("us-east-1", DataAsk::Reset { dry_run: false })).unwrap();
    assert_eq!(done.items, 50);
    assert!(w.core.queues.is_empty());
    let old = std::mem::replace(&mut w.core, Core::new(w.clock.clone()));
    old.persist.unwrap().close();
    w.core = Core::restore(w.clock.clone(), Some(Persist::open(&dir.0).unwrap()));
    assert!(w.core.queues.is_empty());
    // The name is free at once: a reset is a fresh start, not a delete AWS would make you wait 60 seconds after.
    w.queue("jobs");
}
