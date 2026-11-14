//! The postgres module (docs/port/modules/PLAN.md step 11, core plan M1): one PostgreSQL server per instance on its unix
//! socket, its cluster made once, and its roles, database, schemas, extensions and grants kept as declared.
//!
//! Better than the Go module, each proven on the engine (`tests/acceptance.rs`) or against Go's answers
//! (`tests/oracle.rs`):
//! - converge keeps the declared state (bug 10): memberships, passwords, expiry, role settings, comments, database
//!   options and grants removed or narrowed are taken back; a role that owns objects can be pruned; grants are objects;
//! - `autovacuum` is on unless declared off, as the description always said (bug 1: the code had it off);
//! - settings are one rule without case (bug 11): a setting doze owns, two names PostgreSQL reads as one, a value that
//!   would break doze.conf, a privilege the target does not take, an unknown extension (with did-you-mean), a reserved
//!   role and a second grant on one target are refused at their line, not at boot or converge;
//! - the description and the decoder are one type, so `grant` is unlabelled in both (Go described a `role` label);
//! - SQL goes to psql on its standard input, so no password reaches a command line, and no `PG*` variable or psqlrc of
//!   the machine's changes where it goes; a database closed to connections is closed after converge, not before;
//! - the cluster is made beside its folder and renamed in, and the server stops with a fast shutdown (Go's default,
//!   smart, waited for every client to leave).

mod admin;
mod check;
mod cluster;
mod conf;
mod converge;
mod ext;
mod filter;
mod names;
mod prune;
mod psql;
mod spec;
mod sql;
mod wire;

use doze_module::caps::{BackendUrl, Wire, WireHost};
use doze_module::proto::msg::console::WireAccept;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::hello::{ClientCommand, Detect};
use doze_module::proto::msg::instance::{Connection, Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::{About, Cx, Module, Result, help, serve};
use spec::PostgresSpec;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The port that names the socket (`.s.PGSQL.5432`) when the host grants none; nothing listens on TCP.
pub(crate) const PORT: &str = "5432";

const EXAMPLE: &str = r#"postgres "app" {
  version          = 18
  owner            = "app"
  encoding         = "UTF8"
  locale           = "en_US.UTF-8"
  connection_limit = 50
  comment          = "primary app database"
  shared_buffers   = "256MB"
  max_connections  = 100

  settings = {
    log_min_duration_statement = "200ms"
  }

  role "app" {
    password         = "app"
    login            = true
    createdb         = true
    connection_limit = 20
    member_of        = ["readers"]
  }

  role "readers" {
    login = false
  }

  schema "analytics" {
    owner = "app"
  }

  extension "pg_trgm" {}

  grant {
    role       = "app"
    privileges = ["ALL"]
    schema     = "analytics"
    objects    = "tables"
  }
}
"#;

#[derive(Default)]
struct Postgres {
    /// The synthetic cancel keys of the sessions the wire filter splices.
    cancels: std::sync::Arc<filter::Cancels>,
}

/// One of the engine's programs in its toolchain.
pub(crate) fn tool(tc: &Toolchain, name: &str) -> PathBuf {
    match tc.tools.get(name) {
        Some(p) => PathBuf::from(p),
        None => Path::new(&tc.bin_dir).join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
    }
}

/// An exact version as the mirror names it: "16.14" is "16.14.0".
fn exact(v: &str) -> String {
    if v.matches('.').count() == 1 {
        format!("{v}.0")
    } else {
        v.to_string()
    }
}

impl Module for Postgres {
    type Spec = PostgresSpec;

    fn about(&self) -> About {
        About {
            engine: "postgres".into(),
            category: "database".into(),
            title: "PostgreSQL".into(),
            tagline: "Real local Postgres, declared not scripted.".into(),
            versions: ["14", "15", "16", "17", "18"].map(String::from).to_vec(),
            exact_dots: 1,
            default_port: 5432,
            backend_socket: Some(".s.PGSQL.{port}".into()),
            example: EXAMPLE.into(),
            console: Some(admin::console()),
            actions: admin::actions(),
            // doze-binaries builds PostgreSQL for these; no Windows build yet (PLAN.md D12).
            targets: ["aarch64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
                .map(String::from)
                .to_vec(),
            description: Some(
                "A real PostgreSQL server per instance — no Docker. Declare roles, schemas, extensions and grants in HCL \
                 and doze converges them: creating what's new, updating what changed, dropping what you removed. Boots \
                 on first connect, reaps when idle."
                    .into(),
            ),
            homepage: Some("https://www.postgresql.org".into()),
            source: Some("doze/postgres".into()),
            example_label: Some("app".into()),
            default_version: Some("18".into()),
            client: Some(client()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &PostgresSpec, instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec, instance)
    }

    fn connection(&self, cx: &Cx<PostgresSpec>) -> Result<Connection> {
        let name = &cx.instance.name;
        let url = match cx
            .instance
            .listeners
            .get("main")
            .map(|l| l.addr.as_str())
            .filter(|a| !a.is_empty())
        {
            Some(addr) => format!("postgres://postgres@{addr}/{name}?sslmode=disable"),
            None => format!(
                "postgres://postgres@/{name}?host={}",
                psql::backend(&cx.instance)?.0
            ),
        };
        Ok(Connection {
            env_var: "DATABASE_URL".into(),
            url,
            env: BTreeMap::new(),
        })
    }

    /// `DOZE_POSTGRES_BINDIR` (a developer's own build), else the lock's pin, else an exact version, else the major
    /// through the host. Extensions add nothing here until the catalog's bundles are fetched (step 13).
    async fn resolve(&self, cx: &Cx<PostgresSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        let spec = cx.instance.version.clone();
        let normal = if help::is_exact(&spec, 1) {
            exact(&spec)
        } else {
            spec.clone()
        };
        if let Some(dir) = std::env::var_os("DOZE_POSTGRES_BINDIR").filter(|d| !d.is_empty()) {
            let toolchain = Toolchain {
                engine: "postgres".into(),
                full: normal,
                bin_dir: dir.to_string_lossy().into_owned(),
                tools: BTreeMap::new(),
            };
            return Ok(Resolved {
                toolchain,
                pins: vec![],
            });
        }
        let pin = pins
            .iter()
            .find(|p| p.engine == "postgres" && p.spec == spec);
        let full = match pin {
            Some(p) => p.resolved.clone(),
            None if help::is_exact(&spec, 1) => normal,
            None => cx.fetch().resolve_major("postgres", &spec).await?,
        };
        let expected = pin.and_then(|p| p.hashes.get(target)).map(String::as_str);
        let got = cx
            .fetch()
            .ensure("postgres", &full, target, expected)
            .await?;
        let pin = Pin {
            engine: "postgres".into(),
            spec,
            resolved: full.clone(),
            hashes: BTreeMap::from([(target.to_string(), got.digest)]),
        };
        let toolchain = Toolchain {
            engine: "postgres".into(),
            full,
            bin_dir: got.bin_dir,
            tools: BTreeMap::new(),
        };
        Ok(Resolved {
            toolchain,
            pins: vec![pin],
        })
    }

    async fn provision(&self, cx: &Cx<PostgresSpec>, tc: &Toolchain) -> Result<Provisioned> {
        cluster::provision(&cx.instance, tc, &cx.spec).await
    }

    async fn plan(&self, cx: &Cx<PostgresSpec>, tc: &Toolchain) -> Result<Plan> {
        let inst = &cx.instance;
        let (dir, port) = psql::backend(inst)?;
        let sockets = Path::new(&dir);
        doze_os::fs::private_dir(sockets)
            .map_err(help::io_error("creating socket dir", sockets))?;
        cluster::stale(inst, &sockets.join(format!(".s.PGSQL.{port}")))?;
        let path = |p: PathBuf| p.to_string_lossy().into_owned();
        let args = [
            "-D",
            &inst.data_dir,
            "-k",
            &dir,
            "-p",
            &port,
            "-c",
            "listen_addresses=",
        ]
        .map(String::from)
        .to_vec();
        let mut ready = vec![path(tool(tc, "pg_isready"))];
        ready.extend(
            ["-h", &dir, "-p", &port, "-d", "postgres", "-U", "postgres"].map(String::from),
        );
        Ok(Plan {
            processes: vec![Process {
                name: "postgres".into(),
                program: path(tool(tc, "postgres")),
                args,
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                ready: Some(Ready::Exec(ready)),
                hooks: vec![],
                // A fast shutdown: open sessions are ended, not waited for; the data is checkpointed.
                stop: Some(StopHow::Interrupt),
            }],
        })
    }
}

impl BackendUrl for Postgres {
    fn backend_url(&self, cx: &Cx<PostgresSpec>) -> Result<String> {
        let (dir, port) = psql::backend(&cx.instance)?;
        Ok(format!(
            "postgres://postgres@/{}?host={dir}&port={port}",
            cx.instance.name
        ))
    }
}

impl Wire for Postgres {
    async fn accept(
        &self,
        conn: tokio::net::TcpStream,
        req: &WireAccept,
        host: &WireHost,
    ) -> Result<()> {
        filter::accept(&self.cancels, conn, req, host).await
    }
}

/// The engine's own `psql` through doze's address (so it wakes the instance), on the instance's database.
fn client() -> ClientCommand {
    ClientCommand {
        program: "psql".into(),
        args: vec!["{url}".into()],
        env: BTreeMap::new(),
        fallbacks: vec![],
    }
}

/// PostgreSQL drivers, images and the variables projects use for them.
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            (
                "go".into(),
                list(&["github.com/jackc/pgx", "github.com/lib/pq"]),
            ),
            ("npm".into(), list(&["pg", "postgres", "pg-promise"])),
            (
                "pypi".into(),
                list(&["psycopg", "psycopg2", "psycopg2-binary", "asyncpg"]),
            ),
            ("cargo".into(), list(&["tokio-postgres", "postgres"])),
        ]),
        images: list(&["postgres", "postgis/postgis", "pgvector/pgvector"]),
        env: [
            ("POSTGRES_URL", "url"),
            ("POSTGRESQL_URL", "url"),
            ("PGHOST", "address"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into(),
    }
}

fn main() -> std::process::ExitCode {
    serve(Postgres::default())
        .release(env!("CARGO_PKG_VERSION"))
        .converge()
        .objects()
        .prune()
        .template()
        .admin()
        .backend_url()
        .wire()
        .run()
}
