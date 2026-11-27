//! A module process is started when the daemon needs it to do something, never to read what it declares. The front
//! asked the module pool for the module of every connection to a main listener, to see whether the module takes its
//! clients itself (`wire.accept`); for kafka (it does not) that started `kafka-plugin` again after each idle reap, only
//! to hear "no". The echo stands in: a copy named `…-nowire` leaves its clients to the front.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use doze_daemon::catalog::Catalog;
use doze_engine::modules::Pinned;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod daemon_support;
use daemon_support::*;

async fn echoed(addr: &str) {
    let c = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("{addr}: {e}"));
    let mut c = BufReader::new(c);
    c.get_mut().write_all(b"ping\n").await.unwrap();
    let mut line = String::new();
    tokio::time::timeout(std::time::Duration::from_secs(30), c.read_line(&mut line))
        .await
        .expect("an answer within 30s")
        .unwrap();
    assert_eq!(line, "ping\n", "echoed through the front at {addr}");
}

/// The project's daemon answering its front, its module the copy that leaves its clients to the front.
struct Served {
    dir: std::path::PathBuf,
    cat: Arc<Catalog>,
    d: doze_daemon::Daemon,
    front: Arc<doze_daemon::front::Front>,
    e: Arc<doze_engine::modules::ModuleInstance>,
}

async fn served() -> Served {
    let (dir, _) = project("front-nowire");
    let nowire = dir.join(format!(
        "doze-module-echo-nowire{}",
        std::env::consts::EXE_SUFFIX
    ));
    std::fs::copy(env!("CARGO_BIN_EXE_doze-module-echo"), &nowire).unwrap();
    let lock = dir.join("doze.lock");
    let downloads = Arc::new(Pinned {
        inner: Shared(Arc::new(Fetch::default())),
        pins: doze_daemon::catalog::lock_pins(&lock),
    });
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        Arc::new(Find {
            echo: Some(nowire),
            ..Find::default()
        }),
        downloads,
        lock,
        dir.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        dir.join("home").to_string_lossy().into_owned(),
    )]);
    let config = dir.join("doze.hcl");
    let mut a = doze_daemon::entry::assemble(&config, "0.0.0", cat.clone(), env.clone())
        .await
        .unwrap();
    let wire = cat.manifest("echo").unwrap().caps & doze_module_proto::msg::hello::caps::WIRE;
    assert_eq!(wire, 0, "the copy does not take its clients");
    let front = Arc::new(a.open_front());
    cat.attach(&a.engine);
    let tls = a.tls.clone();
    let e = a.modules.get("e").cloned().unwrap();
    let live = doze_daemon::live::Live::new(cat.clone(), &config, "0.0.0", env, front.clone());
    let d = doze_daemon::Daemon::at(
        a.engine,
        a.labels,
        doze_daemon::StackInfo::default(),
        dir.join("ctl.sock"),
    )
    .with_modules(a.modules)
    .with_live(live);
    d.answer_front(&front, tls);
    Served {
        dir,
        cat,
        d,
        front,
        e,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connection_to_a_module_that_leaves_its_clients_to_the_front_starts_no_module() {
    let Served {
        dir,
        cat,
        d,
        front,
        e,
    } = served().await;
    let addr = front.endpoint("e").unwrap();

    // The first connection wakes it: its module plans the start, so it runs.
    echoed(&addr).await;
    assert!(d.engine().get("e").unwrap().state.running());

    // The pool reaps the idle module (its engine runs on); connections after that start nothing.
    cat.pool().stop_all().await;
    let launched = cat.pool().launches();
    for _ in 0..3 {
        echoed(&addr).await;
    }
    assert_eq!(
        cat.pool().launches(),
        launched,
        "a connection started the module process"
    );
    assert!(cat.pool().running().await.is_empty(), "no module runs");

    // Nor does a question about what the module declares (the console's, the data view's).
    assert!(e.has_admin().await.unwrap());
    assert!(e.has_migrate().await.unwrap());
    assert!(e.manifest().await.unwrap().console.is_some());
    assert_eq!(
        cat.pool().launches(),
        launched,
        "reading the manifest started the module process"
    );

    front.close();
    d.engine().shutdown().await;
    cat.pool().stop_all().await;
    let _ = std::fs::remove_dir_all(&dir);
}
