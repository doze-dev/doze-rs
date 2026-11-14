//! What a loaded doze.hcl is: the typed stack, as Go's `config.Config` holds it.

use doze_hcl::Value;
use std::collections::BTreeMap;
use std::time::Duration;

/// The default client-facing address (Go's `DefaultListen`).
pub const DEFAULT_LISTEN: &str = "127.0.0.1:6432";
/// How long an instance sits idle before it sleeps, unless the stack says otherwise.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(120);
/// How long a command gets to become ready when nothing says.
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(60);
/// The engine every `app`, `service` and `task` runs on.
pub const COMMAND_ENGINE: &str = "process";

/// The four nouns of a stack. A reference is always `<kind>.<name>.<attribute>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Module,
    App,
    Service,
    Task,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Module, Kind::App, Kind::Service, Kind::Task];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Module => "module",
            Kind::App => "app",
            Kind::Service => "service",
            Kind::Task => "task",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// "an app", "a module".
    pub fn article(self) -> String {
        match self {
            Kind::App => "an app".into(),
            k => format!("a {}", k.as_str()),
        }
    }
}

/// A block's header as written: `module postgres "db"`, `app "api"`.
pub fn title(kind: Kind, ty: &str, name: &str) -> String {
    let q = doze_hcl::value::quote::go_quote(name);
    match kind {
        Kind::Module => format!("module {ty} {q}"),
        k => format!("{} {q}", k.as_str()),
    }
}

/// A reference to a declared block: `task.migrate`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ref {
    pub kind: Kind,
    pub name: String,
}

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.kind.as_str(), self.name)
    }
}

/// How far a dependency is woken before what needs it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Condition {
    #[default]
    Healthy,
    Started,
    Lazy,
}

impl Condition {
    pub fn as_str(self) -> &'static str {
        match self {
            Condition::Healthy => "healthy",
            Condition::Started => "started",
            Condition::Lazy => "lazy",
        }
    }
}

/// One dependency of an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub condition: Condition,
}

/// A decoded `sleep`: unset, `"never"`, or a duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sleep {
    pub set: bool,
    pub never: bool,
    pub after: Duration,
}

/// A decoded `ready = …` probe. `arg` is the path, the text or the command; empty for tcp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub kind: String,
    pub arg: String,
}

impl Probe {
    /// Whether the probe can only run against a declared port.
    pub fn needs_port(&self) -> bool {
        self.kind == "tcp" || self.kind == "http"
    }
}

/// A command's restart policy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Restart {
    pub policy: String,
    pub backoff: Duration,
    pub max_retries: i64,
}

/// What an `app`, `service` or `task` runs: the command engine's config (Go's `process.Config`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Command {
    /// Absolute working directory.
    pub dir: String,
    pub run: String,
    pub env: BTreeMap<String, String>,
    /// Absolute, empty if none.
    pub env_file: String,
    pub setup: Vec<String>,
    pub on_ready: Vec<String>,
    pub stop: Vec<String>,
    pub ready: Option<Probe>,
    pub ready_timeout: Duration,
    pub restart: Restart,
    pub sleep: Sleep,
    pub console: String,
    pub watch: Vec<String>,
    pub direct: bool,
    /// By platform (`mac`, `mac_intel`, `linux`, `linux_arm`), or the single key "" for all.
    pub download: BTreeMap<String, String>,
    /// The block set `dir` itself (if not, a download is where it runs).
    pub dir_set: bool,
    pub service: bool,
    pub task: bool,
    pub inputs: Vec<String>,
    pub manual: bool,
    /// The block's `port`: doze holds it and gives the command a private `$PORT`, unless `direct`.
    pub declared_port: u16,
}

impl Command {
    /// doze fronts it: it declares a port and has not opted out with `direct`.
    pub fn proxied(&self) -> bool {
        self.declared_port > 0 && !self.direct
    }
}

/// A module block's own arguments and nested blocks, evaluated as written: what its module reads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModuleBody {
    pub attrs: BTreeMap<String, Value>,
    pub blocks: Vec<ModuleBlock>,
}

/// A nested block of a module body.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleBlock {
    pub ty: String,
    pub labels: Vec<String>,
    pub body: ModuleBody,
}

/// What an instance runs with.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Spec {
    /// Not decoded: a module in the shape pass.
    #[default]
    None,
    Command(Box<Command>),
    Module(ModuleBody),
}

/// One declared instance: a module, an app, a service or a task.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Instance {
    pub kind: Option<Kind>,
    /// The engine type: the module's, or [`COMMAND_ENGINE`].
    pub ty: String,
    pub name: String,
    /// "16", "16.14", or "builtin" for a versionless engine.
    pub version: String,
    /// A full address override (`host:port`, `unix:/path`).
    pub listen: String,
    pub port: u16,
    pub domains: Vec<String>,
    pub cert: String,
    pub key: String,
    pub spec: Spec,
    /// Declaration order.
    pub index: usize,
    pub deps: Vec<Dependency>,
    pub enabled: bool,
    pub after: Vec<Ref>,
    pub refs: Vec<Ref>,
    /// A module block's own `sleep` (an app's is in its command).
    pub sleep: Sleep,
    /// A module block's `routes = false`.
    pub no_routes: bool,
}

impl Instance {
    pub fn kind(&self) -> Kind {
        self.kind.unwrap_or(if self.ty == COMMAND_ENGINE {
            Kind::App
        } else {
            Kind::Module
        })
    }

    /// The block header as written.
    pub fn title(&self) -> String {
        title(self.kind(), &self.ty, &self.name)
    }

    pub fn command(&self) -> Option<&Command> {
        match &self.spec {
            Spec::Command(c) => Some(c),
            _ => None,
        }
    }
}

/// A `service` block as written, for what an instance cannot say (its downloads, its own restart).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServiceDecl {
    pub name: String,
    pub run: String,
    pub dir: String,
    pub port: u16,
    pub dir_set: bool,
    pub env: BTreeMap<String, String>,
    pub env_file: String,
    pub download: BTreeMap<String, String>,
    pub setup: Vec<String>,
    pub on_ready: Vec<String>,
    pub stop: Vec<String>,
    pub ready: Option<Probe>,
    pub ready_timeout: Duration,
    pub restart: Restart,
    pub sleep: Sleep,
    pub console: String,
    pub after: Vec<Ref>,
    pub refs: Vec<Ref>,
}

/// A `task` block as written.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TaskDecl {
    pub name: String,
    pub run: String,
    pub dir: String,
    pub env: BTreeMap<String, String>,
    pub env_file: String,
    pub inputs: Vec<String>,
    pub manual: bool,
    pub after: Vec<Ref>,
    pub refs: Vec<Ref>,
}

/// A declared `output`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    pub name: String,
    pub value: String,
    pub description: String,
    pub sensitive: bool,
}

/// The stack's defaults for every command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandDefaults {
    pub ready_timeout: Duration,
    pub restart_policy: String,
    pub restart_backoff: Duration,
    pub restart_max_retries: i64,
    pub env: BTreeMap<String, String>,
}

/// The `defaults` block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Defaults {
    /// Zero: only blocks with a sleep of their own sleep.
    pub idle_timeout: Duration,
    /// `names = true`: every instance gets `<name>.<stack>.doze`.
    pub names: bool,
    pub log_runs: i64,
    pub log_bytes: i64,
    pub commands: CommandDefaults,
}

/// The `tls` block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tls {
    pub enabled: bool,
    pub cert: String,
    pub key: String,
    pub required: bool,
}

/// A loaded, validated doze.hcl.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    /// What loads but is likely a mistake (`doze lint` prints these).
    pub warnings: Vec<String>,
    pub listen: String,
    /// The doze home: `home`, `$DOZE_HOME`, or `~/.doze`.
    pub home: String,
    /// This project's state: `<home>/projects/<slug>` unless `data_dir` says.
    pub data_dir: String,
    /// The `stack` argument as written ("" when unset).
    pub stack_name: String,
    pub defaults: Defaults,
    pub tls: Tls,
    pub instances: Vec<Instance>,
    pub services: Vec<ServiceDecl>,
    pub tasks: Vec<TaskDecl>,
    pub outputs: BTreeMap<String, Output>,
    pub output_order: Vec<String>,
    /// The file (or folder) the config was loaded from.
    pub path: String,
    /// The folder the config is in, absolute.
    pub dir: String,
}

impl Config {
    pub fn instance(&self, name: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.name == name)
    }

    pub fn service(&self, name: &str) -> Option<&ServiceDecl> {
        self.services.iter().find(|s| s.name == name)
    }

    pub fn task(&self, name: &str) -> Option<&TaskDecl> {
        self.tasks.iter().find(|t| t.name == name)
    }
}
