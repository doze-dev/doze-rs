//! The kvrocks module (docs/port/modules/PLAN.md step 7): one kvrocks server reading a config file doze writes on
//! every boot, gated on its backend accepting connections. Like valkey it rides the core's generic accept, boot, splice
//! path.
//!
//! Departures from the Go module, each deliberate: the server listens where the host granted (D13); the password is
//! escaped in the URL (module bug 7); a value with control characters is refused, as a newline in it would add a line
//! of its own to the config file; the published example quotes its dotted setting so it decodes without the core.

mod conf;

use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::{Connection, Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Resolved, StopHow};
use doze_module::proto::msg::value::PathSeg;
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;
use std::path::Path;

/// The socket's name in the instance's socket directory when the host grants none.
const SOCKET: &str = "kvrocks.sock";

const EXAMPLE: &str = r#"kvrocks "store" {
  version  = 2
  port     = 6380
  workers  = 4
  password = "admin"
  settings = {
    "rocksdb.block_cache_size" = "256"
  }
  namespace "tenant_a" {
    token = "tenant-a-token"
  }
  namespace "tenant_b" {
    token = "tenant-b-token"
  }
}
"#;

#[derive(Debug, Spec)]
struct KvrocksSpec {
    #[spec(doc = "Number of worker threads.")]
    workers: Option<u32>,
    #[spec(
        doc = "requirepass auth password (the admin namespace); required when namespaces are declared."
    )]
    password: Option<String>,
    #[spec(doc = "Arbitrary kvrocks.conf directives, applied verbatim.")]
    settings: BTreeMap<String, String>,
    #[spec(block, doc = "A logical namespace with its own access token.")]
    namespace: Vec<Namespace>,
}

#[derive(Debug, Spec)]
struct Namespace {
    #[spec(label)]
    name: String,
    #[spec(doc = "Auth token scoped to this namespace.")]
    token: String,
}

impl KvrocksSpec {
    fn password(&self) -> Option<&str> {
        self.password.as_deref().filter(|p| !p.is_empty())
    }
}

struct Kvrocks;

impl Module for Kvrocks {
    type Spec = KvrocksSpec;

    fn about(&self) -> About {
        About {
            engine: "kvrocks".into(),
            category: "database".into(),
            title: "Kvrocks".into(),
            tagline: "Redis API on RocksDB — durable, low-RAM.".into(),
            versions: vec!["2".into()],
            exact_dots: 2,
            default_port: 6380,
            backend_socket: Some(SOCKET.into()),
            example: EXAMPLE.into(),
            // doze-binaries builds kvrocks for these; there is no Windows kvrocks (PLAN.md D12).
            targets: [
                "aarch64-apple-darwin",
                "aarch64-unknown-linux-gnu",
                "x86_64-unknown-linux-gnu",
            ]
            .map(String::from)
            .to_vec(),
            description: Some(
                "A local Apache Kvrocks server: the Redis protocol backed by RocksDB on disk. Durable and \
                 memory-frugal — a drop-in Redis API when you want persistence without holding the dataset in RAM. \
                 Supports namespaces with per-namespace tokens."
                    .into(),
            ),
            homepage: Some("https://github.com/doze-dev/doze-rs/tree/main/crates/doze-module-kvrocks".into()),
            source: Some("doze/kvrocks".into()),
            example_label: Some("store".into()),
            ..About::default()
        }
    }

    fn check(&self, spec: &KvrocksSpec, _instance: &Instance) -> Vec<Diagnostic> {
        let mut out = conf::unwritable(spec);
        if !spec.namespace.is_empty() && spec.password().is_none() {
            out.push(Diagnostic {
                path: vec![PathSeg::Key("password".into())],
                message: "kvrocks namespaces require a `password` (the default-namespace token)"
                    .into(),
                hint: None,
            });
        }
        out
    }

    fn connection(&self, cx: &Cx<KvrocksSpec>) -> Result<Connection> {
        let addr = cx
            .instance
            .listeners
            .get("main")
            .map(|l| l.addr.clone())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "localhost".into());
        Ok(Connection {
            env_var: "REDIS_URL".into(),
            url: doze_resp::redis_url(cx.spec.password(), &addr),
            env: BTreeMap::new(),
        })
    }

    async fn resolve(&self, cx: &Cx<KvrocksSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        help::resolve_via(cx, pins, target, 2).await
    }

    async fn provision(&self, cx: &Cx<KvrocksSpec>, _tc: &Toolchain) -> Result<Provisioned> {
        help::empty_data_dir(&cx.instance.data_dir)
    }

    async fn plan(&self, cx: &Cx<KvrocksSpec>, tc: &Toolchain) -> Result<Plan> {
        let data = Path::new(&cx.instance.data_dir);
        let backend = help::prepare_backend(&cx.instance, SOCKET)?;
        let (text, ready) = conf::render(&cx.instance.data_dir, &backend, &cx.spec)?;
        let path = data.join("kvrocks.conf");
        doze_os::fs::private_dir(data).map_err(help::io_error("creating", data))?;
        doze_os::fs::write_private(&path, text.as_bytes())
            .map_err(help::io_error("writing", &path))?;
        let program =
            Path::new(&tc.bin_dir).join(format!("kvrocks{}", std::env::consts::EXE_SUFFIX));
        Ok(Plan {
            processes: vec![Process {
                name: cx.instance.name.clone(),
                program: program.to_string_lossy().into_owned(),
                args: vec!["-c".into(), path.to_string_lossy().into_owned()],
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                ready: Some(ready),
                hooks: vec![],
                stop: Some(StopHow::Terminate),
            }],
        })
    }
}

fn main() -> std::process::ExitCode {
    serve(Kvrocks).release(env!("CARGO_PKG_VERSION")).run()
}
