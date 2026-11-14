//! postgres on the real server (`DOZE_POSTGRES_BINDIR`, from doze-binaries). Each test is a promise the module makes,
//! checked on the engine: the published example converges (roles, membership, schema, extension, grant, settings) and
//! psql connects; converge keeps the declared state and prune takes away what left doze.hcl, a role that owns a table
//! included, without losing the table; data survives sleep and wake; the server stops gracefully, listens on its
//! socket only, and the console reads it and cancels a query.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::calls::{PruneArgs, RunAction};
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, acceptance};
use doze_os::tree::Stopped;
use std::time::Duration;

fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-postgres"), "postgres").unwrap()
}

/// The build's major ("17" from "postgres (PostgreSQL) 17.9"): every block declares it, as the data must match.
fn major(h: &Harness) -> String {
    let out = std::process::Command::new(h.bin_dir.join("postgres"))
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .last()
        .unwrap()
        .split('.')
        .next()
        .unwrap()
        .to_string()
}

/// The block with the build's major in place of `version = 17`.
fn on_build(h: &Harness, hcl: &str) -> String {
    hcl.replace("version = 17", &format!("version = {}", major(h)))
}

/// The socket's directory and port.
fn socket(b: &Booted) -> (String, String) {
    let Some(Address::Unix(sock)) = b.backend() else {
        panic!("a socket backend, got {:?}", b.backend())
    };
    let p = std::path::Path::new(&sock);
    let port = p
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix(".s.PGSQL.")
        .unwrap();
    (
        p.parent().unwrap().to_string_lossy().into_owned(),
        port.to_string(),
    )
}

/// `psql -tA -c q` as `user` on `db` over the instance's socket: its output, or its error.
fn sql(b: &Booted, user: &str, db: &str, q: &str) -> Result<String, String> {
    let (dir, port) = socket(b);
    let bin = std::path::Path::new(&b.toolchain.bin_dir).join("psql");
    let out = std::process::Command::new(bin)
        .args([
            "-X",
            "-h",
            &dir,
            "-p",
            &port,
            "-U",
            user,
            "-d",
            db,
            "-v",
            "ON_ERROR_STOP=1",
            "-tAc",
            q,
        ])
        .output()
        .unwrap();
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
async fn the_published_example_converges_and_psql_connects() {
    let Some(h) = harness() else { return };
    let example = h.module().await.unwrap().manifest.example;
    let b = h
        .boot(&example.replace("version          = 18", &format!("version = {}", major(&h))))
        .await
        .unwrap();
    let q = |q: &str| sql(&b, "postgres", "app", q).unwrap();
    assert_eq!(sql(&b, "app", "app", "SELECT current_user").unwrap(), "app");
    assert_eq!(
        q("SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = 'app'"),
        "app"
    );
    assert_eq!(
        q("SELECT rolcanlogin, rolcreatedb, rolconnlimit FROM pg_roles WHERE rolname = 'app'"),
        "t|t|20"
    );
    assert_eq!(
        q("SELECT rolcanlogin FROM pg_roles WHERE rolname = 'readers'"),
        "f"
    );
    assert_eq!(q("SELECT pg_has_role('app', 'readers', 'MEMBER')"), "t");
    assert_eq!(
        q("SELECT nspowner::regrole FROM pg_namespace WHERE nspname = 'analytics'"),
        "app"
    );
    assert_eq!(
        q("SELECT extname FROM pg_extension WHERE extname = 'pg_trgm'"),
        "pg_trgm"
    );
    assert_eq!(
        q("SELECT shobj_description(oid, 'pg_database') FROM pg_database WHERE datname = 'app'"),
        "primary app database"
    );
    // The settings and the tuning: autovacuum on (the description's default), fsync off, the person's setting applied.
    assert_eq!(q("SHOW autovacuum"), "on");
    assert_eq!(q("SHOW fsync"), "off");
    assert_eq!(q("SHOW log_min_duration_statement"), "200ms");
    assert_eq!(q("SHOW max_connections"), "100");
    // The grant, and the default privileges: a table postgres makes later is the app's to write.
    q("CREATE TABLE analytics.events (id int)");
    sql(&b, "app", "app", "INSERT INTO analytics.events VALUES (1)").unwrap();
    // Socket only.
    assert_eq!(q("SHOW listen_addresses"), "");
    graceful(&b.stop(Duration::from_secs(20)).await);
}

async fn reconverge(b: &Booted, hcl: &str) -> Vec<doze_module_proto::msg::instance::Object> {
    let before = b.module.objects(&b.instance).await.unwrap();
    let parsed = doze_module_test::hcl::parse_block(hcl, &b.module.manifest.schema).unwrap();
    let mut inst = b.instance.clone();
    inst.spec = parsed.spec;
    b.module.decode(&inst).await.unwrap();
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
    let args = PruneArgs {
        instance: inst,
        toolchain: b.toolchain.clone(),
        removed: removed.clone(),
    };
    b.module.prune(&args).await.unwrap();
    removed
}

const FIRST: &str = r#"postgres "shop" {
  version = 17
  comment = "first"
  role "app" {
    password    = "one"
    valid_until = "2999-01-01"
    member_of   = ["readers", "writers"]
    comment     = "the app"
    config      = { work_mem = "8MB" }
  }
  role "readers" { login = false }
  role "writers" { login = false }
  role "old" {}
  schema "s" { owner = "app" }
  extension "hstore" {}
  grant {
    role       = "app"
    privileges = ["SELECT", "INSERT"]
    schema     = "s"
    objects    = "tables"
  }
  grant {
    role       = "readers"
    privileges = ["CONNECT", "TEMPORARY"]
    database   = "shop"
  }
}
"#;

const SECOND: &str = r#"postgres "shop" {
  version = 17
  role "app" {
    member_of = ["readers"]
  }
  role "readers" { login = false }
  role "writers" { login = false }
  schema "s" { owner = "app" }
  grant {
    role       = "app"
    privileges = ["SELECT"]
    schema     = "s"
    objects    = "tables"
  }
}
"#;

#[tokio::test(flavor = "current_thread")]
async fn converge_keeps_the_declared_state_and_prune_takes_the_rest() {
    let Some(h) = harness() else { return };
    let b = h.boot(&on_build(&h, FIRST)).await.unwrap();
    let q = |q: &str| sql(&b, "postgres", "shop", q).unwrap();
    // `old` owns a table holding data: dropping it must not lose the data.
    q(
        "CREATE TABLE public.kept (v int); INSERT INTO kept VALUES (7); ALTER TABLE kept OWNER TO old",
    );
    q("CREATE TABLE s.t (v int)");
    assert_eq!(q("SELECT has_table_privilege('app', 's.t', 'INSERT')"), "t");
    let removed = reconverge(&b, &on_build(&h, SECOND)).await;
    let names: Vec<_> = removed
        .iter()
        .map(|o| format!("{} {}", o.kind, o.name))
        .collect();
    assert_eq!(
        names,
        [
            "role old",
            "extension hstore",
            "grant \"readers\" on database \"shop\""
        ]
    );
    // Removed: membership, password, expiry, setting, comments; narrowed: the grant.
    assert_eq!(q("SELECT pg_has_role('app', 'writers', 'MEMBER')"), "f");
    assert_eq!(
        q("SELECT rolpassword IS NULL FROM pg_authid WHERE rolname = 'app'"),
        "t"
    );
    assert_eq!(
        q("SELECT coalesce(rolvaliduntil::text, 'never') FROM pg_roles WHERE rolname = 'app'"),
        "infinity"
    );
    assert_eq!(
        q(
            "SELECT count(*) FROM pg_db_role_setting s JOIN pg_roles r ON r.oid = s.setrole WHERE r.rolname = 'app'"
        ),
        "0"
    );
    assert_eq!(
        q(
            "SELECT coalesce(shobj_description(oid, 'pg_authid'), 'none') FROM pg_roles WHERE rolname = 'app'"
        ),
        "none"
    );
    assert_eq!(
        q(
            "SELECT coalesce(shobj_description(oid, 'pg_database'), 'none') FROM pg_database WHERE datname = 'shop'"
        ),
        "none"
    );
    assert_eq!(q("SELECT has_table_privilege('app', 's.t', 'INSERT')"), "f");
    assert_eq!(q("SELECT has_table_privilege('app', 's.t', 'SELECT')"), "t");
    assert_eq!(
        q("SELECT has_database_privilege('readers', 'shop', 'TEMPORARY')"),
        "t",
        "PUBLIC still has it"
    );
    assert_eq!(
        q("SELECT count(*) FROM pg_extension WHERE extname = 'hstore'"),
        "0"
    );
    // `old` is gone, its table kept and now postgres's.
    assert_eq!(
        q("SELECT count(*) FROM pg_roles WHERE rolname = 'old'"),
        "0"
    );
    assert_eq!(
        q("SELECT v || ' ' || tableowner FROM kept, pg_tables WHERE tablename = 'kept'"),
        "7 postgres"
    );
    graceful(&b.stop(Duration::from_secs(20)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn data_survives_sleep_and_wake_and_the_console_reads_the_server() {
    let Some(h) = harness() else { return };
    let b = h
        .boot(&on_build(&h, "postgres \"keep\" {\n  version = 17\n}\n"))
        .await
        .unwrap();
    sql(
        &b,
        "postgres",
        "keep",
        "CREATE TABLE t (v text); INSERT INTO t VALUES ('kept')",
    )
    .unwrap();
    let (stops, b) = b.sleep_and_wake(Duration::from_secs(20)).await.unwrap();
    graceful(&stops);
    assert!(!b.fresh);
    assert_eq!(
        sql(&b, "postgres", "keep", "SELECT v FROM t").unwrap(),
        "kept"
    );
    console_cancels_a_query(&b).await;
    graceful(&b.stop(Duration::from_secs(20)).await);
}

/// A session running a long query: listed, and its query cancelled through the console's action.
async fn console_cancels_a_query(b: &Booted) {
    let (dir, port) = socket(b);
    let bin = std::path::Path::new(&b.toolchain.bin_dir).join("psql");
    let slow = tokio::process::Command::new(bin)
        .args([
            "-X",
            "-h",
            &dir,
            "-p",
            &port,
            "-U",
            "postgres",
            "-d",
            "keep",
            "-c",
            "SELECT pg_sleep(60)",
        ])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut pid = None;
    for _ in 0..50 {
        let rs = b.module.resources(&b.instance).await.unwrap();
        assert!(rs.iter().any(|r| r.kind == "activity"), "{rs:?}");
        assert!(
            rs.iter().any(|r| r.kind == "database" && r.name == "keep"),
            "{rs:?}"
        );
        pid = rs
            .iter()
            .find(|r| r.kind == "session" && r.info["query"].contains("pg_sleep"))
            .map(|r| r.name.clone());
        if pid.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let pid = pid.expect("the sleeping session is listed");
    let act = |action: &str| RunAction {
        instance: b.instance.clone(),
        action: action.into(),
        resource: pid.clone(),
        input: None,
    };
    let done = b.module.run_action(&act("cancel")).await.unwrap();
    assert_eq!(done, format!("cancelled the query of session {pid}"));
    let out = tokio::time::timeout(Duration::from_secs(10), slow.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("canceling statement"),
        "{out:?}"
    );
    assert!(b.module.run_action(&act("explode")).await.is_err());
}
