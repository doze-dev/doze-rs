//! The console's blocks on the real server (`DOZE_POSTGRES_BINDIR`): the schema tree lists what was made (a name with a
//! slash in it included) a page at a time and finds a table by part of its name; its leaves open as JSON (a table's
//! rows, a column's definition); a session's whole query opens past the table's 600 characters, and an ended session
//! says so; the create-database form makes a database, refuses a name taken on its field, and still runs from an app
//! that sends only the one box.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{graceful, harness, on_build, socket, sql};
use doze_module_proto::msg::blocks::{Children, ChildrenArgs, ValueAnswer, ValueArgs};
use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_test::Booted;
use std::collections::BTreeMap;
use std::time::Duration;

async fn children(b: &Booted, parent: &str, filter: &str, cursor: Option<String>) -> Children {
    let a = ChildrenArgs {
        instance: b.instance.clone(),
        panel: "browse".into(),
        parent: parent.into(),
        filter: filter.into(),
        cursor,
    };
    b.module.children(&a).await.unwrap()
}

fn ids(c: &Children) -> Vec<&str> {
    c.nodes.iter().map(|n| n.id.as_str()).collect()
}

async fn value(b: &Booted, panel: &str, row: &[(&str, &str)]) -> Result<ValueAnswer, Error> {
    let a = ValueArgs {
        instance: b.instance.clone(),
        panel: panel.into(),
        row: row
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        offset: 0,
        max: 0,
    };
    b.module.value(&a).await
}

fn text(v: &ValueAnswer) -> String {
    String::from_utf8(v.body.clone()).unwrap()
}

async fn create(
    b: &Booted,
    values: &[(&str, &str)],
    input: Option<&str>,
    form: bool,
) -> Result<String, Error> {
    let a = RunAction {
        instance: b.instance.clone(),
        action: "new-database".into(),
        resource: String::new(),
        input: input.map(String::from),
        values: values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        form,
    };
    b.module.run_action(&a).await
}

#[tokio::test(flavor = "current_thread")]
async fn the_blocks_read_the_server() {
    let Some(h) = harness() else { return };
    let b = h
        .boot(&on_build(&h, "postgres \"shop\" {\n  version = 17\n}\n"))
        .await
        .unwrap();
    let q = |q: &str| sql(&b, "postgres", "shop", q).unwrap();
    q(r#"CREATE SCHEMA sales;
         CREATE TABLE sales."or/ders" (id int PRIMARY KEY, note text NOT NULL DEFAULT 'none', total numeric(10,2));
         COMMENT ON COLUMN sales."or/ders".total IS 'in euros';
         INSERT INTO sales."or/ders" VALUES (1, 'first', 9.50), (2, 'second', 12.00);
         CREATE VIEW sales.big AS SELECT * FROM sales."or/ders" WHERE total > 10;
         CREATE TABLE public.wide (id int, blob text);
         INSERT INTO public.wide SELECT i, repeat('w', 30000) FROM generate_series(1, 100) i;
         CREATE SCHEMA many;
         DO $$ BEGIN FOR i IN 1..250 LOOP EXECUTE format('CREATE TABLE many.t%s (v int)', lpad(i::text, 3, '0')); END LOOP; END $$"#);
    tree(&b).await;
    pages_and_filters(&b).await;
    leaves(&b).await;
    session_query(&b).await;
    form(&b).await;
    graceful(&b.stop(Duration::from_secs(20)).await);
}

async fn tree(b: &Booted) {
    let top = children(b, "", "", None).await;
    assert_eq!(ids(&top), ["shop"], "no template, not the server's own");
    let schemas = children(b, "shop", "", None).await;
    assert_eq!(ids(&schemas), ["shop/many", "shop/public", "shop/sales"]);
    let sales = children(b, "shop/sales", "", None).await;
    assert_eq!(ids(&sales), ["shop/sales/big", "shop/sales/or%2Fders"]);
    assert_eq!(sales.nodes[0].info["kind"], "view");
    assert!(!sales.nodes[1].leaf);
    let table = children(b, "shop/sales/or%2Fders", "", None).await;
    assert_eq!(
        ids(&table),
        [
            "shop/sales/or%2Fders/#rows",
            "shop/sales/or%2Fders/.id",
            "shop/sales/or%2Fders/.note",
            "shop/sales/or%2Fders/.total"
        ]
    );
    assert!(table.nodes.iter().all(|n| n.leaf));
    assert_eq!(table.nodes[3].info["type"], "numeric(10,2)");
    assert_eq!(table.nodes[3].info["null"], "nullable");
    assert!(
        !table.nodes[1].info.contains_key("null"),
        "a key is not null"
    );
}

async fn pages_and_filters(b: &Booted) {
    // 250 tables: a page, then the rest.
    let first = children(b, "shop/many", "", None).await;
    assert_eq!(first.nodes.len(), 200);
    let rest = children(b, "shop/many", "", first.next.clone()).await;
    assert_eq!((rest.nodes.len(), rest.next), (50, None));
    assert_eq!(rest.nodes[49].id, "shop/many/t250");
    // A filter searches every folder, opened or not, without case.
    let found = children(b, "", "DERS", None).await;
    assert_eq!(ids(&found), ["shop/sales/or%2Fders"]);
    let found = children(b, "", "sal", None).await;
    assert_eq!(
        ids(&found),
        ["shop/sales", "shop/sales/big", "shop/sales/or%2Fders"]
    );
    let found = children(b, "shop/many", "T24", None).await;
    assert_eq!(found.nodes.len(), 10, "t240..t249: {:?}", ids(&found));
}

/// Rows wider than one answer: as many as fit, whole and valid, in one part.
async fn wide_rows(b: &Booted) {
    // Rows wider than one answer: as many as fit, whole and valid, in one part (read once, never re-run for a part).
    let wide = value(b, "browse", &[("id", "shop/public/wide/#rows")])
        .await
        .unwrap();
    let body = text(&wide);
    assert!(!wide.more, "one part");
    assert!(body.len() <= 1 << 20, "{}", body.len());
    let kept = body.matches(r#""id":"#).count();
    assert!((30..100).contains(&kept), "{kept} rows fit");
    assert!(body.ends_with(']'), "a whole JSON array");
}

async fn leaves(b: &Booted) {
    let rows = value(b, "browse", &[("id", "shop/sales/or%2Fders/#rows")])
        .await
        .unwrap();
    assert_eq!(rows.media, "json");
    let body = text(&rows);
    assert!(
        body.starts_with('[') && body.contains(r#""note":"first""#) && body.contains("12.00"),
        "{body}"
    );
    assert!(!rows.more);
    let col = value(b, "browse", &[("id", "shop/sales/or%2Fders/.total")])
        .await
        .unwrap();
    let body = text(&col);
    for want in [
        r#""type" : "numeric(10,2)""#,
        r#""nullable" : true"#,
        r#""comment" : "in euros""#,
        r#""position" : 3"#,
    ] {
        assert!(body.contains(want), "{want} in {body}");
    }
    let note = text(
        &value(b, "browse", &[("id", "shop/sales/or%2Fders/.note")])
            .await
            .unwrap(),
    );
    assert!(note.contains(r#""default" : "'none'::text""#), "{note}");
    // A part at a time.
    let a = ValueArgs {
        instance: b.instance.clone(),
        panel: "browse".into(),
        row: BTreeMap::from([("id".into(), "shop/sales/or%2Fders/#rows".into())]),
        offset: 10,
        max: 5,
    };
    let part = b.module.value(&a).await.unwrap();
    assert_eq!((part.body.len(), part.more), (5, true));
    wide_rows(b).await;
    let gone = value(b, "browse", &[("id", "shop/sales/nope/#rows")])
        .await
        .unwrap_err();
    assert_eq!(gone.kind(), ErrorKind::NotFound, "{gone}");
    let gone = value(b, "browse", &[("id", "shop/sales/or%2Fders/.nope")])
        .await
        .unwrap_err();
    assert_eq!(gone.kind(), ErrorKind::NotFound, "{gone}");
}

/// A session running a query longer than the table shows: its whole text opens, and once it ends it says so.
async fn session_query(b: &Booted) {
    let (dir, port) = socket(b);
    let bin = std::path::Path::new(&b.toolchain.bin_dir).join("psql");
    let long = format!("SELECT pg_sleep(60)\n/* {} */", "x".repeat(700));
    let mut slow = tokio::process::Command::new(bin)
        .args([
            "-X", "-h", &dir, "-p", &port, "-U", "postgres", "-d", "shop", "-c",
        ])
        .arg(&long)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut pid = None;
    for _ in 0..50 {
        let rs = b.module.resources(&b.instance).await.unwrap();
        pid = rs
            .iter()
            .find(|r| r.kind == "session" && r.info["query"].contains("pg_sleep"))
            .map(|r| r.info["pid"].clone());
        if pid.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let pid = pid.expect("the sleeping session is listed");
    let v = value(b, "sessions", &[("pid", &pid), ("state", "active")])
        .await
        .unwrap();
    assert_eq!((v.media.as_str(), text(&v)), ("text", long.clone()));
    // A client killed leaves its backend asleep until it next writes: end the session as the console does.
    let end = RunAction {
        instance: b.instance.clone(),
        action: "terminate".into(),
        resource: pid.clone(),
        input: None,
        values: BTreeMap::new(),
        form: false,
    };
    b.module.run_action(&end).await.unwrap();
    let _ = slow.wait().await;
    let mut ended = None;
    for _ in 0..50 {
        match value(b, "sessions", &[("pid", &pid)]).await {
            Err(e) => {
                ended = Some(e);
                break;
            }
            Ok(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let e = ended.expect("the session ends");
    assert_eq!(e.kind(), ErrorKind::NotFound);
    assert!(e.message.contains("has ended"), "{e}");
}

async fn form(b: &Booted) {
    let q = |q: &str| sql(b, "postgres", "shop", q).unwrap();
    let done = create(b, &[("name", "made here")], None, true)
        .await
        .unwrap();
    assert_eq!(done, "created database made here, owned by postgres");
    assert_eq!(
        q(
            "SELECT pg_get_userbyid(datdba), pg_encoding_to_char(encoding) FROM pg_database WHERE datname = 'made here'"
        ),
        "postgres|UTF8"
    );
    // The tree lists it.
    assert!(ids(&children(b, "", "", None).await).contains(&"made here"));
    let taken = create(b, &[("name", "made here")], None, true)
        .await
        .unwrap_err();
    assert_eq!(taken.field.as_deref(), Some("name"), "{taken}");
    assert!(taken.message.contains("already exists"), "{taken}");
    let blank = create(b, &[("name", " ")], None, true).await.unwrap_err();
    assert_eq!(blank.field.as_deref(), Some("name"), "{blank}");
    let nobody = create(b, &[("name", "x"), ("owner", "nobody")], None, true)
        .await
        .unwrap_err();
    assert_eq!(nobody.field.as_deref(), Some("owner"), "{nobody}");
    let ascii = [("name", "ascii"), ("encoding", "SQL_ASCII")];
    let e = create(b, &ascii, None, true).await.unwrap_err();
    assert_eq!(e.field.as_deref(), Some("encoding"), "{e}");
    create(
        b,
        &[ascii[0], ascii[1], ("template", "template0")],
        None,
        true,
    )
    .await
    .unwrap();
    // An app that does not know forms sends the one box as input.
    let done = create(b, &[], Some("legacy"), false).await.unwrap();
    assert_eq!(done, "created database legacy, owned by postgres");
    assert_eq!(
        q("SELECT count(*) FROM pg_database WHERE datname IN ('legacy', 'ascii')"),
        "2"
    );
}
