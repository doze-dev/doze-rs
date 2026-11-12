//! IAM and STS without a socket: requests are decoded against the models and dispatched on a core with a manual clock.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::Value;

use crate::core::{Core, Which};
use crate::{Iam, Sts, services};

mod entities;
mod persistence;
mod policies;
mod sessions;

pub(crate) const START: i64 = 1_700_000_000_000;

pub(crate) struct World {
    pub(crate) core: Core,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) iam: Harness<Iam>,
    pub(crate) sts: Harness<Sts>,
}

impl World {
    pub(crate) fn new() -> Self {
        let clock = Arc::new(ManualClock::at(START));
        Self::with(Core::in_memory(clock.clone()), clock)
    }

    pub(crate) fn with(core: Core, clock: Arc<ManualClock>) -> Self {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-iam-unused"),
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        let (iam, sts) = services(Arc::new(ctx));
        Self {
            core,
            clock,
            iam: Harness::new(iam),
            sts: Harness::new(sts),
        }
    }

    fn go(&mut self, which: Which, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let (ix, call) = match which {
            Which::Iam => {
                let ix = self.iam.op(op);
                (ix, self.iam.call(ix, request)?)
            }
            Which::Sts => {
                let ix = self.sts.op(op);
                (ix, self.sts.call(ix, request)?)
            }
        };
        let out = self.core.dispatch(which, &call);
        if let Some(saved) = self.core.flush().unwrap() {
            saved.wait().unwrap();
        }
        let out = out?;
        Ok(match which {
            Which::Iam => self.iam.json(ix, &out),
            Which::Sts => self.sts.json(ix, &out),
        })
    }

    pub(crate) fn run(&mut self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        self.go(Which::Iam, op, request)
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

    pub(crate) fn sts_ok(&mut self, op: &str, request: &Value) -> Value {
        self.go(Which::Sts, op, request)
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message))
    }

    pub(crate) fn sts_err(&mut self, op: &str, request: &Value) -> (String, String) {
        let e = self.go(Which::Sts, op, request).unwrap_err();
        (
            e.shape
                .map_or_else(|| e.code.to_string(), |s| s.to_string()),
            e.message,
        )
    }

    /// How many rows the call changed: it is run and staged, and its rows are counted, not written.
    pub(crate) fn rows_of(&mut self, op: &str, request: &Value) -> usize {
        let ix = self.iam.op(op);
        let call = self.iam.call(ix, request).unwrap();
        self.core.dispatch(Which::Iam, &call).unwrap();
        let c = &mut self.core;
        c.entities.stage().unwrap().rows()
            + c.policies.stage().unwrap().rows()
            + c.keys.stage().unwrap().rows()
            + c.profiles.stage().unwrap().rows()
    }
}

pub(crate) const TRUST: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
pub(crate) const READ_S3: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#;

pub(crate) fn temp() -> TempDir {
    TempDir::new("iam")
}

#[test]
fn every_operation_the_list_names_is_run_and_the_rest_are_refused() {
    let w = World::new();
    for op in crate::ops::BUILT {
        let name = format!("{op:?}");
        assert!(
            w.iam.serves(&name),
            "{name} is on the list but the service refuses it"
        );
    }
    for name in [
        "UploadServerCertificate",
        "EnableMFADevice",
        "GenerateCredentialReport",
        "SimulatePrincipalPolicy",
    ] {
        assert!(!w.iam.serves(name), "{name} is refused");
    }
}
