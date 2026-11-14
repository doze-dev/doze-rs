//! A unit of several processes (Go's `Composite`, audit M1): when one exits by itself, the others are asked to stop
//! the way their plan says, in reverse order, not killed outright.

mod common;

use common::*;
use doze_engine::exec;
use doze_engine::logs::RunLog;
use doze_engine::supervise::{Ended, Handle};
use doze_module_proto::msg::run::{Plan, Process, Ready};
use std::collections::BTreeMap;
use std::time::Duration;

fn process(name: &str, args: &[&str], port: Option<u16>, after: &[&str]) -> Process {
    let mut env = BTreeMap::new();
    if let Some(p) = port {
        env.insert("PORT".to_string(), p.to_string());
    }
    Process {
        name: name.into(),
        program: HELPER.into(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
        env,
        dir: None,
        after: after.iter().map(|a| (*a).to_string()).collect(),
        ready: port.map(|p| Ready::Tcp(format!("127.0.0.1:{p}"))),
        hooks: vec![],
        stop: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn when_one_process_exits_the_others_are_stopped_gracefully() {
    let d = Dir::new("composite");
    let log = RunLog::open(&d.path().join("run"), "pair").unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .unwrap()
        .port();
    let plan = Plan {
        processes: vec![
            process("db", &["listen"], Some(port), &[]),
            process("proxy", &["exit", "3", "1000"], None, &["db"]),
        ],
    };
    let unit = exec::start(&plan, &log, Duration::from_secs(10), None)
        .await
        .unwrap();
    let pids = unit.pids();
    assert_eq!(pids.len(), 2);
    let handle = Handle::supervise(unit);
    let ended = tokio::time::timeout(Duration::from_secs(15), handle.ended())
        .await
        .unwrap();
    assert_eq!(
        ended,
        Ended::Exited {
            success: false,
            text: "exited with status 3".into()
        }
    );
    assert!(all_gone(&pids).await);
    let text = log.text();
    assert!(
        text.contains("stopping"),
        "db was asked to stop and said so, not killed: {text:?}"
    );
}
