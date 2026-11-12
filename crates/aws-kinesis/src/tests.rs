//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Kinesis;
use crate::core::Core;

mod persistence;
mod refusals;
mod reshard;
mod streams;

pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    h: Harness<Kinesis>,
}

impl World {
    pub(crate) fn new() -> Self {
        let clock = Arc::new(ManualClock::at(1_700_000_000_000));
        Self::with(Core::in_memory(clock.clone()), clock)
    }

    pub(crate) fn with(core: Core, clock: Arc<ManualClock>) -> Self {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-kinesis-unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        Self {
            core,
            clock,
            h: Harness::new(Kinesis::new(Arc::new(ctx))),
        }
    }

    /// Runs one operation: the request as JSON in, the answer as JSON (or the error) out.
    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        let out = self.core.dispatch(&call);
        if let Some(saved) = self.core.flush().unwrap() {
            saved.wait().unwrap();
        }
        Ok(self.h.json(ix, &out?))
    }

    pub(crate) fn ok(&mut self, op: &str, request: &Value) -> Value {
        self.run(op, request)
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message))
    }

    /// The error's code (its shape's name when it has one), and its message.
    pub(crate) fn err(&mut self, op: &str, request: &Value) -> (String, String) {
        let e = self.run(op, request).unwrap_err();
        (
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    }

    pub(crate) fn stream(&mut self, name: &str, shards: u32) {
        self.ok(
            "CreateStream",
            &json!({"StreamName": name, "ShardCount": shards}),
        );
    }

    pub(crate) fn put(&mut self, name: &str, key: &str, data: &str) -> Value {
        self.ok(
            "PutRecord",
            &json!({"StreamName": name, "PartitionKey": key, "Data": aws_core::base64::encode(data.as_bytes())}),
        )
    }

    pub(crate) fn iterator(&mut self, name: &str, shard: &str, kind: &str) -> String {
        self.ok(
            "GetShardIterator",
            &json!({"StreamName": name, "ShardId": shard, "ShardIteratorType": kind}),
        )["ShardIterator"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[test]
fn streaming_is_refused_by_name_and_resharding_is_built() {
    let w = World::new();
    for op in [
        "PutRecord",
        "GetRecords",
        "RegisterStreamConsumer",
        "ListShards",
        "SplitShard",
        "MergeShards",
        "UpdateShardCount",
        "DescribeAccountSettings",
        "UpdateAccountSettings",
    ] {
        assert!(w.h.serves(op), "{op} is built");
    }
    for op in ["SubscribeToShard", "CreateChannel"] {
        assert!(!w.h.serves(op), "{op} is refused");
    }
}

pub(crate) fn temp() -> TempDir {
    TempDir::new("kinesis")
}
