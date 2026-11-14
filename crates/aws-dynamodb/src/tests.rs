//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use aws_wire::ApiError;
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Dynamodb;
use crate::core::{Core, Request};

mod expiry;
mod items;
mod partiql;
mod persistence;
mod queries;
mod refusals;
mod service;
mod tables;
mod txn;

pub(crate) struct World {
    pub(crate) core: Option<Core>,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) dir: TempDir,
    rt: tokio::runtime::Runtime,
    h: Harness<Dynamodb>,
}

impl World {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new("ddb");
        let clock = Arc::new(ManualClock::at(1_700_000_000_000));
        let ctx = Arc::new(Ctx {
            dir: dir.path().join("unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        });
        let core = Core::open_in(dir.path(), clock.clone(), None).unwrap();
        Self {
            core: Some(core),
            clock,
            dir,
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            h: Harness::new(Dynamodb::new(ctx)),
        }
    }

    /// Runs a request as the gateway would: decoded against the model, then handled.
    pub(crate) fn call(&mut self, op: &str, req: Value) -> Result<Value, ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, &req)?;
        let core = self.core.as_mut().unwrap();
        let answer = self.rt.block_on(doze_runtime::Service::handle(
            core,
            Request::Call(Box::new(call)),
        ));
        let out = answer.reply?;
        Ok(self.h.json(ix, &out))
    }

    pub(crate) fn ok(&mut self, op: &str, req: Value) -> Value {
        self.call(op, req)
            .unwrap_or_else(|e| panic!("{op} failed: {} {}", e.code, e.message))
    }

    pub(crate) fn err(&mut self, op: &str, req: Value) -> ApiError {
        match self.call(op, req) {
            Ok(v) => panic!("{op} should have failed, answered {v}"),
            Err(e) => e,
        }
    }

    /// Stops the service and starts it again on the same files.
    pub(crate) fn restart(&mut self) {
        let core = self.core.take().unwrap();
        self.rt.block_on(doze_runtime::Service::close(core));
        self.core = Some(Core::open_in(self.dir.path(), self.clock.clone(), None).unwrap());
    }

    /// A table with a string partition key `pk` (and a number sort key `sk` when asked).
    pub(crate) fn table(&mut self, name: &str, sort: bool) {
        let mut schema = vec![json!({"AttributeName": "pk", "KeyType": "HASH"})];
        let mut attrs = vec![json!({"AttributeName": "pk", "AttributeType": "S"})];
        if sort {
            schema.push(json!({"AttributeName": "sk", "KeyType": "RANGE"}));
            attrs.push(json!({"AttributeName": "sk", "AttributeType": "N"}));
        }
        self.ok(
            "CreateTable",
            json!({"TableName": name, "KeySchema": schema, "AttributeDefinitions": attrs, "BillingMode": "PAY_PER_REQUEST"}),
        );
    }

    pub(crate) fn put(&mut self, table: &str, item: Value) {
        self.ok("PutItem", json!({"TableName": table, "Item": item}));
    }
}

/// Shorthand for the message of an error.
pub(crate) fn message(e: &ApiError) -> &str {
    &e.message
}
