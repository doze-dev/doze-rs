//! The health prober's records (W11, audit M7) and a `Lazy` need's wiring before it ever ran (W3, audit M8).

mod common;

use common::*;
use doze_engine::stack::BoxFuture;
use doze_engine::{
    Cause, Endpoint, Engine, PlanCtx, Planned, Planner, Probe, ProcessSpec, Service, When,
};
use doze_module_proto::msg::run::{Plan, Process};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Go's `CheckHealth`: a repeatable check decides; with none, or a one-shot `log` check, a running command is healthy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_prober_records_each_running_commands_health() {
    let d = Dir::new("health");
    let flag = d.file("healthy.flag");
    std::fs::write(&flag, "").unwrap();
    let check = run(&["check", &flag.to_string_lossy()]);
    let e = Engine::builder("t", d.path())
        .service(svc("tcp", listener(&d, 0)))
        .service(svc(
            "log",
            listener(&d, 0).ready(Probe::Log("listening".into())),
        ))
        .service(svc("exec", listener(&d, 0).ready(Probe::Exec(check))))
        .build()
        .unwrap();
    for n in ["tcp", "log", "exec"] {
        e.wake(n, Cause::Command).await.unwrap();
    }
    e.probe_health().await;
    let healthy = |n: &str| e.get(n).and_then(|i| i.healthy);
    assert_eq!(
        (healthy("tcp"), healthy("log"), healthy("exec")),
        (Some(true), Some(true), Some(true))
    );
    std::fs::remove_file(&flag).unwrap();
    e.probe_health().await;
    assert_eq!(healthy("exec"), Some(false), "a failing check is recorded");
    assert!(
        e.get("exec").unwrap().state.running(),
        "and never restarts it"
    );
    let pids: Vec<u32> = ["tcp", "log", "exec"]
        .iter()
        .map(|n| pid_of(&e, n))
        .collect();
    e.shutdown().await;
    assert!(all_gone(&pids).await);
}

/// A module stand-in that says where it will be reached, and remembers what it was told about its needs.
#[derive(Debug, Default)]
struct Recorder {
    seen: Mutex<Vec<BTreeMap<String, Endpoint>>>,
    socket: bool,
}

impl Planner for Recorder {
    fn plan<'a>(&'a self, ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>> {
        if let Ok(mut s) = self.seen.lock() {
            s.push(ctx.needs.clone());
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .map_err(|e| e.to_string());
        Box::pin(async move {
            let port = port?;
            let mut env = BTreeMap::new();
            env.insert("PORT".to_string(), port.to_string());
            let p = Process {
                name: "main".into(),
                program: HELPER.into(),
                args: vec!["listen".into()],
                env,
                dir: None,
                after: vec![],
                ready: Some(doze_module_proto::msg::run::Ready::Tcp(format!(
                    "127.0.0.1:{port}"
                ))),
                hooks: vec![],
                stop: None,
            };
            Ok(Planned {
                plan: Plan { processes: vec![p] },
                endpoint: Endpoint {
                    backend: format!("127.0.0.1:{port}"),
                },
                restart: None,
                supervised: false,
                ready_within: Duration::from_secs(10),
            })
        })
    }

    fn endpoint(&self, ctx: &PlanCtx) -> Option<Endpoint> {
        self.socket.then(|| Endpoint {
            backend: format!("unix:{}", ctx.run_dir.join("s.sock").display()),
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lazy_need_is_wired_with_its_declared_address_and_not_woken() {
    let d = Dir::new("lazy");
    let web = Arc::new(Recorder::default());
    let db = Arc::new(Recorder {
        socket: true,
        ..Recorder::default()
    });
    let cache = ProcessSpec::new(run(&["listen"]), d.path()).port(6390);
    let e = Engine::builder("t", d.path())
        .service(
            Service::planned("web", web.clone())
                .needs_when("db", When::Lazy)
                .needs_when("cache", When::Lazy),
        )
        .service(Service::planned("db", db))
        .service(svc("cache", cache))
        .build()
        .unwrap();
    e.wake("web", Cause::Command).await.unwrap();
    let seen = web
        .seen
        .lock()
        .unwrap()
        .first()
        .cloned()
        .unwrap_or_default();
    let sock = d.path().join("run").join("db").join("s.sock");
    assert_eq!(
        seen.get("db").map(|ep| ep.backend.clone()),
        Some(format!("unix:{}", sock.display())),
        "a module says its address before it runs"
    );
    assert_eq!(
        seen.get("cache").map(|ep| ep.backend.clone()),
        Some("127.0.0.1:6390".to_string()),
        "a command's own port"
    );
    for n in ["db", "cache"] {
        assert!(!e.get(n).unwrap().state.running(), "{n} was not woken");
    }
    let pid = pid_of(&e, "web");
    e.shutdown().await;
    assert!(all_gone(&[pid]).await);
}
