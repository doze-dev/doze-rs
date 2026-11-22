//! Every listener a module declares is granted, where nothing fronts it too. The published temporal module would not
//! boot under the daemon ("no backend granted for \"ui\""): it is supervised, so it has no front, and an instance with
//! no front was granted its `main` listener alone. The echo stands in: a copy named `…-supervised` is supervised, and
//! the setting `side` declares a second listener its engine must be granted.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use doze_daemon::catalog::Catalog;
use doze_engine::modules::Pinned;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod daemon_support;
use daemon_support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_supervised_module_is_granted_every_listener_it_declares() {
    let d = std::env::temp_dir().join(format!("doze-daemon-listeners-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("home")).unwrap();
    let supervised = d.join(format!(
        "doze-module-echo-supervised{}",
        std::env::consts::EXE_SUFFIX
    ));
    std::fs::copy(env!("CARGO_BIN_EXE_doze-module-echo"), &supervised).unwrap();
    let (main, side) = (free_port(), free_port());
    let hcl = format!(
        "module echo \"s\" {{\n  version = \"2\"\n  port = {main}\n  settings = {{\n    side = \"{side}\"\n  }}\n}}\n"
    );
    std::fs::write(d.join("doze.hcl"), hcl).unwrap();
    let lock = d.join("doze.lock");
    let find = Arc::new(Find {
        echo: Some(supervised),
        ..Find::default()
    });
    let downloads = Arc::new(Pinned {
        inner: Shared(Arc::new(Fetch::default())),
        pins: doze_daemon::catalog::lock_pins(&lock),
    });
    let cat = Arc::new(Catalog::new(
        tokio::runtime::Handle::current(),
        find,
        downloads,
        lock,
        d.join("home"),
    ));
    let env = BTreeMap::from([(
        "DOZE_HOME".to_string(),
        d.join("home").to_string_lossy().into_owned(),
    )]);
    let a = doze_daemon::entry::assemble(&d.join("doze.hcl"), "0.0.0", cat, env)
        .await
        .unwrap();
    assert!(a.supervised.contains("s"), "the copy is supervised");
    assert!(a.fronted.is_empty(), "nothing fronts a supervised module");

    // It boots (it refused before: no backend granted for "side"), each engine where it was declared.
    a.engine
        .wake("s", doze_engine::Cause::Command)
        .await
        .unwrap_or_else(|e| panic!("s: {e}"));
    for port in [main, side] {
        let c = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap_or_else(|e| panic!("127.0.0.1:{port}: {e}"));
        let mut c = BufReader::new(c);
        c.get_mut().write_all(b"ping\n").await.unwrap();
        let mut line = String::new();
        c.read_line(&mut line).await.unwrap();
        assert_eq!(line, "ping\n", "the engine on {port}");
    }

    // While the config loads, the module is asked about its instance with every listener it declared.
    let l = a.labels.get("s").unwrap();
    assert_eq!(
        l.env.get("ECHO_SIDE").map(String::as_str),
        Some(format!("127.0.0.1:{side}").as_str()),
        "{:?}",
        l.env
    );
    a.engine.shutdown().await;
    let _ = std::fs::remove_dir_all(&d);
}
