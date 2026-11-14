//! A stack as the engine runs it: its services, what each needs, how each sleeps. This is the seam doze-config (H4)
//! fills from doze.hcl; until then stacks are built in code with [`crate::Engine::builder`].

use crate::process::ProcessSpec;
use doze_module_proto::msg::run::Plan;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// A boxed future, for [`Planner`].
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One declared service.
#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub kind: Kind,
    /// What must be up before it starts, in order.
    pub needs: Vec<Need>,
    /// The block's own `sleep`, which decides last.
    pub sleep: Sleep,
}

impl Service {
    /// A process the engine runs itself (`app`, `service`).
    pub fn process(name: &str, spec: ProcessSpec) -> Service {
        Service {
            name: name.into(),
            kind: Kind::Process(Box::new(spec)),
            needs: Vec::new(),
            sleep: Sleep::Default,
        }
    }

    /// An instance whose processes something else plans (a module, through the module pool).
    pub fn planned(name: &str, planner: Arc<dyn Planner>) -> Service {
        Service {
            name: name.into(),
            kind: Kind::Planned(planner),
            needs: Vec::new(),
            sleep: Sleep::Default,
        }
    }

    /// Adds a need, woken to health first.
    pub fn needs(mut self, name: &str) -> Service {
        self.needs.push(Need {
            name: name.into(),
            when: When::Healthy,
        });
        self
    }

    /// Adds a need with its condition.
    pub fn needs_when(mut self, name: &str, when: When) -> Service {
        self.needs.push(Need {
            name: name.into(),
            when,
        });
        self
    }

    /// Sets the block's `sleep`.
    pub fn sleep(mut self, sleep: Sleep) -> Service {
        self.sleep = sleep;
        self
    }
}

/// How a service runs.
#[derive(Debug, Clone)]
pub enum Kind {
    Process(Box<ProcessSpec>),
    Planned(Arc<dyn Planner>),
}

/// A need of a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    pub name: String,
    pub when: When,
}

/// How far a need is woken before the service that needs it starts (Go's `engine.Condition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum When {
    /// Ready (its check passed, its `on_ready` ran).
    #[default]
    Healthy,
    /// Started (its `on_ready` is not waited for).
    Started,
    /// Not woken at all: its address is wired, and a connection wakes it.
    Lazy,
}

/// A block's `sleep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sleep {
    /// Not set: the kind decides.
    #[default]
    Default,
    /// `sleep = "never"`.
    Never,
    /// `sleep = "<duration>"`.
    After(Duration),
}

/// Where an instance is reached once running.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Endpoint {
    /// `127.0.0.1:<port>`, `unix:<path>`, or empty for a process with no port.
    pub backend: String,
}

/// What an instance is told when it is planned.
#[derive(Debug, Clone)]
pub struct PlanCtx {
    pub name: String,
    /// Its own data folder (`clusters/<name>`).
    pub data_dir: PathBuf,
    /// Its own runtime folder (`run/<name>`), for sockets.
    pub run_dir: PathBuf,
    /// The endpoints of what it needs, by name.
    pub needs: BTreeMap<String, Endpoint>,
}

/// What a [`Planner`] answers.
#[derive(Debug, Clone)]
pub struct Planned {
    pub plan: Plan,
    /// Where it is reached once ready.
    pub endpoint: Endpoint,
    /// Its restart policy, if it is supervised.
    pub restart: Option<crate::restart::Restart>,
    /// A long-lived process the engine watches (stays up by default), rather than a backend that sleeps.
    pub supervised: bool,
    /// How long its processes may take to be ready.
    pub ready_within: Duration,
}

/// Plans an instance's processes (the module pool in E2; fixed plans in tests).
pub trait Planner: Send + Sync + Debug {
    fn plan<'a>(&'a self, ctx: &'a PlanCtx) -> BoxFuture<'a, Result<Planned, String>>;

    /// Where the instance will be reached, said without starting it (its socket under `ctx.run_dir`, a declared
    /// address): a `Lazy` need is wired with this before it ever ran (Go computed it from the config). `None`: not
    /// known until it runs.
    fn endpoint(&self, ctx: &PlanCtx) -> Option<Endpoint> {
        let _ = ctx;
        None
    }
}

/// A whole stack.
#[derive(Debug, Clone)]
pub struct Stack {
    /// Its name (the project's).
    pub name: String,
    /// The file it came from, as errors name it (`doze.hcl`).
    pub config_name: String,
    /// The project's state folder under the doze home (`projects/<slug>`): `clusters/`, `run/`, `logs/`.
    pub data_dir: PathBuf,
    /// The stack's idle time; `None`: only services with a time of their own sleep.
    pub idle_timeout: Option<Duration>,
    pub services: Vec<Service>,
}

impl Stack {
    /// The data folder of one instance.
    pub fn cluster_dir(&self, name: &str) -> PathBuf {
        self.data_dir.join("clusters").join(name)
    }

    /// The stack's runtime folder (pidfiles, sockets).
    pub fn run_dir(&self) -> PathBuf {
        self.data_dir.join("run")
    }

    /// Looks a service up by name.
    pub fn service(&self, name: &str) -> Option<&Service> {
        self.services.iter().find(|s| s.name == name)
    }
}
