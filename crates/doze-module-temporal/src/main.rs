//! The temporal module (docs/port/modules/PLAN.md step 8): the temporal CLI's dev server (the Temporal services, a
//! SQLite store and the Web UI in one pure-Go binary), supervised and always on (workers long-poll it), with the
//! declared namespaces converged.
//!
//! Better than the Go module (D16), each tested on the real server:
//! - two projects can both run temporal: it listens where the host grants (D13: `Listener::tcp`, it has no socket
//!   mode), not on 7233 and 8233 itself, and its Web UI gets a name of its own (`ui-<name>`);
//! - declared namespaces exist the moment it is ready (`start-dev --namespace`), and converge changes only what
//!   differs; only "is not found" means a namespace is absent (module bug 9), and the CLI is called with `-n` (the
//!   positional form Go used is deprecated);
//! - retention in days (`7d`), custom search attributes and dynamic config, each checked at its line ([`check`]);
//! - ready means temporal says SERVING, not that a port opened; a health check for the supervisor;
//! - the app's console opens the Web UI and lists namespaces with their running workflows ([`admin`]);
//! - `version = 1` means the newest 1.x (owner, 2026-11-13); and it ships for Windows too.

mod admin;
mod caps;
mod check;
mod cli;
mod duration;

use doze_module::proto::msg::error::{Diagnostic, Error, ErrorKind};
use doze_module::proto::msg::hello::Detect;
use doze_module::proto::msg::instance::{Address, Connection, Instance, Listener, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;
use std::path::Path;

const EXAMPLE: &str = r#"temporal "dev" {
  version = 1
  port    = 7233
  ui_port = 8233

  search_attributes = {
    CustomerId = "Keyword"
  }

  namespace "orders" {
    retention = "7d"
  }
  namespace "billing" {}
}
"#;

#[derive(Debug, Spec)]
struct TemporalSpec {
    #[spec(default = 8233, doc = "The Web UI's port.")]
    ui_port: u16,
    #[spec(default = false, doc = "Run without the Web UI.")]
    headless: bool,
    #[spec(
        doc = "Custom search attributes to register, name = type (Text, Keyword, Int, Double, Bool, Datetime, \
                  KeywordList)."
    )]
    search_attributes: BTreeMap<String, String>,
    #[spec(
        doc = "Dev-server dynamic config, key = value (the value is read as YAML: true, 10, \"text\")."
    )]
    dynamic_config: BTreeMap<String, String>,
    #[spec(block, doc = "A namespace to create and keep as declared.")]
    namespace: Vec<Namespace>,
    #[spec(
        block,
        doc = "How the supervisor restarts the server when it exits (at most one)."
    )]
    restart: Vec<RestartBlock>,
}

#[derive(Debug, Spec)]
struct Namespace {
    #[spec(label)]
    name: String,
    #[spec(doc = "How long closed workflows are kept, e.g. \"7d\" or \"168h\".")]
    retention: Option<String>,
    #[spec(doc = "What the namespace is for.")]
    description: Option<String>,
}

#[derive(Debug, Spec)]
struct RestartBlock {
    #[spec(doc = "no, on_failure (the default) or always.")]
    policy: Option<String>,
    #[spec(doc = "The wait before a restart, e.g. \"1s\" (the default).")]
    backoff: Option<String>,
    #[spec(doc = "Restarts before giving up (5 by default).")]
    max_retries: Option<u32>,
}

/// The listener the Web UI is served on.
const UI: &str = "ui";

/// The engine's own CLI in the toolchain.
fn program(bin_dir: &str) -> std::path::PathBuf {
    Path::new(bin_dir).join(format!("temporal{}", std::env::consts::EXE_SUFFIX))
}

/// A granted backend's `host:port`: temporal listens on TCP only, and the host was told so.
fn tcp_backend(instance: &Instance, listener: &str) -> Result<String> {
    match instance.listeners.get(listener).map(|g| &g.backend) {
        Some(Address::Tcp(a)) => Ok(a.clone()),
        Some(Address::Unix(_)) => Err(Error::new(
            ErrorKind::Unsupported,
            format!(
                "temporal cannot listen on a socket; listener {listener:?} needs a TCP backend"
            ),
        )),
        None => Err(Error::new(
            ErrorKind::Unavailable,
            format!("no backend granted for {listener:?}"),
        )),
    }
}

fn first_namespace(spec: &TemporalSpec) -> String {
    spec.namespace
        .first()
        .map_or_else(|| "default".to_string(), |n| n.name.clone())
}

struct Temporal;

impl Module for Temporal {
    type Spec = TemporalSpec;

    fn about(&self) -> About {
        let targets = [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        About {
            engine: "temporal".into(),
            category: "workflow".into(),
            title: "Temporal".into(),
            tagline: "Durable workflow engine (local dev server)".into(),
            versions: vec!["1".into()],
            exact_dots: 2,
            default_port: 7233,
            supervised: true,
            example: EXAMPLE.into(),
            console: Some(admin::console()),
            targets: targets.map(String::from).to_vec(),
            description: Some(
                "A local Temporal dev server: a single pure-Go binary bundling the Temporal services, a SQLite store \
                 and the Web UI, with no JVM or Docker. Supervised and always on (workers long-poll it); connect via \
                 TEMPORAL_ADDRESS. Declared namespaces exist the moment it is ready."
                    .into(),
            ),
            homepage: Some("https://temporal.io".into()),
            source: Some("doze/temporal".into()),
            example_label: Some("dev".into()),
            default_version: Some("1".into()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &TemporalSpec, _instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec)
    }

    fn network(&self, cx: &Cx<TemporalSpec>) -> Vec<Listener> {
        let mut out = vec![Listener {
            name: "main".into(),
            port: 0,
            http: false,
            routes: vec![],
            tcp: true,
        }];
        if !cx.spec.headless {
            let ui = Listener {
                name: UI.into(),
                port: cx.spec.ui_port,
                http: true,
                routes: vec!["ui-@".into()],
                tcp: true,
            };
            out.push(ui);
        }
        out
    }

    fn connection(&self, cx: &Cx<TemporalSpec>) -> Result<Connection> {
        let addr = cx
            .instance
            .listeners
            .get("main")
            .map(|l| l.addr.clone())
            .unwrap_or_default();
        Ok(Connection {
            env_var: "TEMPORAL_ADDRESS".into(),
            url: addr,
            // The SDKs and the CLI read both; the namespace an app most likely wants is the first declared.
            env: BTreeMap::from([("TEMPORAL_NAMESPACE".to_string(), first_namespace(&cx.spec))]),
        })
    }

    async fn resolve(&self, cx: &Cx<TemporalSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        help::resolve_via(cx, pins, target, 2).await
    }

    async fn provision(&self, cx: &Cx<TemporalSpec>, _tc: &Toolchain) -> Result<Provisioned> {
        help::empty_data_dir(&cx.instance.data_dir)
    }

    async fn plan(&self, cx: &Cx<TemporalSpec>, tc: &Toolchain) -> Result<Plan> {
        let s = &cx.spec;
        let main = tcp_backend(&cx.instance, "main")?;
        let (host, port) = help::host_port(&main)?;
        let mut args: Vec<String> = ["server", "start-dev", "--ip", host, "--port", port]
            .map(String::from)
            .to_vec();
        if !s.headless {
            let ui = tcp_backend(&cx.instance, UI)?;
            let (ui_host, ui_port) = help::host_port(&ui)?;
            if ui_host != host {
                args.extend(["--ui-ip".into(), ui_host.into()]);
            }
            args.extend(["--ui-port".into(), ui_port.into()]);
        }
        let db = Path::new(&cx.instance.data_dir).join("temporal.db");
        args.extend(["--db-filename".into(), db.to_string_lossy().into_owned()]);
        if s.headless {
            args.push("--headless".into());
        }
        for ns in &s.namespace {
            args.extend(["--namespace".into(), ns.name.clone()]);
        }
        for (k, v) in &s.search_attributes {
            let ty = check::canonical_type(v).unwrap_or(v);
            args.extend(["--search-attribute".into(), format!("{k}={ty}")]);
        }
        for (k, v) in &s.dynamic_config {
            args.extend(["--dynamic-config-value".into(), format!("{k}={v}")]);
        }
        let program = program(&tc.bin_dir).to_string_lossy().into_owned();
        let health = [
            program.as_str(),
            "operator",
            "cluster",
            "health",
            "--address",
            main.as_str(),
        ];
        Ok(Plan {
            processes: vec![Process {
                name: cx.instance.name.clone(),
                program: program.clone(),
                args,
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                ready: Some(Ready::Exec(health.map(String::from).to_vec())),
                hooks: vec![],
                stop: Some(StopHow::Terminate),
            }],
        })
    }
}

/// Temporal's SDKs and images, and the variable they read.
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            ("go".into(), list(&["go.temporal.io/sdk"])),
            (
                "npm".into(),
                list(&["@temporalio/client", "@temporalio/worker"]),
            ),
            ("pypi".into(), list(&["temporalio"])),
        ]),
        images: list(&[
            "temporalio/auto-setup",
            "temporalio/server",
            "temporalio/temporal",
        ]),
        env: BTreeMap::from([("TEMPORAL_ADDRESS".into(), "address".into())]),
    }
}

fn main() -> std::process::ExitCode {
    serve(Temporal)
        .release(env!("CARGO_PKG_VERSION"))
        .objects()
        .converge()
        .prune()
        .attributes()
        .health()
        .restarts()
        .admin()
        .run()
}
