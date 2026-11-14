//! The daemon's data ops and console topic with the echo module (Go's `Resources`, `Admin`, `Reset` and
//! `subConsole`): what the wire carries is what the module answers, and nothing in the daemon names the module.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod daemon_support;

use daemon_support::*;
use doze_daemon::catalog::Catalog;
use doze_daemon::{Daemon, StackInfo};
use doze_engine::modules::Pinned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use std::time::Duration;

struct Raw {
    w: doze_os::local::LocalStream,
    r: BufReader<doze_os::local::LocalStream>,
}

impl Raw {
    fn open(sock: &std::path::Path) -> Raw {
        let w = doze_os::local::LocalStream::connect(sock).unwrap();
        w.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let r = BufReader::new(w.try_clone().unwrap());
        let mut c = Raw { w, r };
        c.say(r#"{"hello":{"proto":2}}"#);
        c.hear();
        c
    }
    fn say(&mut self, line: &str) {
        self.w.write_all(format!("{line}\n").as_bytes()).unwrap();
    }
    fn hear(&mut self) -> Value {
        let mut s = String::new();
        self.r.read_line(&mut s).unwrap();
        serde_json::from_str(&s).unwrap()
    }
    /// A unary answer (the frame's `response`).
    fn ask(&mut self, line: &str) -> Value {
        self.say(line);
        self.hear()["response"].clone()
    }
}

async fn serve(dir: &std::path::Path) -> (Daemon, doze_daemon::Serving, std::path::PathBuf) {
    let lock = dir.join("doze.lock");
    let downloads = Arc::new(Pinned {
        inner: Shared(Arc::new(Fetch::default())),
        pins: doze_daemon::catalog::lock_pins(&lock),
    });
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find::default()),
        downloads,
        lock,
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let a = doze_daemon::entry::assemble(&dir.join("doze.hcl"), "0.0.0", cat, env)
        .await
        .unwrap();
    let sock = dir.join("ctl.sock");
    let d =
        Daemon::at(a.engine, a.labels, StackInfo::default(), sock.clone()).with_modules(a.modules);
    let s = d.serve().unwrap();
    (d, s, sock)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resources_actions_the_console_and_a_reset_come_from_the_module() {
    let (dir, _port) = project("admin");
    let (d, s, sock) = serve(&dir).await;
    let mut c = tokio::task::spawn_blocking({
        let sock = sock.clone();
        move || Raw::open(&sock)
    })
    .await
    .unwrap();
    let r = c.ask(r#"{"id":1,"op":"resources","db":"e"}"#);
    assert_eq!(
        r["error"], "instance \"e\" is not running — boot it first",
        "{r}"
    );
    // The plan: what a sync would make, asked of the module without waking it.
    let r = c.ask(r#"{"id":30,"op":"plan"}"#);
    assert_eq!(
        r["changes"],
        serde_json::json!([{"instance":"e","change":"create","kind":"channel","name":"a"}]),
        "{r}"
    );
    d.engine()
        .wake("e", doze_engine::Cause::Command)
        .await
        .unwrap();
    the_client(&mut c);

    let r = c.ask(r#"{"id":2,"op":"resources","db":"e"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        r["resources"][0],
        serde_json::json!({"kind":"channel","name":"a","status":"open","info":{"size":"2"}})
    );
    assert_eq!(
        r["actions"][0],
        serde_json::json!({"id":"purge","label":"Purge","kind":"channel","destructive":true})
    );
    let r = c.ask(r#"{"id":3,"op":"admin","db":"e","action":"purge","resource":"a"}"#);
    assert_eq!(r["result"], "purged a", "{r}");
    let r = c.ask(r#"{"id":4,"op":"admin","db":"show","action":"purge","resource":"a"}"#);
    assert_eq!(
        r["error"], "instance \"show\" (process) has no data actions",
        "{r}"
    );
    let r = c.ask(r#"{"id":5,"op":"resources","db":"nope"}"#);
    assert_eq!(
        (r["error"].as_str(), r["code"].as_str()),
        (Some("instance \"nope\" is not declared"), Some("not_found")),
        "{r}"
    );

    the_console(&mut c);

    // A reset stops it and takes its data folder.
    let data = d.engine().stack().cluster_dir("e");
    assert!(data.exists());
    let r = c.ask(r#"{"id":7,"op":"reset","db":"e"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert!(!data.exists());
    assert!(!d.engine().get("e").unwrap().state.running());
    s.stop();
    d.engine().shutdown().await;
}

/// The console: the spec and actions, the stream panel's rows, and the counters panel's rows.
fn the_console(c: &mut Raw) {
    let ack = c.ask(r#"{"id":6,"op":"subscribe","topics":["console"],"names":["e"],"panel":"tail","selector":"x"}"#);
    assert_eq!(ack["ok"], true);
    let first = c.hear();
    let f = &first["console"];
    assert_eq!(f["instance"], "e");
    assert_eq!(f["state"], "idle", "{first}");
    assert_eq!(f["spec"]["panels"][0]["id"], "channels", "{first}");
    assert_eq!(f["actions"][0]["id"], "purge");
    let mut rows = Vec::new();
    let mut counters = None;
    while counters.is_none() || rows.len() < 2 {
        let f = c.hear();
        let f = &f["console"];
        if f["panel"] == "tail" && !f["row"].is_null() {
            rows.push(f["row"]["cells"]["n"].as_str().unwrap().to_string());
        } else if f["panel"] == "channels" {
            counters = Some(f["rows"].clone());
        }
    }
    assert_eq!(rows, ["x1", "x2"]);
    assert_eq!(
        counters.unwrap(),
        serde_json::json!([{"cells":{"name":"a","size":"2","status":"open"}}])
    );
    c.say(r#"{"id":6,"op":"cancel"}"#);
    while c.hear()["end"] != true {}
}

/// `doze connect`'s command, resolved: the program in the engine's build, at doze's address for the instance.
fn the_client(c: &mut Raw) {
    let st = c.ask(r#"{"id":20,"op":"status"}"#);
    let e = st["instances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "e")
        .unwrap();
    let conn = &e["connect"];
    assert_eq!(conn["program"], "doze-module-echo-engine", "{e}");
    let (host, port) = e["endpoint"].as_str().unwrap().rsplit_once(':').unwrap();
    assert_eq!(conn["args"], serde_json::json!(["client", host, port]));
    assert_eq!(conn["fallbacks"], serde_json::json!(["nc"]));
    let path = conn["path"].as_str().unwrap();
    assert!(
        std::path::Path::new(path).is_file(),
        "the build's own program: {conn}"
    );
}
