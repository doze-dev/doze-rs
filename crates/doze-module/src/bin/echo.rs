//! The echo module: a module written against the SDK the way real ones will be, for tests/echo.rs to drive through
//! a real host peer. Its "engine" is imaginary; everything around it is real.

use doze_module::caps::Objects;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::{Connection, Instance, Object, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::proto::msg::value::PathSeg;
use doze_module::{About, Cx, Module, Result, Spec, help, serve};
use std::collections::BTreeMap;

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
            versions: vec!["1".into(), "2".into()],
            exact_dots: 2,
            default_port: 7000,
            backend_socket: Some("echo.sock".into()),
            example: "echo \"e\" {\n  version = \"2\"\n}\n".into(),
            console: Some(console()),
            ..About::default()
        }
    }

    fn check(&self, spec: &EchoSpec, _instance: &Instance) -> Vec<Diagnostic> {
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
            env: BTreeMap::from([("ECHO_TOKEN".to_string(), "doze".to_string())]),
        })
    }

    async fn resolve(&self, cx: &Cx<EchoSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
        help::resolve_via(cx, pins, target, 2).await
    }

    async fn provision(&self, cx: &Cx<EchoSpec>, _tc: &Toolchain) -> Result<Provisioned> {
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
        cx.log(2, "planning the echo engine");
        let socket = std::path::Path::new(&cx.instance.socket_dir).join("echo.sock");
        let mut args = vec!["--buffer".to_string(), cx.spec.buffer.to_string()];
        if let Some(g) = &cx.spec.greeting {
            args.push(format!("--greeting={g}"));
        }
        for (k, v) in help::settings(&cx.spec.settings, &["socket"]) {
            args.push(format!("--{k}={v}"));
        }
        let program = std::path::Path::new(&tc.bin_dir)
            .join("echo-engine")
            .to_string_lossy()
            .into_owned();
        let process = Process {
            name: "echo".into(),
            program,
            args,
            env: BTreeMap::new(),
            dir: None,
            after: vec![],
            ready: Some(Ready::Socket(socket.to_string_lossy().into_owned())),
            hooks: vec![],
            stop: Some(StopHow::Terminate),
        };
        Ok(Plan {
            processes: vec![process],
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

fn main() -> std::process::ExitCode {
    serve(Echo)
        .release(env!("CARGO_PKG_VERSION"))
        .objects()
        .wire()
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

/// A console with a stat block added to the app's overview after its "vitals" card: a module powering its own block in
/// the app's page.
fn console() -> doze_module::proto::msg::console::ConsoleSpec {
    use doze_module::proto::msg::console::{Column, ConsoleSpec, Extension, Node, Panel};
    let channels = Panel {
        id: "channels".into(),
        title: "Channels".into(),
        kind: "counters".into(),
        source: "channel".into(),
        columns: vec![Column {
            key: "size".into(),
            title: "Size".into(),
            role: "count".into(),
            align: "right".into(),
        }],
        select: None,
    };
    let stat = Node {
        kind: "stat".into(),
        panel: "channels".into(),
        title: "Channels".into(),
        ..Node::default()
    };
    ConsoleSpec {
        panels: vec![channels],
        tabs: vec![],
        web_url: None,
        front: Some("console".into()),
        extend: vec![Extension {
            after: "vitals".into(),
            node: stat,
        }],
    }
}
