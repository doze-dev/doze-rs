//! `sync` and `destroy` through an instance's structure (Go's `Apply`, `reconcileObjects`, `Destroy`): with the instance
//! awake, converge, then prune what was applied before and is no longer declared, in reverse order; the state file
//! records what is applied; an instance with no structure, or off, is skipped.

mod common;

use common::*;
use doze_engine::applied::{Applied, Change};
use doze_engine::stack::{BoxFuture, Endpoint, PlanCtx, Planned, Planner, Structure};
use doze_engine::{Cause, Engine, Service, State};
use doze_module_proto::msg::instance::Object;
use doze_module_proto::msg::run::{Plan, Process, Ready};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A module stand-in: plans the helper's listener, declares `desired`, and records every structure call.
#[derive(Debug, Default)]
struct Module {
    desired: Mutex<Vec<Object>>,
    calls: Mutex<Vec<String>>,
    /// What `fingerprint` says.
    fp: Mutex<String>,
    /// `converge` fails with this.
    broken: Mutex<Option<String>>,
}

fn obj(name: &str) -> Object {
    Object {
        kind: "database".into(),
        name: name.into(),
        hash: format!("h-{name}"),
    }
}

impl Module {
    fn declare(&self, names: &[&str]) {
        if let Ok(mut d) = self.desired.lock() {
            *d = names.iter().map(|n| obj(n)).collect();
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    fn call(&self, c: String) {
        if let Ok(mut v) = self.calls.lock() {
            v.push(c);
        }
    }
}

impl Planner for Module {
    fn plan<'a>(&'a self, _ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>> {
        Box::pin(async move {
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .map(|a| a.port())
                .map_err(|e| e.to_string())?;
            let env = BTreeMap::from([("PORT".to_string(), port.to_string())]);
            let p = Process {
                name: "main".into(),
                program: HELPER.into(),
                args: vec!["listen".into()],
                env,
                dir: None,
                after: vec![],
                ready: Some(Ready::Tcp(format!("127.0.0.1:{port}"))),
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

    fn structure(&self) -> Option<&dyn Structure> {
        Some(self)
    }
}

impl Structure for Module {
    fn objects<'a>(&'a self, _ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Vec<Object>, String>> {
        Box::pin(async move { Ok(self.desired.lock().map(|d| d.clone()).unwrap_or_default()) })
    }

    fn converge<'a>(&'a self, ctx: &'a PlanCtx) -> BoxFuture<'a, Result<(), String>> {
        self.call(format!("converge {}", ctx.name));
        let broken = self.broken.lock().ok().and_then(|b| b.clone());
        Box::pin(async move { broken.map_or(Ok(()), Err) })
    }

    fn fingerprint(&self) -> String {
        self.fp.lock().map(|f| f.clone()).unwrap_or_default()
    }

    fn prune<'a>(
        &'a self,
        _ctx: &'a PlanCtx,
        removed: &'a [Object],
    ) -> BoxFuture<'a, Result<(), String>> {
        let names: Vec<&str> = removed.iter().map(|o| o.name.as_str()).collect();
        self.call(format!("prune {}", names.join(",")));
        Box::pin(async { Ok(()) })
    }
}

#[allow(clippy::unwrap_used)] // test code
fn engine(d: &Dir, m: &Arc<Module>) -> Engine {
    Engine::builder("t", d.path())
        .service(Service::planned("db", m.clone()))
        .service(svc("web", listener(d, 0)))
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sync_converges_then_prunes_what_is_no_longer_declared() {
    let d = Dir::new("sync");
    let m = Arc::new(Module::default());
    let e = engine(&d, &m);
    m.declare(&["a", "b", "c"]);
    let got = e.sync(None).await.unwrap();
    let creates: Vec<(Change, String)> = got
        .iter()
        .flat_map(|(_, ch)| ch.iter().map(|(c, o)| (*c, o.name.clone())))
        .collect();
    assert_eq!(
        creates,
        [
            (Change::Create, "a".into()),
            (Change::Create, "b".into()),
            (Change::Create, "c".into())
        ]
    );
    assert_eq!(got.len(), 1, "web has no structure: {got:?}");
    assert!(e.get("db").unwrap().state.running(), "left running");
    assert_eq!(e.get("web").unwrap().state, State::Reaped, "not woken");
    // `a` and `c` go: pruned in reverse of the order they were created.
    m.declare(&["b"]);
    e.sync(Some("db")).await.unwrap();
    // The first wake converged the fresh instance, then each sync converged.
    assert_eq!(
        m.calls(),
        ["converge db", "converge db", "converge db", "prune c,a"]
    );
    let st = Applied::load(&d.path().join(".doze/state.json")).unwrap();
    assert_eq!(st.objects("db"), [obj("b")]);
    e.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn destroy_drops_everything_and_an_off_instance_is_not_synced() {
    let d = Dir::new("destroy");
    let m = Arc::new(Module::default());
    let e = engine(&d, &m);
    m.declare(&["a", "b"]);
    e.sync(None).await.unwrap();
    e.destroy(None).await.unwrap();
    assert_eq!(m.calls(), ["converge db", "converge db", "prune b,a"]);
    assert_eq!(
        e.get("db").unwrap().state,
        State::Reaped,
        "stopped after the drops"
    );
    let st = Applied::load(&d.path().join(".doze/state.json")).unwrap();
    assert!(st.objects("db").is_empty());
    e.set_off("db", true).await.unwrap();
    e.sync(None).await.unwrap();
    assert_eq!(m.calls().len(), 3, "off: not converged");
    assert!(e.sync(Some("nope")).await.is_err());
    let _ = e.wake("web", Cause::Command).await;
    e.shutdown().await;
}

/// Go's `Destroy`: one instance's destroy keeps the outputs, a whole destroy forgets them; an instance in the state that
/// the config no longer declares is forgotten without a module call; the converged marker goes with the structure (E2
/// audit 2, n9: S1, S8).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn destroy_forgets_outputs_and_undeclared_instances_and_the_converged_marker() {
    let d = Dir::new("destroyall");
    let m = Arc::new(Module::default());
    let e = engine(&d, &m);
    m.declare(&["a"]);
    e.sync(None).await.unwrap();
    let path = d.path().join(".doze/state.json");
    let mut st = Applied::load(&path).unwrap();
    st.set("gone", vec![obj("x")]);
    st.outputs.insert("db_url".into(), "postgres://x".into());
    st.save(&path).unwrap();
    let marker = e
        .stack()
        .cluster_dir("db")
        .join(doze_engine::sync::CONVERGED_MARKER);
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "fp").unwrap();
    e.destroy(Some("db")).await.unwrap();
    assert!(
        !marker.exists(),
        "the converged marker goes with the structure"
    );
    let st = Applied::load(&path).unwrap();
    assert_eq!(
        st.outputs.len(),
        1,
        "one instance's destroy keeps the outputs"
    );
    assert_eq!(st.objects("gone"), [obj("x")]);
    e.destroy(None).await.unwrap();
    let st = Applied::load(&path).unwrap();
    assert!(st.outputs.is_empty(), "a whole destroy forgets the outputs");
    assert!(
        st.instances.is_empty(),
        "undeclared: forgotten: {:?}",
        st.instances
    );
    assert_eq!(
        m.calls(),
        ["converge db", "converge db", "prune a"],
        "nothing asked of a module for gone"
    );
    e.shutdown().await;
}

/// E2 audit 2, M1: a task the sync's wakes ran keeps its record (the sync saves over the file as it is then).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sync_keeps_the_records_of_the_tasks_it_ran() {
    let d = Dir::new("synctask");
    let m = Arc::new(Module::default());
    let ran = d.file("ran");
    let migrate =
        doze_engine::tasks::TaskSpec::new(run(&["append", &ran.to_string_lossy()]), d.path());
    let e = Engine::builder("t", d.path())
        .service(Service::task("migrate", migrate))
        .service(Service::planned("db", m.clone()).needs("migrate"))
        .build()
        .unwrap();
    m.declare(&["a"]);
    e.sync(None).await.unwrap();
    assert_eq!(lines(&ran), 1);
    assert_eq!(
        e.task_record("migrate").map(|r| r.result),
        Some("done".into())
    );
    let st = Applied::load(&d.path().join(".doze/state.json")).unwrap();
    assert_eq!(st.objects("db"), [obj("a")]);
    e.shutdown().await;
}

/// Go's boot guard: a fresh instance is converged as it starts (its databases exist after `up`, no `apply`), once;
/// a changed declaration converges on the next start; a failed converge stops it, taints it and leaves no marker, so
/// the next start tries again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_start_converges_a_fresh_or_changed_instance_once() {
    let d = Dir::new("bootconverge");
    let m = Arc::new(Module::default());
    *m.fp.lock().unwrap() = "one".into();
    let e = engine(&d, &m);
    let marker = d.path().join("clusters/db/.doze-converged");
    e.wake("db", Cause::Command).await.unwrap();
    assert_eq!(m.calls(), ["converge db"], "fresh: converged as it started");
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        "one",
        "Go's marker: the fingerprint, no newline"
    );
    e.stop("db", Cause::Command).await.unwrap();
    e.wake("db", Cause::Command).await.unwrap();
    assert_eq!(m.calls(), ["converge db"], "converged before: not again");
    e.stop("db", Cause::Command).await.unwrap();
    *m.fp.lock().unwrap() = "two".into();
    *m.broken.lock().unwrap() = Some("role \"app\" cannot be made".into());
    let err = e.wake("db", Cause::Command).await.unwrap_err().to_string();
    assert!(
        err.contains("provisioning \"db\": role \"app\" cannot be made"),
        "{err}"
    );
    let view = e.get("db").unwrap();
    assert!(view.tainted && !view.state.running(), "{view:?}");
    assert!(!marker.exists(), "a failed converge leaves no marker");
    *m.broken.lock().unwrap() = None;
    e.wake("db", Cause::Command).await.unwrap();
    assert_eq!(
        m.calls().len(),
        3,
        "the next start tries again: {:?}",
        m.calls()
    );
    assert!(!e.get("db").unwrap().tainted);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "two");
    e.shutdown().await;
}

/// `plan`: what a sync would change, asked of the module without waking the instance or changing anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plan_lists_the_changes_and_changes_nothing() {
    let d = Dir::new("plan");
    let m = Arc::new(Module::default());
    let e = engine(&d, &m);
    m.declare(&["a", "b"]);
    let words = |p: doze_engine::sync::Synced| -> Vec<String> {
        p.into_iter()
            .flat_map(|(n, ch)| {
                ch.into_iter()
                    .map(move |(c, o)| format!("{n} {} {}", c.as_str(), o.name))
            })
            .collect()
    };
    assert_eq!(
        words(e.plan(None).await.unwrap()),
        ["db create a", "db create b"]
    );
    assert_eq!(e.get("db").unwrap().state, State::Reaped, "not woken");
    assert!(m.calls().is_empty(), "nothing converged: {:?}", m.calls());
    e.sync(None).await.unwrap();
    assert!(
        e.plan(None).await.unwrap().is_empty(),
        "in line: no changes"
    );
    m.declare(&["b"]);
    assert_eq!(words(e.plan(Some("db")).await.unwrap()), ["db delete a"]);
    e.shutdown().await;
}
