//! The core's boot sequence, for tests: launch the module, decode the block, check its network, grant listeners, resolve
//! (downloads come from a local build directory), provision, plan, start, and converge when fresh.

use crate::run::{self, Started};
use crate::sandbox::Sandbox;
use doze_module_host::{BoxFuture, Downloads, Running, Services};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::{Address, Instance, Listener, Toolchain};
use doze_module_proto::msg::run::{Ensure, Ensured, Provisioned, ResolveArgs, WithToolchain};
use doze_module_proto::network;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Engine builds come from one local directory (`DOZE_<ENGINE>_BINDIR` in a module's own tests).
struct LocalBuild {
    bin_dir: PathBuf,
    full: String,
}

impl Downloads for LocalBuild {
    fn resolve_major<'a>(
        &'a self,
        _e: &'a str,
        _m: &'a str,
    ) -> BoxFuture<'a, Result<String, Error>> {
        Box::pin(async move { Ok(self.full.clone()) })
    }
    fn ensure<'a>(&'a self, _r: &'a Ensure) -> BoxFuture<'a, Result<Ensured, Error>> {
        Box::pin(async move {
            Ok(Ensured {
                bin_dir: self.bin_dir.to_string_lossy().into_owned(),
                digest: "sha256:local".into(),
            })
        })
    }
}

/// A module under test.
#[derive(Debug, Clone)]
pub struct Harness {
    /// The module binary.
    pub program: PathBuf,
    pub engine: String,
    /// Where the engine's own binaries are, and the version they are.
    pub bin_dir: PathBuf,
    pub full: String,
}

/// A booted instance. Stop it with [`Booted::stop`].
#[derive(Debug)]
pub struct Booted {
    pub module: Arc<Running>,
    pub instance: Instance,
    pub toolchain: Toolchain,
    pub fresh: bool,
    started: Started,
    _sandbox: Sandbox,
}

/// The target triple of this build, as the host would report it.
pub fn target() -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let rest = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(windows) {
        "pc-windows-msvc"
    } else {
        "unknown-linux-gnu"
    };
    format!("{arch}-{rest}")
}

fn render(e: &Error) -> String {
    let mut s = e.message.clone();
    for d in &e.diagnostics {
        s.push_str(&format!("\n  at {:?}: {}", d.path, d.message));
        if let Some(h) = &d.hint {
            s.push_str(&format!(" ({h})"));
        }
    }
    s
}

impl Harness {
    /// Starts the module on its own.
    pub async fn module(&self) -> Result<Running, String> {
        let downloads = Arc::new(LocalBuild {
            bin_dir: self.bin_dir.clone(),
            full: self.full.clone(),
        });
        let services = Services {
            downloads,
            backends: None,
            notices: None,
        };
        doze_module_host::launch(&self.engine, &self.program, services)
            .await
            .map_err(|e| render(&e))
    }

    /// Boots the module's own published example: the example must work, or the registry would publish a lie.
    pub async fn boot_example(&self) -> Result<Booted, String> {
        let example = self.module().await?.manifest.example;
        self.boot(&example).await
    }

    /// Boots one block of literal HCL in a fresh sandbox.
    pub async fn boot(&self, hcl: &str) -> Result<Booted, String> {
        let sandbox = Sandbox::new().map_err(|e| e.to_string())?;
        let module = Arc::new(self.module().await?);
        let parsed =
            crate::hcl::parse_block(hcl, &module.manifest.schema).map_err(|e| e.to_string())?;
        if parsed.engine != self.engine {
            return Err(format!(
                "the block is `{}`, the module is `{}`",
                parsed.engine, self.engine
            ));
        }
        let mut instance = new_instance(parsed, &sandbox)?;
        module.decode(&instance).await.map_err(|e| render(&e))?;
        let listeners = checked_network(&module, &instance).await?;
        instance.listeners = crate::grant::grant(&listeners, &sandbox, &instance)?;
        let args = ResolveArgs {
            instance: instance.clone(),
            target: target(),
            pins: vec![],
        };
        let toolchain = module
            .resolve(&args)
            .await
            .map_err(|e| render(&e))?
            .toolchain;
        instance.toolchain = Some(toolchain.clone());
        let with = WithToolchain {
            instance: instance.clone(),
            toolchain: toolchain.clone(),
        };
        let fresh = module.provision(&with).await.map_err(|e| render(&e))? == Provisioned::Fresh;
        let plan = module.plan(&with).await.map_err(|e| render(&e))?;
        let started = run::start(
            &plan,
            &sandbox.logs().map_err(|e| e.to_string())?,
            Duration::from_secs(90),
        )
        .await?;
        if fresh && module.manifest.caps & caps::CONVERGE != 0 {
            module.converge(&with).await.map_err(|e| render(&e))?;
        }
        Ok(Booted {
            module,
            instance,
            toolchain,
            fresh,
            started,
            _sandbox: sandbox,
        })
    }
}

impl Booted {
    /// Where the instance's main backend listens.
    pub fn backend(&self) -> Option<Address> {
        self.instance
            .listeners
            .get(network::MAIN)
            .map(|g| g.backend.clone())
    }

    /// A process's output so far.
    pub fn log(&self, process: &str) -> String {
        self.started.log(process)
    }

    /// Stops the instance; how each process ended.
    pub async fn stop(
        self,
        grace: Duration,
    ) -> Vec<(String, std::io::Result<doze_os::tree::Stopped>)> {
        self.started.stop(grace).await
    }

    /// What doze does to an idle instance and then to its next client: stop it gracefully, then provision (it is
    /// already there), plan and start it again in the same sandbox. How each process ended, and the instance awake.
    pub async fn sleep_and_wake(self, grace: Duration) -> Result<(Stops, Booted), String> {
        let Booted {
            module,
            instance,
            toolchain,
            started,
            _sandbox: sandbox,
            ..
        } = self;
        let stops = started.stop(grace).await;
        let with = WithToolchain {
            instance: instance.clone(),
            toolchain: toolchain.clone(),
        };
        let fresh = module.provision(&with).await.map_err(|e| render(&e))? == Provisioned::Fresh;
        let plan = module.plan(&with).await.map_err(|e| render(&e))?;
        let logs = sandbox.logs().map_err(|e| e.to_string())?;
        let started = run::start(&plan, &logs, Duration::from_secs(90)).await?;
        let awake = Booted {
            module,
            instance,
            toolchain,
            fresh,
            started,
            _sandbox: sandbox,
        };
        Ok((stops, awake))
    }
}

/// How each process of an instance ended.
pub type Stops = Vec<(String, std::io::Result<doze_os::tree::Stopped>)>;

fn new_instance(parsed: crate::hcl::Parsed, sandbox: &Sandbox) -> Result<Instance, String> {
    Ok(Instance {
        data_dir: sandbox
            .data_dir(&parsed.name)
            .to_string_lossy()
            .into_owned(),
        socket_dir: sandbox
            .socket_dir(&parsed.name)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned(),
        host: format!("{}.test.doze", parsed.name),
        name: parsed.name,
        engine: parsed.engine,
        version: parsed.version,
        spec: parsed.spec,
        listeners: BTreeMap::new(),
        deps: vec![],
        toolchain: None,
    })
}

/// The module's listeners, which must be valid and the same every time for the same instance (the core caches them).
async fn checked_network(module: &Running, instance: &Instance) -> Result<Vec<Listener>, String> {
    let listeners = module.network(instance).await.map_err(|e| render(&e))?;
    if module.network(instance).await.map_err(|e| render(&e))? != listeners {
        return Err(
            "network answered differently for the same instance: it must be a pure function".into(),
        );
    }
    let errs = network::validate(&listeners);
    if !errs.is_empty() {
        return Err(format!("invalid listeners: {}", errs.join("; ")));
    }
    Ok(listeners)
}
