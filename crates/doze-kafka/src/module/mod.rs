//! The doze module face of doze-kafka (docs/port/phase11.md; docs/port/modules/PLAN.md step 14): the same binary,
//! started by doze as its kafka module, speaks module protocol 2 and plans itself as the engine.
//!
//! It is the module when it runs under the name a module release packages it as (`kafka-plugin`, how doze starts it,
//! with no arguments or only `--describe`), or as `doze-kafka module`. Anything else is the broker, as before; the plan
//! starts the binary with the broker's own flags, so a `kafka-plugin` with arguments is the broker too.
//!
//! What it does, against the Go module it replaces (`doze-kafka/module`, the oracle: `tools/module-oracle/kafka`):
//! the same block (`auto_create_topics`, `default_partitions`, `retention`, `retention_bytes`, `topic "name"
//! { partitions, config }`), the same variable (`KAFKA_BROKERS`, the broker's address), the same console panels and
//! produce action, the same listeners but the registry's (only once this broker has one). Better than Go, each tested:
//! - what the broker would refuse is refused before boot at its line ([`check`]): topic names, partition counts, topic
//!   configs (by the broker's own table, with a "did you mean"), a version that is not 1 to 4, a negative retention;
//!   retention also takes days (`7d`);
//! - converge keeps topics as declared: it raises partitions and sets changed configs on topics that exist, where Go
//!   only created missing ones (and never deletes: a topic left out of the block keeps its data, and Kafka cannot
//!   remove partitions, which is said, not done);
//! - the topics are the instance's objects, so `doze plan` shows them;
//! - the console answers on the instance's name (`--console-allow-host`) and the app can open it (`web_url`).

mod admin;
mod check;
mod client;
mod converge;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use doze_module::proto::msg::error::{Diagnostic, Error, ErrorKind};
use doze_module::proto::msg::hello::{Detect, Type};
use doze_module::proto::msg::instance::{Address, Connection, Instance, Listener, Toolchain};
use doze_module::proto::msg::run::{Pin, Plan, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::proto::msg::value::{Node, Value};
use doze_module::spec::{Decoding, Field};
use doze_module::{About, Cx, Module, Result, Spec, help, serve};

/// The name a module release gives the binary (`bin/<engine>-plugin`), which doze starts with no arguments.
const PLUGIN_SUFFIX: &str = "-plugin";

/// The published example: the Go module's, unchanged.
const EXAMPLE: &str = r#"kafka "events" {
  version = 4
  port    = 9092

  auto_create_topics = true
  default_partitions = 1
  retention          = "168h"

  topic "orders" {
    partitions = 3
  }
  topic "payments" {
    partitions = 1
    config = { "cleanup.policy" = "compact" }
  }
}
"#;

/// The console's and the schema registry's listeners, and their ports on the instance's address (Go's).
const CONSOLE: &str = "console";
const REGISTRY: &str = "registry";
const CONSOLE_PORT: u16 = 9093;
const REGISTRY_PORT: u16 = 8081;

/// Whether this process was started as the module (see the module docs).
pub(crate) fn invoked() -> bool {
    let args: Vec<OsString> = std::env::args_os().collect();
    is_module(&args)
}

fn is_module(args: &[OsString]) -> bool {
    let rest = args.get(1..).unwrap_or_default();
    if rest.first().is_some_and(|a| a == "module") {
        return true;
    }
    let plugin = args
        .first()
        .and_then(|a| Path::new(a).file_stem())
        .is_some_and(|s| s.to_string_lossy().ends_with(PLUGIN_SUFFIX));
    plugin && rest.iter().all(|a| a == "--describe")
}

/// Serves doze on the channel it was started with, until doze goes.
pub(crate) fn run() -> std::process::ExitCode {
    serve(Kafka)
        .release(env!("CARGO_PKG_VERSION"))
        .converge()
        .objects()
        .health()
        .admin()
        .console()
        .run()
}

/// The `kafka` block.
#[derive(Debug, Spec)]
pub(crate) struct KafkaSpec {
    #[spec(default = true, doc = "Create unknown topics on first reference.")]
    auto_create_topics: bool,
    #[spec(
        default = 1,
        doc = "Partition count for auto-created topics (1 to 1000)."
    )]
    default_partitions: i32,
    #[spec(
        doc = "Delete segments older than this: a duration such as \"168h\", \"90m\" or \"7d\"; unset keeps them for ever."
    )]
    retention: Option<String>,
    #[spec(
        default = 0,
        doc = "Delete old segments once a partition exceeds this many bytes (0 = unbounded)."
    )]
    retention_bytes: i64,
    #[spec(block, doc = "A topic to create on boot and keep as declared.")]
    topic: Vec<Topic>,
}

/// A declared topic.
#[derive(Debug, Spec)]
pub(crate) struct Topic {
    #[spec(label)]
    name: String,
    #[spec(
        default = 1,
        doc = "Partition count (1 to 1000). Raised on a topic that has fewer; Kafka cannot remove partitions."
    )]
    partitions: i32,
    #[spec(doc = "Per-topic config, as Kafka names it (e.g. cleanup.policy = compact).")]
    config: BTreeMap<String, ConfigValue>,
}

impl Topic {
    /// The declared configs as text.
    pub(crate) fn configs(&self) -> BTreeMap<String, String> {
        self.config
            .iter()
            .map(|(k, v)| (k.clone(), v.0.clone()))
            .collect()
    }
}

/// A topic config's value: text, as Kafka takes it, written as a string, a number or a bool (`retention.ms =
/// 86400000`), as HCL's own conversion to a string map allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigValue(String);

impl Field for ConfigValue {
    fn ty() -> Type {
        Type::String
    }
    fn read(node: &Node, cx: &mut Decoding) -> Option<ConfigValue> {
        match &node.value {
            Value::String(s) | Value::Number(s) => Some(ConfigValue(s.clone())),
            Value::Bool(b) => Some(ConfigValue(b.to_string())),
            _ => {
                cx.error(
                    "a topic config's value is a string, a number or a bool",
                    None,
                );
                None
            }
        }
    }
}

struct Kafka;

/// The listeners: the broker (TCP: it cannot listen on a socket), its console on the instance's own name, and the
/// schema registry on `registry.<name>` once the broker has one.
pub(crate) fn listeners(console: bool, registry: bool) -> Vec<Listener> {
    let http = |name: &str, port: u16, route: &str| Listener {
        name: name.into(),
        port,
        http: true,
        routes: vec![route.into()],
        tcp: true,
    };
    let mut out = vec![Listener {
        name: doze_module::proto::network::MAIN.into(),
        port: 0,
        http: false,
        routes: vec![],
        tcp: true,
    }];
    if console {
        out.push(http(CONSOLE, CONSOLE_PORT, "@"));
    }
    if registry {
        out.push(http(REGISTRY, REGISTRY_PORT, REGISTRY));
    }
    out
}

/// Whether the broker this binary runs has a command-line flag (the module plans only what the broker accepts).
fn broker_has(flag: &str) -> bool {
    use clap::CommandFactory;
    crate::cli::Args::command()
        .get_arguments()
        .any(|a| a.get_long() == Some(flag))
}

/// The schema registry is planned only when the broker can serve one: its flag is the Go edition's,
/// `--schema-registry-addr` (docs/port/phase11.md, "The schema registry"). Until then the instance has no registry
/// listener, rather than a name that answers nothing.
fn has_registry() -> bool {
    broker_has("schema-registry-addr")
}

fn has_console() -> bool {
    cfg!(feature = "console")
}

/// A granted backend's `host:port`: the broker and its console listen on TCP only.
fn tcp_backend(instance: &Instance, listener: &str) -> Result<Option<String>> {
    match instance.listeners.get(listener).map(|g| &g.backend) {
        Some(Address::Tcp(a)) => Ok(Some(a.clone())),
        Some(Address::Unix(_)) => Err(Error::new(
            ErrorKind::Unsupported,
            format!(
                "the kafka broker cannot listen on a socket; listener {listener:?} needs a TCP backend"
            ),
        )),
        None => Ok(None),
    }
}

/// The broker's command line for an instance.
pub(crate) fn broker_args(
    instance: &Instance,
    spec: &KafkaSpec,
    console: bool,
    registry: bool,
) -> Result<Vec<String>> {
    let main = tcp_backend(instance, doze_module::proto::network::MAIN)?
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "no backend granted for \"main\""))?;
    let major =
        check::major(&instance.version).map_err(|d| Error::new(ErrorKind::Invalid, d.message))?;
    let mut args: Vec<String> = vec![
        "--listen".into(),
        main,
        "--data-dir".into(),
        instance.data_dir.clone(),
        "--kafka-version".into(),
        major.to_string(),
    ];
    // Clients bootstrap at the address doze gives them and then dial what Metadata advertises: that same address.
    if let Some(g) = instance.listeners.get(doze_module::proto::network::MAIN) {
        let (host, port) = help::host_port(&g.addr)?;
        args.extend([
            "--advertised-host".into(),
            host.into(),
            "--advertised-port".into(),
            port.into(),
        ]);
    }
    match tcp_backend(instance, CONSOLE)?.filter(|_| console) {
        Some(addr) => {
            args.extend(["--console-addr".into(), addr]);
            // The console turns away a Host it does not know (DNS rebinding); doze reaches it by the instance's name.
            let hosts = instance.listeners.get(CONSOLE).map(|g| g.hosts.clone());
            for h in hosts.unwrap_or_default() {
                args.extend(["--console-allow-host".into(), h]);
            }
        }
        // No console was granted: none is served (the broker's own default would take 127.0.0.1:9093 for itself).
        None => args.push("--console=false".into()),
    }
    if registry {
        match tcp_backend(instance, REGISTRY)? {
            Some(addr) => args.extend(["--schema-registry-addr".into(), addr]),
            None => args.push("--schema-registry=false".into()),
        }
    }
    if !spec.auto_create_topics {
        args.push("--auto-create-topics=false".into());
    }
    if spec.default_partitions != 1 {
        args.extend([
            "--default-partitions".into(),
            spec.default_partitions.to_string(),
        ]);
    }
    let retention = match spec.retention.as_deref() {
        Some(r) => check::retention_ms(r).map_err(|e| Error::new(ErrorKind::Invalid, e))?,
        None => 0,
    };
    if retention > 0 {
        args.extend(["--retention-ms".into(), retention.to_string()]);
    }
    if spec.retention_bytes > 0 {
        args.extend(["--retention-bytes".into(), spec.retention_bytes.to_string()]);
    }
    Ok(args)
}

impl Module for Kafka {
    type Spec = KafkaSpec;

    fn about(&self) -> About {
        let targets = [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        About {
            engine: "kafka".into(),
            category: "queue".into(),
            title: "Kafka".into(),
            tagline: "Single-node Kafka-protocol broker, no JVM.".into(),
            versions: ["1", "2", "3", "4"].map(String::from).to_vec(),
            exact_dots: 0,
            default_port: 9092,
            example: EXAMPLE.into(),
            console: Some(admin::console()),
            actions: admin::actions(),
            targets: targets.map(String::from).to_vec(),
            description: Some(
                "A local, disk-backed broker that speaks the Kafka wire protocol well enough that unmodified clients \
                 (franz-go, librdkafka, kcat, Sarama, the Java client) connect and work. No JVM, no ZooKeeper, no \
                 KRaft: the engine is doze-kafka, the module binary itself, and `version` selects the Kafka major it \
                 stands for (1-4). Declared topics are created on boot and kept as declared; auto-create handles the \
                 rest."
                    .into(),
            ),
            homepage: Some("https://github.com/doze-dev/doze-rs".into()),
            source: Some("doze/kafka".into()),
            example_label: Some("events".into()),
            default_version: Some("4".into()),
            detect: Some(detect()),
            ..About::default()
        }
    }

    fn check(&self, spec: &KafkaSpec, instance: &Instance) -> Vec<Diagnostic> {
        check::check(spec, &instance.version)
    }

    fn network(&self, _cx: &Cx<KafkaSpec>) -> Vec<Listener> {
        listeners(has_console(), has_registry())
    }

    fn connection(&self, cx: &Cx<KafkaSpec>) -> Result<Connection> {
        // A bare host:port, no scheme: what every Kafka client's bootstrap list takes.
        let addr = cx
            .instance
            .listeners
            .get(doze_module::proto::network::MAIN)
            .map_or_else(|| "localhost:9092".to_string(), |g| g.addr.clone());
        Ok(Connection {
            env_var: "KAFKA_BROKERS".into(),
            url: addr,
            env: BTreeMap::new(),
        })
    }

    async fn resolve(&self, cx: &Cx<KafkaSpec>, _pins: &[Pin], _target: &str) -> Result<Resolved> {
        // Nothing to fetch: the engine is this binary, and the version is the Kafka major it stands for.
        let major = check::major(&cx.instance.version)
            .map_err(|d| Error::new(ErrorKind::Invalid, d.message))?;
        let exe = std::env::current_exe()
            .map_err(|e| Error::new(ErrorKind::Internal, format!("finding this binary: {e}")))?;
        let bin_dir = exe
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Resolved {
            toolchain: Toolchain {
                engine: "kafka".into(),
                full: major.to_string(),
                bin_dir,
                tools: BTreeMap::new(),
            },
            pins: vec![],
        })
    }

    async fn provision(&self, cx: &Cx<KafkaSpec>, _tc: &Toolchain) -> Result<Provisioned> {
        help::empty_data_dir(&cx.instance.data_dir)
    }

    async fn plan(&self, cx: &Cx<KafkaSpec>, _tc: &Toolchain) -> Result<Plan> {
        let args = broker_args(&cx.instance, &cx.spec, has_console(), has_registry())?;
        let main =
            tcp_backend(&cx.instance, doze_module::proto::network::MAIN)?.unwrap_or_default();
        let program = std::env::current_exe()
            .map_err(|e| Error::new(ErrorKind::Internal, format!("finding this binary: {e}")))?;
        Ok(Plan {
            processes: vec![Process {
                name: cx.instance.name.clone(),
                program: program.to_string_lossy().into_owned(),
                args,
                env: BTreeMap::new(),
                dir: None,
                after: vec![],
                // The broker opens its data before it binds, so a port that accepts is a broker that serves.
                ready: Some(Ready::Tcp(main)),
                hooks: vec![],
                stop: Some(StopHow::Terminate),
            }],
        })
    }
}

/// How `doze init` recognises a project that uses Kafka (protocol 1's core, `internal/detect/detect.go`).
fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            (
                "go".into(),
                list(&[
                    "github.com/twmb/franz-go",
                    "github.com/segmentio/kafka-go",
                    "github.com/IBM/sarama",
                    "github.com/Shopify/sarama",
                    "github.com/confluentinc/confluent-kafka-go",
                ]),
            ),
            ("npm".into(), list(&["kafkajs", "node-rdkafka"])),
        ]),
        images: list(&[
            "confluentinc/cp-kafka",
            "bitnami/kafka",
            "apache/kafka",
            "redpandadata/redpanda",
        ]),
        env: BTreeMap::from([
            ("KAFKA_BROKERS".into(), "address".into()),
            ("KAFKA_BOOTSTRAP_SERVERS".into(), "address".into()),
            ("KAFKA_URL".into(), "address".into()),
        ]),
    }
}
