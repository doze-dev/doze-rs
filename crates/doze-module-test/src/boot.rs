//! The core's boot sequence, for tests: launch the module, decode the block, check its network, grant listeners, resolve
//! (downloads come from a local build directory), provision, plan, start, and converge when fresh.

use crate::local::{LocalBuild, LocalEngine};
use crate::run::{self, Started};
use crate::sandbox::Sandbox;
use doze_module_host::{Running, Services};
use doze_module_proto::msg::error::Error;
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::{Address, Instance, Listener, Toolchain};
use doze_module_proto::msg::run::{Pin, Provisioned, ResolveArgs, WithToolchain};
use doze_module_proto::network;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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

/// How [`Harness::boot_with`] boots.
#[derive(Debug, Clone, Copy, Default)]
pub struct Boot {
    /// The main listener's backend on loopback TCP, not a socket (Windows, or an engine's TCP path).
    pub tcp: bool,
    /// The data directory cloned from the module's template.
    pub template: bool,
}

/// A booted instance. Stop it with [`Booted::stop`].
#[derive(Debug)]
pub struct Booted {
    pub module: Arc<Running>,
    pub instance: Instance,
    pub toolchain: Toolchain,
    pub fresh: bool,
    /// What resolve pinned, as the core keeps them in doze.lock.
    pub pins: Vec<Pin>,
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
        self.module_with(None).await
    }

    /// Starts the module with other engines it fetches served from local folders.
    pub async fn module_extra(&self, extra: &[LocalEngine]) -> Result<Running, String> {
        self.launch(None, extra).await
    }

    /// Starts the module with `backends` serving its `wire.accept` hand-offs (a test of a wire filter boots an
    /// instance, then hands this module client connections for it).
    pub async fn module_with(
        &self,
        backends: Option<Arc<dyn doze_module_host::Backends>>,
    ) -> Result<Running, String> {
        self.launch(backends, &[]).await
    }

    async fn launch(
        &self,
        backends: Option<Arc<dyn doze_module_host::Backends>>,
        extra: &[LocalEngine],
    ) -> Result<Running, String> {
        let downloads = Arc::new(LocalBuild {
            engine: self.engine.clone(),
            bin_dir: self.bin_dir.clone(),
            full: self.full.clone(),
            extra: extra.to_vec(),
        });
        let services = Services {
            downloads,
            backends,
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
        self.boot_with(hcl, Boot::default()).await
    }

    /// The module, the sandbox and the decoded instance, its listeners granted (main on loopback TCP if `tcp`).
    async fn prepare(
        &self,
        hcl: &str,
        tcp: bool,
        extra: &[LocalEngine],
    ) -> Result<(Arc<Running>, Sandbox, Instance), String> {
        let sandbox = Sandbox::new().map_err(|e| e.to_string())?;
        let module = Arc::new(self.module_extra(extra).await?);
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
        let socket = module
            .manifest
            .backend_socket
            .as_ref()
            .map(|s| s.replace("{port}", &module.manifest.default_port.to_string()));
        instance.listeners = crate::grant::grant(&listeners, &sandbox, &instance, socket)?;
        if tcp && let Some(main) = instance.listeners.get_mut(network::MAIN) {
            let port = crate::sandbox::free_port().map_err(|e| e.to_string())?;
            main.backend = Address::Tcp(format!("127.0.0.1:{port}"));
        }
        Ok((module, sandbox, instance))
    }

    /// Boots one block as `how` says: with a TCP backend, or from the module's shared template (made, then cloned
    /// into the data directory before provision, as the core does for a first boot); converge runs on a fresh or a
    /// cloned data directory.
    pub async fn boot_with(&self, hcl: &str, how: Boot) -> Result<Booted, String> {
        self.boot_extra(hcl, how, &[]).await
    }

    /// [`Harness::boot_with`], with other engines the module fetches served from local folders.
    pub async fn boot_extra(
        &self,
        hcl: &str,
        how: Boot,
        extra: &[LocalEngine],
    ) -> Result<Booted, String> {
        let (module, sandbox, mut instance) = self.prepare(hcl, how.tcp, extra).await?;
        let args = ResolveArgs {
            instance: instance.clone(),
            target: target(),
            pins: vec![],
        };
        let resolved = module.resolve(&args).await.map_err(|e| render(&e))?;
        let (toolchain, pins) = (resolved.toolchain, resolved.pins);
        instance.toolchain = Some(toolchain.clone());
        if how.template {
            let tmpl = sandbox
                .root()
                .join("template")
                .to_string_lossy()
                .into_owned();
            module
                .template_ensure(&toolchain, &tmpl)
                .await
                .map_err(|e| render(&e))?;
            module
                .template_clone(&tmpl, &instance.data_dir)
                .await
                .map_err(|e| render(&e))?;
        }
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
        if (fresh || how.template) && module.manifest.caps & caps::CONVERGE != 0 {
            module.converge(&with).await.map_err(|e| render(&e))?;
        }
        Ok(Booted {
            module,
            instance,
            toolchain,
            fresh,
            pins,
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
            pins,
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
            pins,
            started,
            _sandbox: sandbox,
        };
        Ok((stops, awake))
    }

    /// What doze does when doze.hcl changes: stop the instance, decode the new block, resolve again with the lock's
    /// pins, provision, plan, start, and converge, in the same sandbox. The pins resolve returned replace the old ones
    /// of the same engine and spec.
    pub async fn redeclare(self, hcl: &str, grace: Duration) -> Result<(Stops, Booted), String> {
        let Booted {
            module,
            mut instance,
            pins: old,
            started,
            _sandbox: sandbox,
            ..
        } = self;
        let stops = started.stop(grace).await;
        let parsed =
            crate::hcl::parse_block(hcl, &module.manifest.schema).map_err(|e| e.to_string())?;
        instance.spec = parsed.spec;
        instance.version = parsed.version;
        module.decode(&instance).await.map_err(|e| render(&e))?;
        let args = ResolveArgs {
            instance: instance.clone(),
            target: target(),
            pins: old.clone(),
        };
        let resolved = module.resolve(&args).await.map_err(|e| render(&e))?;
        let mut pins: Vec<Pin> = old
            .into_iter()
            .filter(|o| {
                !resolved
                    .pins
                    .iter()
                    .any(|n| n.engine == o.engine && n.spec == o.spec)
            })
            .collect();
        pins.extend(resolved.pins);
        let toolchain = resolved.toolchain;
        instance.toolchain = Some(toolchain.clone());
        let with = WithToolchain {
            instance: instance.clone(),
            toolchain: toolchain.clone(),
        };
        let fresh = module.provision(&with).await.map_err(|e| render(&e))? == Provisioned::Fresh;
        let plan = module.plan(&with).await.map_err(|e| render(&e))?;
        let logs = sandbox.logs().map_err(|e| e.to_string())?;
        let started = run::start(&plan, &logs, Duration::from_secs(90)).await?;
        if module.manifest.caps & caps::CONVERGE != 0 {
            module.converge(&with).await.map_err(|e| render(&e))?;
        }
        let awake = Booted {
            module,
            instance,
            toolchain,
            fresh,
            pins,
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
        // A test's relative paths (a seed file) are relative to the crate under test, where cargo runs it.
        dir: std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
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
