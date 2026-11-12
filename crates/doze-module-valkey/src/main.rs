//! The valkey module (docs/port/modules/PLAN.md step 7): one valkey-server, gated on its backend accepting
//! connections. Valkey needs no structure converged and its protocol no preamble, so the core's generic accept, boot,
//! splice path serves it; the module only decodes, plans and names the URL.
//!
//! Departures from the Go module, each deliberate: the server listens where the host granted (D13), not on a path of
//! its own choosing; the password is in a file only the user can read, not on the command line where `ps` shows it,
//! and it is escaped in the URL (module bug 7); a password with control characters is refused, as it would break the
//! config line it is written into.

use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::{Address, Connection, Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::proto::msg::value::PathSeg;
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;
use std::path::Path;

/// The socket's name in the instance's socket directory when the host grants none.
const SOCKET: &str = "valkey.sock";
/// The config file that holds the password, in the data directory.
const AUTH_FILE: &str = "doze-auth.conf";

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
    #[spec(doc = "RDB snapshot schedule, e.g. \"900 1 300 10\" (unset or empty disables it).")]
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
            ..About::default()
        }
    }

    fn check(&self, spec: &ValkeySpec, _instance: &Instance) -> Vec<Diagnostic> {
        let bad = spec
            .password()
            .is_some_and(|p| p.chars().any(char::is_control));
        if !bad {
            return vec![];
        }
        vec![Diagnostic {
            path: vec![PathSeg::Key("password".into())],
            message: "a password cannot hold control characters (a newline would end the config line it is written \
                      into)"
                .into(),
            hint: None,
        }]
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
        let mut args = auth_file(Path::new(data), s.password())?;
        let (listen, ready) = listen(help::prepare_backend(&cx.instance, SOCKET)?)?;
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

/// With a password, the config file holding it (valkey reads a config file only as its first argument); without
/// one, nothing, and an old file is removed so a dropped password does not linger on disk.
fn auth_file(data: &Path, password: Option<&str>) -> Result<Vec<String>> {
    let path = data.join(AUTH_FILE);
    let Some(p) = password else {
        help::clear_stale(&path)?;
        return Ok(vec![]);
    };
    doze_os::fs::private_dir(data).map_err(help::io_error("creating", data))?;
    doze_os::fs::write_private(&path, auth_conf(p).as_bytes())
        .map_err(help::io_error("writing", &path))?;
    Ok(vec![path.to_string_lossy().into_owned()])
}

/// `requirepass "…"`, quoted the way valkey's config parser reads a quoted argument.
fn auth_conf(password: &str) -> String {
    let quoted = password.replace('\\', "\\\\").replace('"', "\\\"");
    format!("# Written by doze on every boot.\nrequirepass \"{quoted}\"\n")
}

/// The listening arguments and the readiness check for a granted backend.
fn listen(addr: Address) -> Result<(Vec<String>, Ready)> {
    match addr {
        Address::Unix(p) => Ok((
            vec![
                "--port".into(),
                "0".into(),
                "--unixsocket".into(),
                p.clone(),
            ],
            Ready::Socket(p),
        )),
        Address::Tcp(a) => {
            let (host, port) = help::host_port(&a)?;
            let args = vec!["--port".into(), port.into(), "--bind".into(), host.into()];
            Ok((args, Ready::Tcp(a.clone())))
        }
    }
}

fn main() -> std::process::ExitCode {
    serve(Valkey).release(env!("CARGO_PKG_VERSION")).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_line_survives_quotes_and_backslashes() {
        assert_eq!(
            auth_conf(r#"a"b\c d"#),
            "# Written by doze on every boot.\nrequirepass \"a\\\"b\\\\c d\"\n"
        );
    }
}
