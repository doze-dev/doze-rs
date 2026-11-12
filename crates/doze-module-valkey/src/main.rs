//! The valkey module (docs/port/modules/PLAN.md step 7): one valkey-server, gated on its backend accepting
//! connections. Valkey needs no structure converged and its protocol no preamble, so the core's generic accept, boot,
//! splice path serves it; the module only decodes, plans and names the URL.
//!
//! Better than the Go module, each tested (docs/port/modules/PLAN.md, "better than Go"):
//! - a cache that sleeps keeps its data: unless `save` says otherwise, valkey writes a snapshot when doze stops it
//!   (Go's default lost every key at every idle reap);
//! - mistakes are refused before boot at their line, with a way out ([`check`]), not found in the engine's log;
//! - the app shows memory, keys, hits and the live command stream, and can empty the cache or save now ([`admin`]);
//! - the password is in a file only the user can read, not on the command line `ps` shows, and escaped in the URL
//!   (module bug 7); the server listens where the host granted (D13).

mod admin;
mod check;
mod plan;

use doze_module::caps::{Admin, Console, Rows};
use doze_module::proto::msg::console::Resource;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::hello::{ClientCommand, Detect};
use doze_module::proto::msg::instance::{Connection, Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Resolved, StopHow};
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;
use std::path::Path;

/// The socket's name in the instance's socket directory when the host grants none.
const SOCKET: &str = "valkey.sock";

const EXAMPLE: &str = r#"valkey "cache" {
  version          = 9
  port             = 6379
  maxmemory        = "256mb"
  maxmemory_policy = "allkeys-lru"
  appendonly       = true
  password         = "cache"
  save             = "900 1 300 10"

  settings = {
    tcp-keepalive = "60"
  }
}
"#;

#[derive(Debug, Spec)]
struct ValkeySpec {
    #[spec(doc = "Memory ceiling before eviction, e.g. \"256mb\".")]
    maxmemory: Option<String>,
    #[spec(
        doc = "Eviction policy (allkeys-lru, volatile-ttl, …); unset, valkey's own default is noeviction."
    )]
    maxmemory_policy: Option<String>,
    #[spec(default = false, doc = "Enable the append-only file for durability.")]
    appendonly: bool,
    #[spec(doc = "requirepass auth password.")]
    password: Option<String>,
    #[spec(
        doc = "RDB snapshot schedule, e.g. \"900 1 300 10\". Unset, a snapshot is written when doze stops the server, so \
               a sleeping cache keeps its keys; \"\" keeps nothing."
    )]
    save: Option<String>,
    #[spec(doc = "Arbitrary valkey.conf directives, applied verbatim.")]
    settings: BTreeMap<String, String>,
}

impl ValkeySpec {
    fn password(&self) -> Option<&str> {
        self.password.as_deref().filter(|p| !p.is_empty())
    }
}

struct Valkey;

impl Module for Valkey {
    type Spec = ValkeySpec;

    fn about(&self) -> About {
        About {
            engine: "valkey".into(),
            category: "cache".into(),
            title: "Valkey".into(),
            tagline: "In-memory cache, Redis-compatible.".into(),
            versions: vec!["8".into(), "9".into()],
            exact_dots: 2,
            default_port: 6379,
            backend_socket: Some(SOCKET.into()),
            example: EXAMPLE.into(),
            // doze-binaries builds valkey for these; there is no Windows valkey (PLAN.md D12).
            targets: [
                "aarch64-apple-darwin",
                "aarch64-unknown-linux-gnu",
                "x86_64-unknown-linux-gnu",
            ]
            .map(String::from)
            .to_vec(),
            description: Some(
                "A local Valkey server — the open-source Redis fork. Speaks the Redis protocol, so any redis client \
                 works. Set a memory ceiling and an eviction policy and use it as a cache; doze boots it on first \
                 connect and reaps it when idle."
                    .into(),
            ),
            homepage: Some("https://github.com/doze-dev/doze-rs/tree/main/crates/doze-module-valkey".into()),
            source: Some("doze/valkey".into()),
            example_label: Some("cache".into()),
            console: Some(admin::console()),
            actions: admin::actions(),
            default_version: Some("9".into()),
            client: Some(client()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &ValkeySpec, _instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec)
    }

    fn connection(&self, cx: &Cx<ValkeySpec>) -> Result<Connection> {
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

    async fn resolve(&self, cx: &Cx<ValkeySpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        help::resolve_via(cx, pins, target, 2).await
    }

    async fn provision(&self, cx: &Cx<ValkeySpec>, _tc: &Toolchain) -> Result<Provisioned> {
        help::empty_data_dir(&cx.instance.data_dir)
    }

    async fn plan(&self, cx: &Cx<ValkeySpec>, tc: &Toolchain) -> Result<Plan> {
        let s = &cx.spec;
        let data = &cx.instance.data_dir;
        let mut args = plan::auth_file(Path::new(data), s.password())?;
        let (listen, ready) = plan::listen(help::prepare_backend(&cx.instance, SOCKET)?)?;
        args.extend(listen);
        let save = s.save.clone().unwrap_or_default();
        let appendonly = if s.appendonly { "yes" } else { "no" };
        for a in [
            "--dir",
            data,
            "--daemonize",
            "no",
            "--save",
            &save,
            "--appendonly",
            appendonly,
        ] {
            args.push(a.to_string());
        }
        if s.save.is_none() {
            // No periodic snapshots, but one when doze stops it: a cache that sleeps wakes with its keys.
            args.extend(["--shutdown-on-sigterm".to_string(), "save".to_string()]);
        }
        let typed = [
            ("maxmemory", &s.maxmemory),
            ("maxmemory-policy", &s.maxmemory_policy),
        ];
        for (k, v) in typed {
            if let Some(v) = v.as_deref().filter(|v| !v.is_empty()) {
                args.extend([format!("--{k}"), v.to_string()]);
            }
        }
        for (k, v) in help::settings(&s.settings, &[]) {
            args.extend([format!("--{k}"), v.to_string()]);
        }
        let program =
            Path::new(&tc.bin_dir).join(format!("valkey-server{}", std::env::consts::EXE_SUFFIX));
        Ok(Plan {
            processes: vec![Process {
                name: cx.instance.name.clone(),
                program: program.to_string_lossy().into_owned(),
                args,
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

/// `valkey-cli` from the engine's own build, through doze's address (so it wakes the instance), with the password in
/// the client's own variable rather than on its command line.
fn client() -> ClientCommand {
    ClientCommand {
        program: "valkey-cli".into(),
        args: ["-h", "{host}", "-p", "{port}"].map(String::from).to_vec(),
        env: BTreeMap::from([("REDISCLI_AUTH".into(), "{password}".into())]),
        fallbacks: vec!["redis-cli".into()],
    }
}

/// What protocol 1's core looked for (internal/detect): redis clients, redis and valkey images, the usual variables.
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            (
                "go".into(),
                list(&[
                    "github.com/redis/go-redis",
                    "github.com/go-redis/redis",
                    "github.com/gomodule/redigo",
                    "github.com/valkey-io/valkey-go",
                ]),
            ),
            ("npm".into(), list(&["redis", "ioredis", "bullmq"])),
        ]),
        images: list(&["redis", "valkey/valkey"]),
        env: [
            ("REDIS_URL", "url"),
            ("VALKEY_URL", "url"),
            ("CACHE_ADDR", "address"),
            ("REDIS_ADDR", "address"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into(),
    }
}

impl Admin for Valkey {
    async fn resources(&self, cx: &Cx<ValkeySpec>) -> Result<Vec<Resource>> {
        admin::resources(cx).await
    }

    async fn run_action(
        &self,
        cx: &Cx<ValkeySpec>,
        action: &str,
        _resource: &str,
        _input: Option<&str>,
    ) -> Result<String> {
        admin::run_action(cx, action).await
    }
}

impl Console for Valkey {
    async fn tail(
        &self,
        cx: &Cx<ValkeySpec>,
        _panel: &str,
        _selector: &str,
        out: &Rows<'_>,
    ) -> Result<()> {
        admin::tail(cx, out).await
    }
}

fn main() -> std::process::ExitCode {
    serve(Valkey)
        .release(env!("CARGO_PKG_VERSION"))
        .admin()
        .console()
        .run()
}
