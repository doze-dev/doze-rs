//! `doze-aws data` on SNS: no messages are kept, so emptying is refused; a reset removes every topic and subscription, the file
//! shrinks, and a restart finds nothing.

use std::sync::Arc;

use doze_runtime::ManualClock;
use serde_json::json;

use super::{START, World, temp};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

#[test]
fn a_reset_removes_every_topic_and_the_file_shrinks() {
    let dir = temp();
    let base = World::new();
    let clock = Arc::new(ManualClock::at(START));
    let core = Core::open_in(dir.path(), clock.clone(), Some(base.peers.clone())).unwrap();
    let mut w = World::with(core, clock, base.peers.clone(), temp());
    let policy = "p".repeat(2000);
    for n in 0..300 {
        w.ok(
            "CreateTopic",
            &json!({"Name": format!("t{n}"), "Tags": [{"Key": "pad", "Value": policy[..250]}]}),
        );
    }
    let DataReply::Usage(file) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!()
    };
    assert!(file.bytes > 300 * 250, "{file:?}");
    let DataReply::Emptied(Ok(done)) = w.core.data("us-east-1", DataAsk::Reset { dry_run: false })
    else {
        panic!()
    };
    assert_eq!(done.items, 300);
    assert!(done.freed > 0, "the file shrank: {done:?}");
    w.core.db.take().unwrap().close();
    w.core = Core::open_in(dir.path(), w.clock.clone(), Some(w.peers.clone())).unwrap();
    assert!(
        w.ok("ListTopics", &json!({}))["Topics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    w.ok("CreateTopic", &json!({"Name": "t0"}));
}
