//! The declared state on the real server, in the corners the independent audit found: a grant whose grantee
//! re-granted with its grant option; a database closed to connections, which prune still works in; a role setting
//! that is a list; a pruned role that owns objects in another database; two converges racing; a value that would
//! escape its quotes with `standard_conforming_strings` off.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{graceful, harness, on_build, reconverge, sql};
use doze_module_proto::msg::run::WithToolchain;
use std::time::Duration;

const REGRANT: &str = r#"postgres "re" {
  version = 17
  role "app" {}
  role "bob" {}
  schema "s" { owner = "postgres" }
  grant {
    role       = "app"
    privileges = ["USAGE"]
    schema     = "s"
  }
  grant {
    role              = "app"
    privileges        = ["SELECT", "INSERT"]
    schema            = "s"
    objects           = "tables"
    with_grant_option = true
  }
}
"#;

#[tokio::test(flavor = "current_thread")]
async fn a_regrant_by_the_grantee_does_not_block_converge() {
    let Some(h) = harness() else { return };
    let b = h.boot(&on_build(&h, REGRANT)).await.unwrap();
    let q = |q: &str| sql(&b, "postgres", "re", q).unwrap();
    q("CREATE TABLE s.t (v int)");
    let with = WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    };
    b.module.converge(&with).await.unwrap();
    sql(&b, "app", "re", "GRANT SELECT ON s.t TO bob").unwrap();
    assert_eq!(q("SELECT has_table_privilege('bob', 's.t', 'SELECT')"), "t");
    let narrowed = REGRANT
        .replace("[\"SELECT\", \"INSERT\"]", "[\"SELECT\"]")
        .replace("with_grant_option = true", "");
    reconverge(&b, &on_build(&h, &narrowed)).await;
    assert_eq!(q("SELECT has_table_privilege('app', 's.t', 'INSERT')"), "f");
    assert_eq!(
        q("SELECT has_table_privilege('app', 's.t', 'SELECT WITH GRANT OPTION')"),
        "f"
    );
    assert_eq!(
        q("SELECT has_table_privilege('bob', 's.t', 'SELECT')"),
        "f",
        "what app granted went with its option"
    );
    graceful(&b.stop(Duration::from_secs(20)).await);
}

const CLOSED: &str = r#"postgres "closed" {
  version = 17
  allow_connections = false
  extensions        = ["hstore"]
  role "app" {
    config = { search_path = "app, public" }
  }
}
"#;

#[tokio::test(flavor = "current_thread")]
async fn a_closed_database_is_pruned_in_and_closed_again() {
    let Some(h) = harness() else { return };
    let b = h.boot(&on_build(&h, CLOSED)).await.unwrap();
    let q = |q: &str| sql(&b, "postgres", "postgres", q).unwrap();
    assert_eq!(
        q("SELECT datallowconn FROM pg_database WHERE datname = 'closed'"),
        "f"
    );
    // A list setting is a list: the role's search path is two schemas, not one named "app, public".
    assert_eq!(
        q(
            "SELECT array_to_string(setconfig, ';') FROM pg_db_role_setting s JOIN pg_roles r ON r.oid = s.setrole WHERE r.rolname = 'app'"
        ),
        "search_path=app, public"
    );
    let open = CLOSED.replace("  extensions        = [\"hstore\"]\n", "");
    let removed = reconverge(&b, &on_build(&h, &open)).await;
    assert_eq!(
        removed.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["hstore"]
    );
    assert_eq!(
        q("SELECT datallowconn FROM pg_database WHERE datname = 'closed'"),
        "f",
        "closed again"
    );
    q("ALTER DATABASE closed WITH ALLOW_CONNECTIONS true");
    assert_eq!(
        sql(
            &b,
            "postgres",
            "closed",
            "SELECT count(*) FROM pg_extension WHERE extname = 'hstore'"
        )
        .unwrap(),
        "0"
    );
    graceful(&b.stop(Duration::from_secs(20)).await);
}

const OWNER: &str = r#"postgres "own" {
  version = 17
  role "old" {
    comment = "x\\'; CREATE ROLE pwned SUPERUSER; --"
  }
}
"#;

#[tokio::test(flavor = "current_thread")]
async fn a_role_owning_another_database_is_pruned_and_quotes_hold() {
    let Some(h) = harness() else { return };
    let mut hcl = on_build(&h, OWNER);
    hcl = hcl.replace(
        "version = ",
        "settings = { standard_conforming_strings = \"off\" }\n  version = ",
    );
    let b = h.boot(&hcl).await.unwrap();
    let q = |db: &str, s: &str| sql(&b, "postgres", db, s).unwrap();
    // The comment stayed a comment, with standard_conforming_strings off on the server.
    assert_eq!(
        q(
            "own",
            "SELECT count(*) FROM pg_roles WHERE rolname = 'pwned'"
        ),
        "0"
    );
    q("postgres", "CREATE DATABASE other");
    q(
        "other",
        "CREATE TABLE kept (v int); INSERT INTO kept VALUES (3); ALTER TABLE kept OWNER TO old",
    );
    // Two converges at once (a boot and a first connection) both succeed.
    let with = WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    };
    let (x, y) = tokio::join!(b.module.converge(&with), b.module.converge(&with));
    x.unwrap();
    y.unwrap();
    let without = hcl.replace(
        "  role \"old\" {\n    comment = \"x\\\\'; CREATE ROLE pwned SUPERUSER; --\"\n  }\n",
        "",
    );
    let removed = reconverge(&b, &without).await;
    assert_eq!(
        removed.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["old"]
    );
    assert_eq!(
        q(
            "other",
            "SELECT v || ' ' || tableowner FROM kept, pg_tables WHERE tablename = 'kept'"
        ),
        "3 postgres"
    );
    graceful(&b.stop(Duration::from_secs(20)).await);
}
