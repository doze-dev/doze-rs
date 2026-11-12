//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::Value;

use crate::Kms;
use crate::core::Core;

mod crosscheck;
mod grants;
mod import;
mod keys;
mod persistence;
mod refusals;
mod rsa;

pub(crate) const START: i64 = 1_700_000_000_000;

pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    h: Harness<Kms>,
}

impl World {
    pub(crate) fn new() -> Self {
        let clock = Arc::new(ManualClock::at(START));
        let core = Core::in_memory(clock.clone()).unwrap();
        Self::with(core, clock)
    }

    pub(crate) fn with(core: Core, clock: Arc<ManualClock>) -> Self {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-kms-unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        Self {
            core,
            clock,
            h: Harness::new(Kms::new(Arc::new(ctx))),
        }
    }

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

    /// How many rows the call changed: it is run and staged, and its rows are counted, not written.
    pub(crate) fn rows_of(&mut self, op: &str, request: &Value) -> usize {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request).unwrap();
        self.core.dispatch(&call).unwrap();
        self.core.keys.stage().unwrap().rows() + self.core.aliases.stage().unwrap().rows()
    }

    /// A new key; its id.
    pub(crate) fn key(&mut self, request: &Value) -> String {
        self.ok("CreateKey", request)["KeyMetadata"]["KeyId"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[test]
fn what_is_not_built_is_refused_by_name() {
    let w = World::new();
    for op in ["CreateKey", "Encrypt", "Sign", "RotateKeyOnDemand"] {
        assert!(w.h.serves(op), "{op} is built");
    }
    for op in [
        "CreateCustomKeyStore",
        "DeriveSharedSecret",
        "ReplicateKey",
        "UpdatePrimaryRegion",
    ] {
        assert!(!w.h.serves(op), "{op} is refused");
    }
}

pub(crate) fn temp() -> TempDir {
    TempDir::new("kms")
}
