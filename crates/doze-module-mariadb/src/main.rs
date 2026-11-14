//! The mariadb module (docs/port/modules/PLAN.md step 9): one mariadbd, its system tables made once
//! (`mariadb-install-db`, or a clone of the shared template), its database, users and grants kept as declared.
//! MariaDB greets first, so the core's generic accept, boot, splice path serves it with no wire filter.
//!
//! Better than the Go module, each proven on the engine (`tests/acceptance.rs`):
//! - privileges, database, table and character set names are checked before boot, at their line, against MariaDB's
//!   own lists (Go interpolated them into SQL unvalidated: bug 8); a setting doze owns is refused at its line instead
//!   of silently dropped;
//! - converge keeps the declared state: it grants first and then revokes what is no longer declared (GRANT OPTION
//!   included), so a refused GRANT never takes privileges away; a grant removed from doze.hcl is revoked; a password
//!   removed is cleared; a changed character set is applied to the existing database (Go only ever added);
//! - privileges MariaDB refuses on the grant's level, `ALL PRIVILEGES` mixed with others, two grants on one target,
//!   and settings that would break my.cnf are refused at their line;
//! - the readiness ping and every client the module runs read no option file of the machine's (`--no-defaults`);
//! - the server listens where the host grants (D13): its socket on Unix, loopback TCP where the host asks;
//! - `doze connect` opens the engine's own `mariadb` client, the password in its environment, never on its command
//!   line; `doze init` recognises a MySQL or MariaDB project from its packages, images and variables.

mod check;
mod converge;
mod grants;
mod sql;

use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::hello::{ClientCommand, Detect};
use doze_module::proto::msg::instance::{Address, Connection, Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;
use std::path::Path;

/// The socket's name in the instance's socket directory when the host grants none (Go's name).
const SOCKET: &str = "mysqld.sock";
/// doze's own my.cnf in the data directory, the only option file mariadbd reads.
const CONF: &str = "doze.cnf";

const EXAMPLE: &str = r#"mariadb "app" {
  version       = "11.4"
  character_set = "utf8mb4"

  user "app" {
    password = "secret"
    host     = "%"
  }
  grant {
    user       = "app"
    privileges = ["ALL PRIVILEGES"]
    database   = "app"
  }
}
"#;

#[derive(Debug, Spec)]
struct MariadbSpec {
    #[spec(doc = "Default character set of the instance database, e.g. \"utf8mb4\".")]
    character_set: Option<String>,
    #[spec(doc = "Default collation of the instance database, e.g. \"utf8mb4_unicode_ci\".")]
    collation: Option<String>,
    #[spec(doc = "Extra [mysqld] my.cnf entries, applied verbatim.")]
    settings: BTreeMap<String, String>,
    #[spec(block, doc = "A MariaDB account (repeatable; the label is its name).")]
    user: Vec<User>,
    #[spec(
        block,
        doc = "Privileges for an account on a database, or one table of it."
    )]
    grant: Vec<Grant>,
}

#[derive(Debug, Spec)]
struct User {
    #[spec(label)]
    name: String,
    // An account is a name and a host: `user "app"` for two hosts is two accounts.
    #[spec(key, doc = "The host the account is for; \"%\" (any) when left out.")]
    host: Option<String>,
    #[spec(doc = "Its password; none, the account has no password.")]
    password: Option<String>,
}

#[derive(Debug, Spec)]
struct Grant {
    #[spec(doc = "The account's name (a `user` block, or one that exists already).")]
    user: String,
    #[spec(doc = "The account's host; \"%\" when left out.")]
    host: Option<String>,
    #[spec(doc = "Privileges, e.g. [\"SELECT\", \"INSERT\"] or [\"ALL PRIVILEGES\"].")]
    privileges: Vec<String>,
    #[spec(doc = "The database; every database (\"*\") when left out.")]
    database: Option<String>,
    #[spec(doc = "One table of it; every table when left out.")]
    table: Option<String>,
}

/// A host left out is any host, as Go and MariaDB read it.
fn host(h: &Option<String>) -> &str {
    h.as_deref().filter(|h| !h.is_empty()).unwrap_or("%")
}

/// `*` (every one) when left out or empty.
fn every(v: &Option<String>) -> &str {
    v.as_deref().filter(|v| !v.is_empty()).unwrap_or("*")
}

struct Mariadb;

impl Module for Mariadb {
    type Spec = MariadbSpec;

    fn about(&self) -> About {
        About {
            engine: "mariadb".into(),
            category: "database".into(),
            title: "MariaDB".into(),
            tagline: "MySQL-compatible relational database".into(),
            versions: vec!["11.4".into(), "11.8".into(), "12.2".into()],
            exact_dots: 2,
            default_port: 3306,
            backend_socket: Some(SOCKET.into()),
            example: EXAMPLE.into(),
            // doze-binaries builds mariadb for these; no Windows build yet (PLAN.md D12).
            targets: [
                "aarch64-apple-darwin",
                "aarch64-unknown-linux-gnu",
                "x86_64-unknown-linux-gnu",
            ]
            .map(String::from)
            .to_vec(),
            description: Some(
                "A local MariaDB server behind doze, with declared databases, users and grants kept as declared. The \
                 instance database is created for you; connect with DATABASE_URL."
                    .into(),
            ),
            homepage: Some("https://mariadb.org".into()),
            source: Some("doze/mariadb".into()),
            example_label: Some("app".into()),
            default_version: Some("11.4".into()),
            client: Some(client()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &MariadbSpec, _instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec)
    }

    fn connection(&self, cx: &Cx<MariadbSpec>) -> Result<Connection> {
        let addr = cx
            .instance
            .listeners
            .get("main")
            .map(|l| l.addr.clone())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "localhost".into());
        Ok(Connection {
            env_var: "DATABASE_URL".into(),
            url: format!("mysql://root@{addr}/{}", cx.instance.name),
            env: BTreeMap::new(),
        })
    }

    async fn resolve(&self, cx: &Cx<MariadbSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        help::resolve_via(cx, pins, target, 2).await
    }

    async fn provision(&self, cx: &Cx<MariadbSpec>, tc: &Toolchain) -> Result<Provisioned> {
        let data = Path::new(&cx.instance.data_dir);
        let made = if converge::provisioned(data) {
            Provisioned::Existing
        } else {
            converge::install_db(tc, data).await?;
            Provisioned::Fresh
        };
        let conf = data.join(CONF);
        doze_os::fs::write_private(&conf, sql::conf(&cx.spec).as_bytes())
            .map_err(help::io_error("writing", &conf))?;
        Ok(made)
    }

    async fn plan(&self, cx: &Cx<MariadbSpec>, tc: &Toolchain) -> Result<Plan> {
        let data = Path::new(&cx.instance.data_dir);
        let pid = data.join("mariadbd.pid");
        stale_pid(&pid, &cx.instance.name)?;
        let backend = help::prepare_backend(&cx.instance, SOCKET)?;
        // On TCP the server still makes its socket, in the instance's folder: the folder must be there.
        let sockets = Path::new(&cx.instance.socket_dir);
        doze_os::fs::private_dir(sockets).map_err(help::io_error("creating", sockets))?;
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let mut args = vec![
            // First: mariadbd then reads only doze's my.cnf, never the machine's.
            format!("--defaults-file={}", path(&data.join(CONF))),
            format!("--datadir={}", path(data)),
        ];
        args.extend(sql::listen_args(&backend, &cx.instance.socket_dir)?);
        args.push(format!("--pid-file={}", path(&pid)));
        if let Address::Unix(_) = backend {
            args.push("--skip-networking".into());
        }
        // --no-defaults: the machine's ~/.my.cnf must not send the ping to another server.
        let mut ready = vec![path(&tool(tc, "mariadb-admin")), "--no-defaults".into()];
        ready.extend(sql::client_args(&backend)?);
        ready.extend(["--user=root".into(), "ping".into()]);
        Ok(Plan {
            processes: vec![Process {
                name: "mariadb".into(),
                program: path(&tool(tc, "mariadbd")),
                args,
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                ready: Some(Ready::Exec(ready)),
                hooks: vec![],
                stop: Some(StopHow::Terminate),
            }],
        })
    }
}

/// A pid file left by a server: refused while its process lives (Go's `ClearStaleLock`: a second server would wait on
/// InnoDB's lock and fail, and the first could no longer be reached), removed when it is gone.
fn stale_pid(pid: &Path, name: &str) -> Result<()> {
    let live = std::fs::read_to_string(pid)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        // Another user's process with that number (a reboot reused it) is not the server that wrote the file.
        .filter(|p| doze_os::process::alive_own(*p));
    if let Some(p) = live {
        return Err(doze_module::proto::msg::error::Error::new(
            doze_module::proto::msg::error::ErrorKind::Unavailable,
            format!(
                "instance {name:?} appears to already be running (pid {p}); remove {} if you are sure it is not",
                pid.display()
            ),
        ));
    }
    help::clear_stale(pid)
}

/// One of the engine's programs in its toolchain.
fn tool(tc: &Toolchain, name: &str) -> std::path::PathBuf {
    Path::new(&tc.bin_dir).join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// The engine's own `mariadb` client through doze's address (so it wakes the instance), as root, with no
/// database chosen (`USE app` picks one); a password goes in the client's own variable, never on its command line.
fn client() -> ClientCommand {
    ClientCommand {
        program: "mariadb".into(),
        args: [
            "--host={host}",
            "--port={port}",
            "--protocol=TCP",
            "--user=root",
        ]
        .map(String::from)
        .to_vec(),
        env: BTreeMap::from([("MYSQL_PWD".into(), "{password}".into())]),
        fallbacks: vec!["mysql".into()],
    }
}

/// MySQL and MariaDB drivers, images and the variables projects use for them.
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            ("go".into(), list(&["github.com/go-sql-driver/mysql"])),
            ("npm".into(), list(&["mysql", "mysql2", "mariadb"])),
            ("pypi".into(), list(&["mysqlclient", "pymysql", "mariadb"])),
        ]),
        images: list(&["mariadb", "mysql"]),
        env: [("MYSQL_URL", "url"), ("MARIADB_URL", "url")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into(),
    }
}

fn main() -> std::process::ExitCode {
    serve(Mariadb)
        .release(env!("CARGO_PKG_VERSION"))
        .converge()
        .objects()
        .prune()
        .template()
        .run()
}
