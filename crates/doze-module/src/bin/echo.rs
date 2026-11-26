//! The echo module: a module written against the SDK the way real ones will be, for tests/echo.rs to drive through
//! a real host peer. Its "engine" is imaginary; everything around it is real.

use doze_module::caps::Objects;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::{Address, Connection, Instance, Object, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::proto::msg::value::PathSeg;
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;

#[path = "echo/side.rs"]
mod side;

#[path = "echo/answers.rs"]
mod answers;
#[path = "echo/blocks.rs"]
mod blocks;

#[derive(Debug, Spec)]
struct EchoSpec {
    #[spec(since = 2, doc = "What the engine says first (engine 2 and later).")]
    greeting: Option<String>,
    #[spec(default = 10, doc = "Buffer size in kilobytes.")]
    buffer: u32,
    #[spec(doc = "Any other engine setting, passed through.")]
    settings: BTreeMap<String, String>,
    #[spec(block, doc = "A named channel.")]
    channel: Vec<Channel>,
}

#[derive(Debug, Spec)]
struct Channel {
    #[spec(label)]
    name: String,
    #[spec(default = 1, doc = "Messages the channel holds.")]
    size: u32,
}

struct Echo;

impl Module for Echo {
    type Spec = EchoSpec;

    fn about(&self) -> About {
        About {
            engine: "echo".into(),
            category: "test".into(),
            title: "Echo".into(),
            tagline: "A module for testing the SDK".into(),
            versions: versions(),
            exact_dots: 2,
            default_port: 7000,
            supervised: side::supervised(),
            backend_socket: Some("echo.sock".into()),
            example: "echo \"e\" {\n  version = \"2\"\n}\n".into(),
            console: Some(blocks::console()),
            // The engine's own build has a client: `doze connect` runs it there, at doze's address for the instance.
            client: Some(doze_module::proto::msg::hello::ClientCommand {
                program: "doze-module-echo-engine".into(),
                args: ["client", "{host}", "{port}"].map(String::from).to_vec(),
                env: BTreeMap::from([("ECHO_PASSWORD".into(), "{password}".into())]),
                fallbacks: vec!["nc".into()],
            }),
            actions: blocks::actions(),
            targets: [
                "aarch64-apple-darwin",
                "aarch64-unknown-linux-gnu",
                "x86_64-unknown-linux-gnu",
                "x86_64-pc-windows-msvc",
                "aarch64-pc-windows-msvc",
            ]
            .map(String::from)
            .to_vec(),
            description: Some(
                "An engine that echoes lines, for testing the SDK, the host and the release tool."
                    .into(),
            ),
            example_label: Some("e".into()),
            ..About::default()
        }
    }

    fn check(&self, spec: &EchoSpec, _instance: &Instance) -> Vec<Diagnostic> {
        // Every call decodes and checks the spec first: `decode` is the one call that does nothing after.
        record(spec, "check");
        let at = |i: usize| {
            vec![
                PathSeg::Key("channel".into()),
                PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX)),
                PathSeg::Key("size".into()),
            ]
        };
        let message = "a channel holds at least one message".to_string();
        spec.channel
            .iter()
            .enumerate()
            .filter(|(_, c)| c.size == 0)
            .map(|(i, _)| Diagnostic {
                path: at(i),
                message: message.clone(),
                hint: None,
            })
            .collect()
    }

    fn network(&self, cx: &Cx<EchoSpec>) -> Vec<doze_module::proto::msg::instance::Listener> {
        side::network(&cx.spec.settings)
    }

    fn connection(&self, cx: &Cx<EchoSpec>) -> Result<Connection> {
        let addr = cx
            .instance
            .listeners
            .get("main")
            .map(|l| l.addr.clone())
            .unwrap_or_default();
        Ok(Connection {
            env_var: "ECHO_URL".into(),
            url: format!("echo://{addr}/{}", cx.instance.name),
            // What aws needs and protocol 1's core guessed: more variables a client must set.
            env: [("ECHO_TOKEN".to_string(), "doze".to_string())]
                .into_iter()
                .chain(side::env(&cx.spec.settings, &cx.instance))
                .collect(),
        })
    }

    async fn resolve(&self, cx: &Cx<EchoSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        record(&cx.spec, "resolve");
        help::resolve_via(cx, pins, target, 2).await
    }

    async fn provision(&self, cx: &Cx<EchoSpec>, _tc: &Toolchain) -> Result<Provisioned> {
        record(&cx.spec, "provision");
        let marker = std::path::Path::new(&cx.instance.data_dir).join("ECHO_VERSION");
        if marker.exists() {
            return Ok(Provisioned::Existing);
        }
        let err = |e: std::io::Error| {
            doze_module::proto::msg::error::Error::new(
                doze_module::proto::msg::error::ErrorKind::Unavailable,
                e.to_string(),
            )
        };
        std::fs::create_dir_all(&cx.instance.data_dir).map_err(err)?;
        std::fs::write(&marker, cx.instance.version.as_bytes()).map_err(err)?;
        Ok(Provisioned::Fresh)
    }

    async fn plan(&self, cx: &Cx<EchoSpec>, tc: &Toolchain) -> Result<Plan> {
        record(&cx.spec, "plan");
        cx.log(2, "planning the echo engine");
        // Listen where the host said (PLAN.md D13): a Unix socket, or loopback TCP where the host gives no sockets.
        let backend = cx.instance.listeners.get("main").map(|l| l.backend.clone());
        let socket = std::path::Path::new(&cx.instance.socket_dir)
            .join("echo.sock")
            .to_string_lossy()
            .into_owned();
        let (listen, mut ready) = match backend {
            Some(Address::Tcp(a)) => (format!("tcp:{a}"), Ready::Tcp(a)),
            Some(Address::Unix(p)) => (format!("unix:{p}"), Ready::Socket(p)),
            None => (format!("unix:{socket}"), Ready::Socket(socket)),
        };
        // `settings.ready = "log"`: ready when the engine says so, so where it is reached comes from the grant alone.
        if cx.spec.settings.get("ready").is_some_and(|r| r == "log") {
            ready = Ready::LogLine("listening".into());
        }
        let mut args = vec![
            "--listen".to_string(),
            listen,
            "--buffer".to_string(),
            cx.spec.buffer.to_string(),
        ];
        if let Some(g) = &cx.spec.greeting {
            args.push(format!("--greeting={g}"));
        }
        for (k, v) in help::settings(&cx.spec.settings, &["socket", "ready", side::SIDE]) {
            args.push(format!("--{k}={v}"));
        }
        let program = std::path::Path::new(&tc.bin_dir)
            .join(format!(
                "doze-module-echo-engine{}",
                std::env::consts::EXE_SUFFIX
            ))
            .to_string_lossy()
            .into_owned();
        let process = Process {
            name: "echo".into(),
            program,
            args,
            env: BTreeMap::new(),
            dir: None,
            after: vec![],
            ready: Some(ready),
            hooks: vec![],
            stop: Some(StopHow::Terminate),
        };
        let side = side::process(&cx.spec.settings, &cx.instance, &process.program)?;
        Ok(Plan {
            processes: std::iter::once(process).chain(side).collect(),
        })
    }
}

impl Objects for Echo {
    fn objects(&self, cx: &Cx<EchoSpec>) -> Result<Vec<Object>> {
        Ok(cx
            .spec
            .channel
            .iter()
            .map(|c| Object {
                kind: "channel".into(),
                name: c.name.clone(),
                hash: String::new(),
            })
            .collect())
    }
}

impl doze_module::caps::Admin for Echo {
    async fn resources(
        &self,
        cx: &Cx<EchoSpec>,
    ) -> Result<Vec<doze_module::proto::msg::console::Resource>> {
        Ok(cx
            .spec
            .channel
            .iter()
            .map(|c| doze_module::proto::msg::console::Resource {
                kind: "channel".into(),
                name: c.name.clone(),
                status: "open".into(),
                info: BTreeMap::from([("size".to_string(), c.size.to_string())]),
            })
            .chain(blocks::resources())
            .collect())
    }

    async fn run_action(
        &self,
        cx: &Cx<EchoSpec>,
        action: &str,
        resource: &str,
        _input: Option<&str>,
    ) -> Result<String> {
        record(&cx.spec, &format!("{action} {resource}"));
        Ok(format!("{action}d {resource}"))
    }

    async fn run_form(
        &self,
        cx: &Cx<EchoSpec>,
        action: &str,
        resource: &str,
        input: Option<&str>,
        values: &BTreeMap<String, String>,
    ) -> Result<String> {
        if !matches!(action, "create" | "apply-migrations") {
            return self.run_action(cx, action, resource, input).await;
        }
        record(&cx.spec, &format!("{action} form"));
        blocks::run_form(action, values)
    }
}

impl doze_module::caps::Console for Echo {
    /// Two rows naming the panel and the selector, then the stream ends.
    async fn tail(
        &self,
        _cx: &Cx<EchoSpec>,
        panel: &str,
        selector: &str,
        out: &doze_module::caps::Rows<'_>,
    ) -> Result<()> {
        for i in 1..=2 {
            let row = doze_module::proto::msg::console::Row {
                cells: BTreeMap::from([
                    ("panel".to_string(), panel.to_string()),
                    ("n".to_string(), format!("{selector}{i}")),
                ]),
                tone: String::new(),
            };
            out.send(&row).await?;
        }
        Ok(())
    }
}

/// With the setting `calls` (a file), appends `what` to it: the order the host called in, for the engine's tests.
fn record(spec: &EchoSpec, what: &str) {
    use std::io::Write as _;
    let Some(file) = spec.settings.get("calls") else {
        return;
    };
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file);
    if let Ok(mut f) = f {
        let _ = writeln!(f, "{what}");
    }
}

fn main() -> std::process::ExitCode {
    serve(Echo)
        .release(env!("CARGO_PKG_VERSION"))
        .objects()
        .wire()
        .admin()
        .console()
        .migrate()
        .values()
        .tree()
        .run()
}

impl doze_module::caps::Wire for Echo {
    async fn accept(
        &self,
        conn: tokio::net::TcpStream,
        _req: &doze_module::proto::msg::console::WireAccept,
        host: &doze_module::caps::WireHost,
    ) -> Result<()> {
        use doze_module::proto::msg::error::{Error, ErrorKind};
        use doze_module::proto::msg::instance::Address;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let io = |e: std::io::Error| Error::new(ErrorKind::Unavailable, e.to_string());
        // The startup line, as a protocol filter would read a preamble before deciding to boot.
        let mut client = BufReader::new(conn);
        let mut startup = String::new();
        client.read_line(&mut startup).await.map_err(io)?;
        let Address::Tcp(addr) = host.boot().await? else {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "the echo engine listens on TCP",
            ));
        };
        let mut backend = tokio::net::TcpStream::connect(&addr).await.map_err(io)?;
        backend.write_all(startup.as_bytes()).await.map_err(io)?;
        host.acquire().await?;
        let mut client = client.into_inner();
        let spliced = tokio::io::copy_bidirectional(&mut client, &mut backend).await;
        host.release().await?;
        spliced.map(drop).map_err(io)
    }
}

impl doze_module::caps::Migrate for Echo {
    /// Moves the data's version marker to the declared version, after asking the host for the old major's build (as
    /// postgres reads its old data with its old server).
    async fn migrate(&self, cx: &Cx<EchoSpec>, _pins: &[Pin], _target: &str) -> Result<String> {
        use doze_module::proto::msg::error::{Error, ErrorKind};
        record(&cx.spec, "migrate");
        let marker = std::path::Path::new(&cx.instance.data_dir).join("ECHO_VERSION");
        let from = std::fs::read_to_string(&marker).map_err(|_| {
            Error::new(
                ErrorKind::Invalid,
                format!(
                    "{} has no data yet: there is nothing to move",
                    cx.instance.name
                ),
            )
        })?;
        let to = &cx.instance.version;
        if from.trim() == to {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "{} is already on echo {to}; nothing to move",
                    cx.instance.name
                ),
            ));
        }
        let old = cx.fetch().resolve_major("echo", from.trim()).await?;
        std::fs::write(&marker, to.as_bytes())
            .map_err(|e| Error::new(ErrorKind::Unavailable, e.to_string()))?;
        Ok(format!(
            "{} is on echo {to} now (its echo {old} data was read)",
            cx.instance.name
        ))
    }
}

/// Engines 1 and 2; a copy of this program named `…-v1` serves engine 1 only (the daemon's test of one build per
/// major).
fn versions() -> Vec<String> {
    let only1 = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().ends_with("-v1")))
        .unwrap_or(false);
    if only1 {
        vec!["1".into()]
    } else {
        vec!["1".into(), "2".into()]
    }
}
