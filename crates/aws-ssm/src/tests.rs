//! The state machine without a socket: requests are decoded against the model and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_peers::Peers;
use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Ssm;
use crate::core::Core;

mod parameters;
mod peers;
mod persistence;
mod refusals;

/// The services SSM calls, running for real beside it: KMS (and Secrets Manager, for a reference), on a runtime the test drives.
pub(crate) struct Bus {
    pub(crate) peers: Arc<Peers>,
    pub(crate) rt: tokio::runtime::Runtime,
    _dir: TempDir,
}

impl Bus {
    pub(crate) fn new() -> Self {
        let dir = temp();
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let ctx = |name: &str| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: std::time::Duration::from_secs(300),
                tasks: Tasks::new(),
                clock: Arc::new(SystemClock),
                ext: Some(Arc::clone(&ext)),
            })
        };
        peers.register(Arc::new(aws_kms::Kms::new(ctx("kms"))));
        peers.register(Arc::new(aws_secrets::Secrets::new(ctx("secretsmanager"))));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        Self {
            peers,
            rt,
            _dir: dir,
        }
    }
}

pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) bus: Bus,
    h: Harness<Ssm>,
}

impl World {
    pub(crate) fn new() -> Self {
        let clock = Arc::new(ManualClock::at(1_700_000_000_000));
        let bus = Bus::new();
        let core = Core::in_memory(clock.clone(), Some(bus.peers.clone()));
        Self::with(core, clock, bus)
    }

    pub(crate) fn with(core: Core, clock: Arc<ManualClock>, bus: Bus) -> Self {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-ssm-unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        Self {
            core,
            clock,
            bus,
            h: Harness::new(Ssm::new(Arc::new(ctx))),
        }
    }

    /// Runs one operation: the request as JSON in, the answer as JSON (or the error) out.
    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request)?;
        let out = self.bus.rt.block_on(self.core.dispatch(&call));
        // What the call changed is on disk (if there is a disk) before its answer is looked at.
        if let Some(saved) = self.core.flush().unwrap() {
            saved.wait().unwrap();
        }
        Ok(self.h.json(ix, &out?))
    }

    /// How many rows the call changed: the call is run and staged, and its rows are counted, not written.
    pub(crate) fn rows_of(&mut self, op: &str, request: &Value) -> usize {
        let ix = self.h.op(op);
        let call = self.h.call(ix, request).unwrap();
        self.bus.rt.block_on(self.core.dispatch(&call)).unwrap();
        self.core.params.stage().unwrap().rows()
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

    pub(crate) fn put(&mut self, name: &str, value: &str) -> Value {
        self.ok(
            "PutParameter",
            &json!({"Name": name, "Value": value, "Type": "String", "Overwrite": true}),
        )
    }
}

#[test]
fn the_fleet_half_of_ssm_is_refused_by_name() {
    let w = World::new();
    for op in ["PutParameter", "GetParametersByPath", "ListTagsForResource"] {
        assert!(w.h.serves(op), "{op} is built");
    }
    for op in [
        "SendCommand",
        "StartSession",
        "CreateMaintenanceWindow",
        "CreateDocument",
        "PutInventory",
    ] {
        assert!(!w.h.serves(op), "{op} is refused");
    }
}

pub(crate) fn temp() -> TempDir {
    TempDir::new("ssm")
}
