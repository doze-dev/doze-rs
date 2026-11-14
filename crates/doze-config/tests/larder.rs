//! doze-examples/larder's doze.hcl (the oracle corpus holds it as `875f9a5b61.hcl`) loaded into a
//! running engine under a temporary home, and woken.
//!
//! Larder's apps run `go run`, `uv` and `bun`, and its tasks wait for engine phase E2; so a probe
//! app is added beside it (a sibling `*.doze.hcl`, merged as doze merges them) that needs larder's
//! Redis. Waking the probe wakes Redis first, through a module stand-in's plan, and the probe's
//! environment shows what the config wired: Redis's address and doze's CA variables.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use doze_config::model::Instance;
use doze_config::stack::{Wiring, ca_env, to_stack};
use doze_config::{Options, load};
use doze_engine::stack::{BoxFuture, Endpoint, PlanCtx, Planned, Planner};
use doze_engine::{Cause, Engine};
use doze_module_proto::msg::run::{Plan, Process};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::{Fixtures, OracleHost};

const PROBE: &str = r#"app "probe" {
  run = "env > \"$DATA/env.txt\"; exec sleep 600"
  env = {
    REDIS = module.redis.address
  }
}
"#;

/// A module stand-in: one long sleep, reached at the block's port.
#[derive(Debug)]
struct StandIn {
    port: u16,
}

impl Planner for StandIn {
    fn plan<'a>(&'a self, _ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>> {
        let port = self.port;
        Box::pin(async move {
            let p = Process {
                name: "main".into(),
                program: "sleep".into(),
                args: vec!["600".into()],
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                ready: None,
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
}

fn larder_source() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/oracle/files/875f9a5b61.hcl");
    std::fs::read_to_string(p).unwrap()
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn eventually(limit: Duration, f: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn larder_runs_in_the_engine() {
    let root =
        Temp(std::env::temp_dir().join(format!("doze-config-larder-{}", std::process::id())));
    let (project, home) = (root.0.join("larder"), root.0.join("home"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(project.join("doze.hcl"), larder_source()).unwrap();
    std::fs::write(project.join("probe.doze.hcl"), PROBE).unwrap();

    let host = OracleHost {
        home: home.clone(),
        cwd: project.clone(),
        zone: None,
    };
    let vars = BTreeMap::from([("names".to_owned(), "false".to_owned())]);
    let opts = Options {
        host: &host,
        modules: &Fixtures,
        vars,
        version: "dev".into(),
    };
    let cfg = load("doze.hcl", &opts).unwrap();
    assert_eq!(cfg.stack(), "larder");
    assert!(cfg.data_dir.starts_with(home.to_string_lossy().as_ref()));

    let planner = |i: &Instance| -> Arc<dyn Planner> { Arc::new(StandIn { port: i.port }) };
    let tls = home.join(".doze/tls");
    let w = Wiring {
        planner: &planner,
        extra_env: ca_env(&tls.join("ca.pem"), &tls.join("bundle.pem")),
    };
    let stack = to_stack(&cfg, &w);
    let tasks = cfg.tasks.len();
    assert_eq!(
        stack.services.len(),
        cfg.instances.len() - tasks,
        "every block but the tasks"
    );
    let gateway = stack.service("gateway").unwrap();
    assert!(
        gateway.needs.iter().any(|n| n.name == "redis"),
        "{:?}",
        gateway.needs
    );

    let e = Engine::new(stack).unwrap();
    let ep = e.wake("probe", Cause::Command).await.unwrap();
    assert!(
        ep.backend.is_empty(),
        "a probe with no port has no address: {ep:?}"
    );
    assert!(
        e.get("redis").unwrap().state.running(),
        "its need was woken first"
    );
    let env_txt = PathBuf::from(&cfg.data_dir).join("clusters/probe/env.txt");
    assert!(
        eventually(Duration::from_secs(10), || std::fs::read_to_string(
            &env_txt
        )
        .is_ok_and(|t| t.contains("REDIS=")))
        .await
    );
    let env = std::fs::read_to_string(&env_txt).unwrap();
    assert!(env.lines().any(|l| l == "REDIS=127.0.0.1:6379"), "{env}");
    let ca = format!("DOZE_CA_FILE={}", tls.join("ca.pem").display());
    assert!(env.lines().any(|l| l == ca), "{env}");
    assert!(
        e.get("orders").is_some_and(|i| !i.state.running()),
        "nothing else was woken"
    );
    e.shutdown().await;
}
