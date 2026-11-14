//! The ferret module (docs/port/modules/PLAN.md step 10): a MongoDB-wire database that is two processes doze runs and
//! hides, a private PostgreSQL 18 with Microsoft's DocumentDB extension (the `documentdb` build, its version pinned
//! here) and the FerretDB v2 gateway in front of it (the `ferretdb` build, the version the block names). Clients reach
//! the gateway through doze's generic splice; the declared databases, collections, indexes, seeds and users are kept
//! over the gateway's socket by a small OP_MSG client (`wire`, `bson`).
//!
//! Better than the Go module, each proven on the engine (`tests/acceptance.rs`):
//! - nothing listens on TCP: the private PostgreSQL listens on its socket only (Go also listened on loopback and
//!   trusted every local TCP client: bug 6), and the gateway's debug handler is off, so no ports are picked at random
//!   and none can collide or be taken between the pick and the bind;
//! - seed documents keep their numbers' kinds: an integer stays an integer (Go stored every number as a double: bug 6);
//! - `auth = true` and `user` blocks are refused at their line: on FerretDB 2.7 the gateway refuses doze's own setup
//!   once auth is on (`createUser requires authentication`), so Go's scaffold failed at boot, and without auth it
//!   ignored the users without a word;
//! - an index whose key is not 1 or -1, an empty index, a seed file that is not there or not an array of documents,
//!   and a setting doze owns are refused at their line, before boot;
//! - PostgreSQL stops with a fast shutdown, so a sleeping instance is not held up by the gateway's pooled connections;
//! - the template cluster is cloned copy-on-write where the disk can (Go copied every byte);
//! - `doze connect` opens `mongosh`, and `doze init` recognises a MongoDB project from its drivers, images and variables.

mod bson;
mod check;
mod converge;
mod json;
mod msg;
mod pg;
mod wire;

use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::hello::{ClientCommand, Detect};
use doze_module::proto::msg::instance::{Address, Connection, Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;
use std::path::Path;

/// The gateway's socket in the instance's socket directory when the host grants none (Go's name).
const SOCKET: &str = "documentdb.sock";

const EXAMPLE: &str = r#"ferret "shop" {
  version = "2.7"

  database "catalog" {
    collection "products" {
      index {
        keys   = { sku = 1 }
        unique = true
      }
    }
    collection "orders" {}
  }
}
"#;

#[derive(Debug, Spec)]
struct FerretSpec {
    #[spec(
        block,
        doc = "A MongoDB database to keep (repeatable; the label is its name)."
    )]
    database: Vec<Database>,
    #[spec(
        doc = "Extra FerretDB settings, each passed as FERRETDB_<KEY> (e.g. log_level = \"debug\")."
    )]
    settings: BTreeMap<String, String>,
    #[spec(
        default = false,
        doc = "Require clients to authenticate (not supported yet: refused)."
    )]
    auth: bool,
    #[spec(
        block,
        doc = "A MongoDB user (needs auth, which is not supported yet: refused)."
    )]
    user: Vec<User>,
}

#[derive(Debug, Spec)]
struct Database {
    #[spec(label)]
    name: String,
    #[spec(
        block,
        doc = "A collection in the database (repeatable; the label is its name)."
    )]
    collection: Vec<Collection>,
}

#[derive(Debug, Spec)]
struct Collection {
    #[spec(label)]
    name: String,
    #[spec(
        doc = "A JSON file holding an array of documents, inserted while the collection is empty."
    )]
    seed: Option<String>,
    #[spec(block, doc = "An index on the collection (repeatable).")]
    index: Vec<Index>,
}

#[derive(Debug, Spec)]
struct Index {
    #[spec(doc = "The index's name; the server makes one from the keys when left out.")]
    name: Option<String>,
    #[spec(doc = "Fields and their order, 1 ascending or -1 descending, e.g. { sku = 1 }.")]
    keys: BTreeMap<String, i64>,
    #[spec(default = false, doc = "Refuse two documents with the same keys.")]
    unique: bool,
}

/// Kept in the schema so a block that has one is told why it is refused (check.rs), not that `user` is unknown.
#[derive(Debug, Spec)]
#[allow(dead_code, reason = "decoded to be refused at its line")]
struct User {
    #[spec(label)]
    name: String,
    #[spec(doc = "Its password.")]
    password: Option<String>,
    #[spec(doc = "The database it authenticates against; \"admin\" when left out.")]
    database: Option<String>,
    #[spec(doc = "Its roles on that database, e.g. [\"readWrite\"].")]
    roles: Vec<String>,
}

struct Ferret;

impl Module for Ferret {
    type Spec = FerretSpec;

    fn about(&self) -> About {
        About {
            engine: "ferret".into(),
            category: "database".into(),
            title: "FerretDB".into(),
            tagline: "MongoDB-compatible database (FerretDB v2 over Postgres)".into(),
            versions: vec!["2".into()],
            exact_dots: 2,
            default_port: 27017,
            // A cold boot without the bundled template runs initdb and CREATE EXTENSION … CASCADE (Go's budget).
            boot_budget: Some(std::time::Duration::from_secs(180)),
            backend_socket: Some(SOCKET.into()),
            example: EXAMPLE.into(),
            // doze-binaries builds documentdb and ferretdb for these; no Windows build (PLAN.md D12).
            targets: ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
                .map(String::from)
                .to_vec(),
            description: Some(
                "A MongoDB-wire database you connect to with any Mongo client. Under the hood it is FerretDB v2 \
                 fronting a private PostgreSQL with Microsoft's DocumentDB extension: no MongoDB server, no licence \
                 worries. Declared databases, collections, indexes and seed data are kept for you."
                    .into(),
            ),
            homepage: Some("https://www.ferretdb.com".into()),
            source: Some("doze/ferret".into()),
            example_label: Some("shop".into()),
            default_version: Some("2.7".into()),
            client: Some(client()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &FerretSpec, instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec, instance)
    }

    fn connection(&self, cx: &Cx<FerretSpec>) -> Result<Connection> {
        let addr = cx
            .instance
            .listeners
            .get("main")
            .map(|l| l.addr.clone())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "localhost".into());
        let url = format!("mongodb://{addr}/");
        Ok(Connection {
            env_var: "MONGODB_URI".into(),
            url,
            env: BTreeMap::new(),
        })
    }

    async fn resolve(&self, cx: &Cx<FerretSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        pg::resolve(cx, pins, target).await
    }

    async fn provision(&self, cx: &Cx<FerretSpec>, tc: &Toolchain) -> Result<Provisioned> {
        pg::provision(&cx.instance, tc).await
    }

    async fn plan(&self, cx: &Cx<FerretSpec>, tc: &Toolchain) -> Result<Plan> {
        let inst = &cx.instance;
        // The running check (postmaster.pid) first: clearing the gateway's socket of a live instance would cut it off.
        let postgres = pg::postgres(inst, tc)?;
        let backend = help::prepare_backend(inst, SOCKET)?;
        let mut env = BTreeMap::from([
            ("FERRETDB_POSTGRESQL_URL".to_string(), pg::url(inst)),
            ("FERRETDB_DEBUG_ADDR".into(), String::new()),
            ("FERRETDB_TELEMETRY".into(), "disable".into()),
            // `auth = true` is refused at decode (check.rs).
            ("FERRETDB_AUTH".into(), "false".into()),
        ]);
        let state = Path::new(&inst.data_dir).join("ferretdb");
        doze_os::fs::private_dir(&state).map_err(help::io_error("creating", &state))?;
        env.insert(
            "FERRETDB_STATE_DIR".into(),
            state.to_string_lossy().into_owned(),
        );
        let ready = match &backend {
            Address::Unix(p) => {
                env.insert("FERRETDB_LISTEN_UNIX".into(), p.clone());
                env.insert("FERRETDB_LISTEN_ADDR".into(), String::new());
                Ready::Socket(p.clone())
            }
            Address::Tcp(a) => {
                env.insert("FERRETDB_LISTEN_ADDR".into(), a.clone());
                Ready::Tcp(a.clone())
            }
        };
        for (k, v) in help::settings(&cx.spec.settings, check::OWNED) {
            env.insert(
                format!("FERRETDB_{}", k.to_ascii_uppercase()),
                v.to_string(),
            );
        }
        let ferretdb = Process {
            name: "ferretdb".into(),
            program: pg::gateway(tc),
            args: vec![],
            env,
            dir: None,
            after: vec!["postgres".into()],
            ready: Some(ready),
            hooks: vec![],
            stop: Some(StopHow::Terminate),
        };
        Ok(Plan {
            processes: vec![postgres, ferretdb],
        })
    }
}

/// `mongosh` through doze's address, so it wakes the instance; the legacy `mongo` shell when that is all there is.
fn client() -> ClientCommand {
    ClientCommand {
        program: "mongosh".into(),
        args: vec!["{url}".into()],
        env: BTreeMap::new(),
        fallbacks: vec!["mongo".into()],
    }
}

/// MongoDB drivers and ODMs, the images, and the variables projects use for them.
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            (
                "go".into(),
                list(&[
                    "go.mongodb.org/mongo-driver",
                    "go.mongodb.org/mongo-driver/v2",
                ]),
            ),
            ("npm".into(), list(&["mongodb", "mongoose"])),
            ("pypi".into(), list(&["pymongo", "motor", "mongoengine"])),
        ]),
        images: list(&["mongo", "ferretdb/ferretdb"]),
        env: [
            ("MONGODB_URI", "url"),
            ("MONGO_URL", "url"),
            ("MONGODB_URL", "url"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into(),
    }
}

fn main() -> std::process::ExitCode {
    serve(Ferret)
        .release(env!("CARGO_PKG_VERSION"))
        .converge()
        .objects()
        .prune()
        .run()
}
