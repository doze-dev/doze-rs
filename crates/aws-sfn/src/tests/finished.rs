//! A finished execution leaves memory once its record is staged: a describe, a list and its history read it back (while its commit is
//! still queued too), a redrive brings it back, and the executions of one machine share its definition's text.

use std::sync::Arc;
use std::time::Duration;

use aws_testkit::{Harness, TempDir};
use doze_runtime::{Ctx, Service, SystemClock, Tasks};
use serde_json::{Value, json};

use crate::Sfn;
use crate::core::{Asked, Core, Request};

struct Bench {
    h: Harness<Sfn>,
    core: Core,
    dir: TempDir,
}

impl Bench {
    fn new() -> Self {
        let dir = TempDir::new("sfn-finished");
        let core = Core::open_in(dir.path(), Arc::new(SystemClock), None).unwrap();
        let ctx = Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        });
        Self {
            h: Harness::new(Sfn::new(ctx)),
            core,
            dir,
        }
    }

    /// Runs a call; the commit is not waited for (as the actor does not).
    async fn ok(&mut self, op: &str, body: Value) -> Value {
        let ix = self.h.op(op);
        let call = self.h.call(ix, &body).unwrap();
        let asked = Asked {
            config: Arc::default(),
            req: Request::Call(call),
        };
        let answer = Service::handle(&mut self.core, asked).await;
        let out = answer
            .reply
            .unwrap_or_else(|e| panic!("{op}: {} {}", e.code, e.message));
        self.h.json(ix, &out)
    }

    async fn restart(&mut self) {
        let core = std::mem::replace(
            &mut self.core,
            Core::open_in(&self.dir.path().join("other"), Arc::new(SystemClock), None).unwrap(),
        );
        Service::close(core).await;
        let spare = std::mem::replace(
            &mut self.core,
            Core::open_in(self.dir.path(), Arc::new(SystemClock), None).unwrap(),
        );
        Service::close(spare).await;
    }
}

const FAILS: &str =
    r#"{"StartAt":"P","States":{"P":{"Type":"Pass","Next":"F"},"F":{"Type":"Fail","Error":"E"}}}"#;

#[tokio::test(flavor = "current_thread")]
async fn a_finished_execution_is_read_from_disk_listed_redriven_and_shares_its_definition() {
    let mut b = Bench::new();
    let m = b
        .ok("CreateStateMachine", json!({"name": "m", "definition": FAILS, "roleArn": "arn:aws:iam::000000000000:role/r"}))
        .await["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut arns = Vec::new();
    for i in 0..3 {
        let started = b
            .ok(
                "StartExecution",
                json!({"stateMachineArn": m, "name": format!("e{i}"), "input": "{\"n\":1}"}),
            )
            .await;
        arns.push(started["executionArn"].as_str().unwrap().to_owned());
    }
    // A Pass then a Fail: finished within the call, and let go of at its end.
    for arn in &arns {
        assert!(
            b.core.executions.get(arn).is_none(),
            "a finished execution is not held"
        );
        let d = b
            .ok("DescribeExecution", json!({"executionArn": arn}))
            .await;
        assert_eq!(d["status"], "FAILED");
        assert_eq!(d["input"], "{\"n\":1}");
    }
    let listed = b.ok("ListExecutions", json!({"stateMachineArn": m})).await;
    assert_eq!(listed["executions"].as_array().unwrap().len(), 3);
    assert_eq!(listed["executions"][0]["status"], "FAILED");
    let history = b
        .ok("GetExecutionHistory", json!({"executionArn": arns[0]}))
        .await;
    assert!(history["events"].as_array().unwrap().len() >= 3);

    // A redrive reads it back and runs it again (it fails again, and is let go of again).
    b.ok("RedriveExecution", json!({"executionArn": arns[1]}))
        .await;
    let d = b
        .ok("DescribeExecution", json!({"executionArn": arns[1]}))
        .await;
    assert_eq!(d["redriveCount"], 1);

    // Across a restart only the summaries are read into memory.
    b.restart().await;
    assert!(b.core.executions.is_empty());
    let listed = b
        .ok(
            "ListExecutions",
            json!({"stateMachineArn": m, "redriveFilter": "REDRIVEN"}),
        )
        .await;
    assert_eq!(listed["executions"].as_array().unwrap().len(), 1);
    let d = b
        .ok("DescribeExecution", json!({"executionArn": arns[2]}))
        .await;
    assert_eq!(d["status"], "FAILED");
    let again = b
        .call_err(
            "StartExecution",
            json!({"stateMachineArn": m, "name": "e0", "input": "{}"}),
        )
        .await;
    assert_eq!(again, "ExecutionAlreadyExists");
}

#[tokio::test(flavor = "current_thread")]
async fn running_executions_of_one_machine_share_one_copy_of_its_definition() {
    let mut b = Bench::new();
    let waits = r#"{"StartAt":"W","States":{"W":{"Type":"Wait","Seconds":600,"End":true}}}"#;
    let m = b
        .ok("CreateStateMachine", json!({"name": "w", "definition": waits, "roleArn": "arn:aws:iam::000000000000:role/r"}))
        .await["stateMachineArn"]
        .as_str()
        .unwrap()
        .to_owned();
    for i in 0..3 {
        b.ok(
            "StartExecution",
            json!({"stateMachineArn": m, "name": format!("w{i}")}),
        )
        .await;
    }
    let texts: Vec<_> = b
        .core
        .executions
        .iter()
        .map(|(_, e)| e.definition.0.clone())
        .collect();
    assert_eq!(texts.len(), 3);
    assert!(texts.iter().all(|t| Arc::ptr_eq(t, &texts[0])), "one copy");
    b.restart().await;
    let texts: Vec<_> = b
        .core
        .executions
        .iter()
        .map(|(_, e)| e.definition.0.clone())
        .collect();
    assert_eq!(texts.len(), 3);
    assert!(
        texts.iter().all(|t| Arc::ptr_eq(t, &texts[0])),
        "one copy after a restart too"
    );
}

impl Bench {
    async fn call_err(&mut self, op: &str, body: Value) -> String {
        let ix = self.h.op(op);
        let call = self.h.call(ix, &body).unwrap();
        let asked = Asked {
            config: Arc::default(),
            req: Request::Call(call),
        };
        match Service::handle(&mut self.core, asked).await.reply {
            Ok(_) => panic!("{op} should have failed"),
            Err(e) => e.code.into_owned(),
        }
    }
}
