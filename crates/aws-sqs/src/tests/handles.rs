//! Receipt handles: a delete or a change of visibility finds its message by the handle (it carries the message's place), an
//! earlier receive's handle deletes but does not re-time (recorded, `test_an_old_receipt_handle`), and a message keeps only its
//! latest few.

use super::*;
use crate::handles::KEPT_HANDLES;

fn receive(w: &mut World, url: &str, visibility: u32) -> String {
    let got = w.ok(
        Operation::ReceiveMessage,
        &json!({"QueueUrl": url, "VisibilityTimeout": visibility}),
    );
    got["Messages"][0]["ReceiptHandle"]
        .as_str()
        .expect("a message")
        .to_owned()
}

fn count(w: &mut World, url: &str) -> usize {
    w.core.queues[url.rsplit('/').next().unwrap()]
        .messages
        .len()
}

#[test]
fn an_earlier_receives_handle_deletes_the_message_but_does_not_change_its_visibility() {
    let mut w = World::new();
    let url = w.queue("q");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "x"}),
    );
    let old = receive(&mut w, &url, 0);
    let new = receive(&mut w, &url, 30);
    assert_ne!(old, new);
    let (status, code, _) = w.err(
        Operation::ChangeMessageVisibility,
        &json!({"QueueUrl": url, "ReceiptHandle": old, "VisibilityTimeout": 1}),
    );
    assert_eq!((status, code.as_str()), (400, "InvalidParameterValue"));
    w.ok(
        Operation::ChangeMessageVisibility,
        &json!({"QueueUrl": url, "ReceiptHandle": new, "VisibilityTimeout": 31}),
    );
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": url, "ReceiptHandle": old}),
    );
    assert_eq!(count(&mut w, &url), 0, "AWS deleted it (recorded)");
}

#[test]
fn a_handle_names_its_own_message_only() {
    let mut w = World::new();
    let (one, two) = (w.queue("one"), w.queue("two"));
    for url in [&one, &two] {
        w.ok(
            Operation::SendMessage,
            &json!({"QueueUrl": url, "MessageBody": "x"}),
        );
    }
    let first = receive(&mut w, &one, 30);
    // The same place in another queue, and a made-up handle with this message's place: neither deletes.
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": two, "ReceiptHandle": first}),
    );
    let forged = format!("{}{}", &first[..16], "0".repeat(32));
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": one, "ReceiptHandle": forged}),
    );
    assert_eq!((count(&mut w, &one), count(&mut w, &two)), (1, 1));
    // A handle from before handles carried a place (any 48 hex digits) is still looked for.
    w.core
        .queues
        .get_mut("one")
        .unwrap()
        .messages
        .update(0, |m| m.handles = vec!["ab".repeat(24)]);
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": one, "ReceiptHandle": "ab".repeat(24)}),
    );
    assert_eq!(count(&mut w, &one), 0);
}

#[test]
fn a_message_received_again_and_again_keeps_only_its_latest_handles() {
    let mut w = World::new();
    let url = w.queue("q");
    w.ok(
        Operation::SendMessage,
        &json!({"QueueUrl": url, "MessageBody": "x"}),
    );
    let mut handles: Vec<String> = (0..KEPT_HANDLES + 5)
        .map(|_| receive(&mut w, &url, 0))
        .collect();
    let m = w.core.queues["q"].messages.first().unwrap();
    assert_eq!(m.handles.len(), KEPT_HANDLES);
    assert_eq!(m.handles.last(), handles.last());
    // The oldest is forgotten: its delete succeeds and leaves the message (AWS: "might not be deleted").
    let oldest = handles.remove(0);
    w.ok(
        Operation::DeleteMessage,
        &json!({"QueueUrl": url, "ReceiptHandle": oldest}),
    );
    assert_eq!(count(&mut w, &url), 1);
}
