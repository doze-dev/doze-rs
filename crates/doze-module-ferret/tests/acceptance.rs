//! ferret on the real builds (`DOZE_FERRET_BINDIR`: the FerretDB gateway; `DOZE_DOCUMENTDB_BINDIR`: PostgreSQL with the
//! DocumentDB extension; both from doze-binaries). Each test is a promise the module makes, checked on the engine: a
//! MongoDB client's hello is answered through the gateway's socket; the published example's collections and unique
//! index exist; seeds keep their numbers' kinds and are never doubled; converge rebuilds a changed index and prune drops
//! what left doze.hcl; nothing listens on TCP; both processes stop gracefully and the data is there after a wake.
//! ferret ships for macOS and Linux.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

// The module's own client, to read back what converge made. The hello below is written byte by byte instead, so the
// wire itself is checked against MongoDB's spec and not against this module.
#[path = "../src/bson.rs"]
#[allow(dead_code, unreachable_pub)]
mod bson;
#[path = "../src/msg.rs"]
#[allow(dead_code, unreachable_pub)]
mod msg;
#[path = "../src/wire.rs"]
#[allow(dead_code, unreachable_pub)]
mod wire;

use bson::{Bson, Doc};
use doze_module_proto::msg::calls::PruneArgs;
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, acceptance};
use doze_os::tree::Stopped;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn harness() -> Option<Harness> {
    let h = acceptance(env!("CARGO_BIN_EXE_doze-module-ferret"), "ferret").unwrap()?;
    assert!(
        std::env::var_os("DOZE_DOCUMENTDB_BINDIR").is_some(),
        "DOZE_FERRET_BINDIR is set: DOZE_DOCUMENTDB_BINDIR must name the PostgreSQL + DocumentDB build too"
    );
    Some(h)
}

fn socket(b: &Booted) -> String {
    match b.backend() {
        Some(Address::Unix(p)) => p,
        other => panic!("a socket backend, got {other:?}"),
    }
}

async fn client(b: &Booted) -> wire::Client {
    let s = doze_os::net::connect_unix(&socket(b)).await.unwrap();
    wire::Client::new(s, Duration::from_secs(30))
}

fn graceful(stops: &doze_module_test::Stops) {
    assert!(
        stops.len() == 2
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

/// `{hello: 1, $db: "admin"}` as OP_MSG, byte by byte from the MongoDB wire spec, and whether the reply says ok.
async fn hello(sock: &str) -> Vec<u8> {
    let mut doc: Vec<u8> = vec![0, 0, 0, 0, 0x10];
    doc.extend_from_slice(b"hello\0");
    doc.extend_from_slice(&1i32.to_le_bytes());
    doc.push(0x02);
    doc.extend_from_slice(b"$db\0");
    doc.extend_from_slice(&6i32.to_le_bytes());
    doc.extend_from_slice(b"admin\0\0");
    let n = doc.len() as i32;
    doc[..4].copy_from_slice(&n.to_le_bytes());
    let mut msg = Vec::new();
    for w in [16 + 4 + 1 + n, 42, 0, 2013] {
        msg.extend_from_slice(&w.to_le_bytes());
    }
    msg.extend_from_slice(&[0, 0, 0, 0, 0]);
    msg.extend_from_slice(&doc);
    let mut s = tokio::net::UnixStream::connect(sock).await.unwrap();
    s.write_all(&msg).await.unwrap();
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await.unwrap();
    let mut rest = vec![0u8; i32::from_le_bytes(len) as usize - 4];
    s.read_exact(&mut rest).await.unwrap();
    assert_eq!(
        i32::from_le_bytes(rest[4..8].try_into().unwrap()),
        42,
        "the reply answers our request"
    );
    assert_eq!(
        i32::from_le_bytes(rest[8..12].try_into().unwrap()),
        2013,
        "an OP_MSG reply"
    );
    rest
}

fn names(docs: &[Doc]) -> Vec<String> {
    let mut v: Vec<String> = docs
        .iter()
        .filter_map(|d| d.str("name").map(str::to_owned))
        .collect();
    v.sort();
    v
}

async fn collections(c: &mut wire::Client, db: &str) -> Vec<String> {
    names(
        &c.all(
            db,
            Doc::new().with("listCollections", 1),
            "$cmd.listCollections",
        )
        .await
        .unwrap(),
    )
}

async fn indexes(c: &mut wire::Client, db: &str, coll: &str) -> Vec<Doc> {
    c.all(db, Doc::new().with("listIndexes", coll), coll)
        .await
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn the_example_answers_a_mongodb_hello_with_its_collections_and_index() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    let reply = hello(&socket(&b)).await;
    let body = Doc::from_bytes(&reply[17..]).unwrap();
    assert_eq!(body.int("ok"), Some(1), "{body:?}");
    assert_eq!(
        body.get("isWritablePrimary"),
        Some(&Bson::Bool(true)),
        "{body:?}"
    );
    let mut c = client(&b).await;
    assert_eq!(collections(&mut c, "catalog").await, ["orders", "products"]);
    let ix = indexes(&mut c, "catalog", "products").await;
    let sku = ix
        .iter()
        .find(|d| d.str("name") == Some("sku_1"))
        .expect("the sku index");
    assert_eq!(sku.get("unique"), Some(&Bson::Bool(true)), "{sku:?}");
    // Nothing on TCP: PostgreSQL has its socket only, and the gateway's debug handler is off.
    let psql = std::path::Path::new(&b.toolchain.bin_dir).join("psql");
    let pg = std::path::Path::new(&b.instance.socket_dir).join("pg");
    let out = std::process::Command::new(psql)
        .args([
            "-h",
            pg.to_str().unwrap(),
            "-p",
            "5432",
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-XAtc",
            "SHOW listen_addresses",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "", "{out:?}");
    assert!(!b.log("ferretdb").contains("8088"), "{}", b.log("ferretdb"));
    graceful(&b.stop(Duration::from_secs(30)).await);
}

const SEEDED: &str = "ferret \"shop\" {\n  version = \"2.7\"\n  database \"shop\" {\n    collection \"products\" {\n      seed = \"tests/seed/products.json\"\n      index {\n        keys = { sku = 1 }\n      }\n      index {\n        name = \"by_qty\"\n        keys = { qty = -1 }\n      }\n    }\n    collection \"old\" {}\n  }\n}\n";

#[tokio::test(flavor = "current_thread")]
async fn seeds_keep_their_number_kinds_and_are_never_doubled() {
    let Some(h) = harness() else { return };
    let b = h.boot(SEEDED).await.unwrap();
    let mut c = client(&b).await;
    let found = Doc::new()
        .with("find", "products")
        .with("filter", Doc::new().with("_id", 1));
    let doc = match &c.all("shop", found, "products").await.unwrap()[..] {
        [d] => d.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        doc.get("qty"),
        Some(&Bson::Int32(3)),
        "an integer stays an integer: {doc:?}"
    );
    assert_eq!(doc.get("price"), Some(&Bson::Double(2.5)), "{doc:?}");
    assert_eq!(
        doc.get("stock"),
        Some(&Bson::Int64(5_000_000_000)),
        "{doc:?}"
    );
    let with = WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    };
    b.module.converge(&with).await.unwrap();
    let n = c
        .run("shop", Doc::new().with("count", "products"))
        .await
        .unwrap();
    assert_eq!(
        n.int("n"),
        Some(2),
        "a second converge never doubles the seed"
    );
    graceful(&b.stop(Duration::from_secs(30)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn converge_rebuilds_a_changed_index_and_prune_drops_what_left() {
    let Some(h) = harness() else { return };
    let b = h.boot(SEEDED).await.unwrap();
    let before = b.module.objects(&b.instance).await.unwrap();
    // Now: sku_1 is unique, by_qty and the `old` collection are gone from doze.hcl.
    let second = SEEDED
        .replace(
            "keys = { sku = 1 }\n",
            "keys = { sku = 1 }\n        unique = true\n",
        )
        .replace(
            "      index {\n        name = \"by_qty\"\n        keys = { qty = -1 }\n      }\n",
            "",
        )
        .replace("    collection \"old\" {}\n", "");
    let mut inst = b.instance.clone();
    inst.spec = doze_module_test::hcl::parse_block(&second, &b.module.manifest.schema)
        .unwrap()
        .spec;
    b.module
        .converge(&WithToolchain {
            instance: inst.clone(),
            toolchain: b.toolchain.clone(),
        })
        .await
        .unwrap();
    let after = b.module.objects(&inst).await.unwrap();
    let removed: Vec<_> = before
        .into_iter()
        .filter(|o| !after.iter().any(|a| a.kind == o.kind && a.name == o.name))
        .collect();
    assert_eq!(
        removed.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["shop.products by_qty", "shop.old"]
    );
    b.module
        .prune(&PruneArgs {
            instance: inst,
            toolchain: b.toolchain.clone(),
            removed,
        })
        .await
        .unwrap();
    let mut c = client(&b).await;
    let ix = indexes(&mut c, "shop", "products").await;
    assert_eq!(names(&ix), ["_id_", "sku_1"]);
    let sku = ix.iter().find(|d| d.str("name") == Some("sku_1")).unwrap();
    assert_eq!(
        sku.get("unique"),
        Some(&Bson::Bool(true)),
        "the changed index was rebuilt: {sku:?}"
    );
    assert_eq!(collections(&mut c, "shop").await, ["products"]);
    graceful(&b.stop(Duration::from_secs(30)).await);
}

#[tokio::test(flavor = "current_thread")]
async fn a_sleeping_instance_wakes_with_its_data() {
    let Some(h) = harness() else { return };
    let b = h.boot(SEEDED).await.unwrap();
    let (stops, b) = b.sleep_and_wake(Duration::from_secs(30)).await.unwrap();
    graceful(&stops);
    assert!(!b.fresh);
    let mut c = client(&b).await;
    let n = c
        .run("shop", Doc::new().with("count", "products"))
        .await
        .unwrap();
    assert_eq!(n.int("n"), Some(2));
    graceful(&b.stop(Duration::from_secs(30)).await);
}
