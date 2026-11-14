//! mariadb on the real server (`DOZE_MARIADB_BINDIR`, from doze-binaries). Each test is a promise the module makes,
//! checked on the engine: the published example's user logs in with its password and only then (no anonymous account
//! shadows it, which Go never tested); its grant lets it work in its database and nowhere else; converge keeps the
//! declared state (a privilege dropped is revoked, a password removed is cleared) and prune takes away what left
//! doze.hcl; the server stops gracefully. MariaDB ships for macOS and Linux here.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::calls::PruneArgs;
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, acceptance};
use doze_os::tree::Stopped;
use std::time::Duration;

fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-mariadb"), "mariadb").unwrap()
}

/// `mariadb -e sql` as `user` (with `password`, if any) on the instance's socket: its output, or its error.
fn sql(b: &Booted, user: &str, password: Option<&str>, q: &str) -> Result<String, String> {
    let Some(Address::Unix(sock)) = b.backend() else {
        panic!("a socket backend, got {:?}", b.backend())
    };
    let bin = std::path::Path::new(&b.toolchain.bin_dir).join("mariadb");
    let mut c = std::process::Command::new(bin);
    c.args([
        "--no-defaults",
        &format!("--socket={sock}"),
        &format!("--user={user}"),
        "--batch",
        "--skip-column-names",
        "-e",
        q,
    ]);
    if let Some(p) = password {
        c.env("MYSQL_PWD", p);
    }
    let out = c.output().unwrap();
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn graceful(stops: &doze_module_test::Stops) {
    assert!(
        !stops.is_empty()
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_declared_user_logs_in_with_its_password_and_works_in_its_database() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    assert_eq!(
        sql(&b, "app", Some("secret"), "SELECT CURRENT_USER()").unwrap(),
        "app@%"
    );
    let wrong = sql(&b, "app", Some("nope"), "SELECT 1").unwrap_err();
    assert!(wrong.contains("Access denied"), "{wrong}");
    // No anonymous account: a name nobody declared is refused, not let in as ''@localhost.
    let nobody = sql(&b, "nobody", None, "SELECT 1").unwrap_err();
    assert!(nobody.contains("Access denied"), "{nobody}");
    let anon = sql(
        &b,
        "root",
        None,
        "SELECT COUNT(*) FROM mysql.user WHERE user = ''",
    )
    .unwrap();
    assert_eq!(anon, "0");
    // The grant: everything in `app`, nothing elsewhere.
    sql(
        &b,
        "app",
        Some("secret"),
        "CREATE TABLE app.t (id INT); INSERT INTO app.t VALUES (1)",
    )
    .unwrap();
    let elsewhere = sql(&b, "app", Some("secret"), "CREATE DATABASE other").unwrap_err();
    assert!(elsewhere.contains("denied"), "{elsewhere}");
    // The character set the example declares.
    let cs = sql(&b, "root", None, "SELECT DEFAULT_CHARACTER_SET_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = 'app'").unwrap();
    assert_eq!(cs, "utf8mb4");
    graceful(&b.stop(Duration::from_secs(20)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn converge_keeps_the_declared_state_and_prune_takes_the_rest() {
    let Some(h) = harness() else { return };
    let first = "mariadb \"shop\" {\n  version = \"11.4\"\n  user \"app\" {\n    password = \"one\"\n  }\n  user \"old\" {}\n  grant {\n    user       = \"app\"\n    privileges = [\"SELECT\", \"INSERT\", \"GRANT OPTION\"]\n    database   = \"shop\"\n  }\n  grant {\n    user       = \"app\"\n    privileges = [\"SELECT\"]\n    database   = \"archive\"\n  }\n}\n";
    let b = h.boot(first).await.unwrap();
    let before = b.module.objects(&b.instance).await.unwrap();
    // The first declaration holds: app logs in with "one", may insert but not create.
    assert_eq!(
        sql(&b, "app", Some("one"), "SELECT CURRENT_USER()").unwrap(),
        "app@%"
    );
    let create = sql(&b, "app", Some("one"), "CREATE TABLE shop.x (i INT)").unwrap_err();
    assert!(create.contains("denied"), "{create}");
    let first_grants = sql(&b, "root", None, "SHOW GRANTS FOR 'app'@'%'").unwrap();
    assert!(
        first_grants.contains("WITH GRANT OPTION") && first_grants.contains("`archive`"),
        "{first_grants}"
    );
    // Now: app has no password and only SELECT; `old` is gone from doze.hcl.
    let second = "mariadb \"shop\" {\n  version = \"11.4\"\n  user \"app\" {}\n  grant {\n    user       = \"app\"\n    privileges = [\"SELECT\"]\n    database   = \"shop\"\n  }\n}\n";
    let parsed = doze_module_test::hcl::parse_block(second, &b.module.manifest.schema).unwrap();
    let mut inst = b.instance.clone();
    inst.spec = parsed.spec;
    let with = WithToolchain {
        instance: inst.clone(),
        toolchain: b.toolchain.clone(),
    };
    b.module.converge(&with).await.unwrap();
    let after = b.module.objects(&inst).await.unwrap();
    let removed: Vec<_> = before
        .into_iter()
        .filter(|o| !after.iter().any(|a| a.kind == o.kind && a.name == o.name))
        .collect();
    assert_eq!(
        removed.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["old@%", "app@% on `archive`.*"]
    );
    b.module
        .prune(&PruneArgs {
            instance: inst,
            toolchain: b.toolchain.clone(),
            removed,
        })
        .await
        .unwrap();
    // The password is cleared, INSERT is revoked, SELECT stays, `old` is gone.
    assert_eq!(
        sql(&b, "app", None, "SELECT CURRENT_USER()").unwrap(),
        "app@%"
    );
    let grants = sql(&b, "root", None, "SHOW GRANTS FOR 'app'@'%'").unwrap();
    assert!(grants.contains("GRANT SELECT ON `shop`.*"), "{grants}");
    assert!(!grants.contains("INSERT"), "{grants}");
    // GRANT OPTION dropped from the list is gone too (REVOKE ALL alone keeps it), and the removed grant is revoked.
    assert!(!grants.contains("GRANT OPTION"), "{grants}");
    assert!(!grants.contains("`archive`"), "{grants}");
    let old = sql(
        &b,
        "root",
        None,
        "SELECT COUNT(*) FROM mysql.user WHERE user = 'old'",
    )
    .unwrap();
    assert_eq!(old, "0");
    graceful(&b.stop(Duration::from_secs(20)).await);
}

/// The example's user logs in, over TCP when `tcp`, else on the socket.
fn logs_in(b: &Booted, tcp: bool) {
    let bin = std::path::Path::new(&b.toolchain.bin_dir).join("mariadb");
    let mut c = std::process::Command::new(bin);
    c.arg("--no-defaults");
    match b.backend() {
        Some(Address::Tcp(a)) if tcp => {
            let (h, p) = a.rsplit_once(':').unwrap();
            c.args([
                format!("--host={h}"),
                format!("--port={p}"),
                "--protocol=TCP".into(),
            ]);
        }
        Some(Address::Unix(s)) if !tcp => {
            c.arg(format!("--socket={s}"));
        }
        other => panic!("{other:?}"),
    }
    c.args([
        "--user=app",
        "--batch",
        "--skip-column-names",
        "-e",
        "SELECT CURRENT_USER()",
    ]);
    c.env("MYSQL_PWD", "secret");
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "app@%");
}

/// Where the host grants loopback TCP (Windows; any engine's TCP path): the server listens there, converge reaches it
/// there, and the declared user logs in over TCP.
#[tokio::test(flavor = "current_thread")]
async fn the_example_runs_on_a_tcp_backend() {
    let Some(h) = harness() else { return };
    let example = h.module().await.unwrap().manifest.example;
    let how = doze_module_test::Boot {
        tcp: true,
        template: false,
    };
    let b = h.boot_with(&example, how).await.unwrap();
    logs_in(&b, true);
    graceful(&b.stop(Duration::from_secs(20)).await);
}

/// A first boot from the shared template (made once, cloned into the data directory): provision finds the system
/// tables there, converge makes the example's database, user and grant, and the user logs in.
#[tokio::test(flavor = "current_thread")]
async fn the_example_boots_from_the_template() {
    let Some(h) = harness() else { return };
    let example = h.module().await.unwrap().manifest.example;
    let how = doze_module_test::Boot {
        tcp: false,
        template: true,
    };
    let b = h.boot_with(&example, how).await.unwrap();
    assert!(!b.fresh, "the clone is the data directory provision finds");
    logs_in(&b, false);
    graceful(&b.stop(Duration::from_secs(20)).await);
}
