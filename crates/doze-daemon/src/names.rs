//! Names (core plan D2; Go's `daemon/bindplan.go`, `domains.go`, `ingress.go`, `listeners.go`): this stack's place
//! in `.doze`, the zone every doze program on the machine shares.
//!
//! With `defaults { names = true }` the daemon takes its stack's name in the shared registry, then each instance it
//! fronts is `<name>.<stack>.doze`: claimed with an address of its own where the machine is set up for it (so two
//! stacks' postgres both hold 5432), or resolving to 127.0.0.1 where it is not (then ports must differ). Where each
//! listener goes, its route names and what the module is told come from `doze-bind`'s plan; claiming, listening,
//! twinning and routing from its `bind`. An app's own domains are twinned onto its name, with the certificate the
//! person brought or one minted from the doze authority, which every process the stack runs is then told about. The
//! resolver and the front door are joined, not owned: whichever doze program holds them answers for all.
//!
//! Without names everything is on 127.0.0.1 by port, as before: the same plan, with nothing claimed.

use crate::front::Fronted;
use doze_bind::{Addressing, Bound, ListenerPlan, Plan};
use doze_module_proto::msg::instance::Address;
use doze_names::{Lease, Name, Registry, StackLease};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A line in the daemon's log.
macro_rules! say {
    ($($t:tt)*) => {{
        #[allow(clippy::print_stderr)]
        {
            eprintln!("doze daemon: {}", format_args!($($t)*));
        }
    }};
}

/// The owner this daemon signs the registry as (Go's: the same word, so either language's peers read it).
pub const OWNER: &str = "doze";

/// This stack in the zone.
pub struct Zone {
    reg: Registry,
    addressing: Addressing,
    stack: String,
    home: PathBuf,
    /// The project folder (absolute, symlinks resolved): the routes record keys projects by it.
    project: String,
    lease: Option<StackLease>,
    /// Per instance, which of its names another program holds (`"<host> is held by <owner> (pid <n>)"`).
    held: Mutex<BTreeMap<String, String>>,
    serving: Mutex<Option<Serving>>,
}

impl std::fmt::Debug for Zone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Zone")
            .field("stack", &self.stack)
            .field("addressing", &self.addressing)
            .finish_non_exhaustive()
    }
}

/// The resolver and the front door, while this daemon takes its turn at them.
struct Serving {
    resolver: doze_names::resolver::Resolver,
    door: doze_names::front_door::FrontDoor,
}

/// What binding one instance came to.
#[derive(Debug)]
pub enum BindError {
    /// Its own name is another program's: the stack does not start on top of it (Go's `claimAddress`).
    Held(String),
    /// A port something else listens on: said in the log, and the instance is not fronted.
    Busy(String),
    /// Anything else, said the same way.
    Other(String),
}

fn other(e: &doze_bind::Error) -> BindError {
    match &e.source {
        doze_names::Error::Io(io) if io.kind() == std::io::ErrorKind::AddrInUse => {
            BindError::Busy(e.to_string())
        }
        _ => BindError::Other(e.to_string()),
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Held(s) | BindError::Busy(s) | BindError::Other(s) => f.write_str(s),
        }
    }
}

impl Zone {
    /// The zone for a loaded config: joined (the stack's name claimed) when names are on, else a handle that claims
    /// nothing. Refuses a reserved stack name and one another project holds, as Go did.
    pub fn for_config(cfg: &doze_config::Config) -> Result<Zone, String> {
        let home = PathBuf::from(&cfg.home);
        let stack = cfg.stack();
        let reg = Registry::open(&home, OWNER);
        let project = project_dir(cfg);
        if !cfg.defaults.names {
            return Ok(Zone::new(reg, Addressing::Off, stack, home, project, None));
        }
        if doze_names::pool::reserved_stack(&stack) {
            return Err(format!(
                "a stack cannot be called {stack:?}: {stack}.doze is the machine-wide name of a standalone tool, and \
                 this stack's services would sit under it — set another `stack = \"…\"`"
            ));
        }
        let lease = reg.claim_stack(&stack, &project).map_err(|e| match e {
            doze_names::Error::StackTaken { dir, pid, .. } => format!(
                "stack name {stack:?} is already in use by {dir} (daemon pid {pid}) — set a unique `stack = \"…\"` \
                 in this project's doze.hcl"
            ),
            e => e.to_string(),
        })?;
        let addressing = if doze_names::zone_usable(&home, reg.net()) {
            say!("names: each service has an address of its own, sharing canonical ports");
            Addressing::Own
        } else {
            Addressing::Shared
        };
        Ok(Zone::new(
            reg,
            addressing,
            stack,
            home,
            project,
            Some(lease),
        ))
    }

    fn new(
        reg: Registry,
        addressing: Addressing,
        stack: String,
        home: PathBuf,
        project: String,
        lease: Option<StackLease>,
    ) -> Zone {
        Zone {
            reg,
            addressing,
            stack,
            home,
            project,
            lease,
            held: Mutex::default(),
            serving: Mutex::default(),
        }
    }

    /// A zone with names off (tests, and stacks built in code).
    pub fn off(home: &Path) -> Zone {
        let reg = Registry::open(home, OWNER);
        Zone::new(
            reg,
            Addressing::Off,
            String::new(),
            home.to_path_buf(),
            String::new(),
            None,
        )
    }

    pub fn addressing(&self) -> Addressing {
        self.addressing
    }

    /// The shared registry.
    pub fn registry(&self) -> &Registry {
        &self.reg
    }

    /// Where each fronted instance goes; every mistake at once, each naming what is involved.
    pub fn plan(&self, fronted: &[Fronted]) -> Result<Plan, String> {
        let decls: Vec<doze_bind::Decl> = fronted.iter().map(|f| f.decl.clone()).collect();
        doze_bind::plan(&self.stack, &decls, self.addressing).map_err(|errs| errs.join("\n"))
    }

    /// What `f`'s module is told, from the plan alone (the same whether or not binding it works).
    pub fn grant(&self, plan: &Plan, f: &Fronted, run_dir: &Path) -> crate::planners::Grant {
        let Some(i) = plan.instances.iter().find(|i| i.name == f.name) else {
            return crate::planners::Grant::default();
        };
        crate::planners::Grant {
            host: i.host.clone().unwrap_or_default(),
            listeners: i
                .listeners
                .iter()
                .map(|l| {
                    (
                        l.name.clone(),
                        i.granted(l, backend_of(f, &i.name, l, run_dir)),
                    )
                })
                .collect(),
        }
    }

    /// Claims, listens and routes one instance of `plan`. `run_dir` is the stack's run folder (a listener's socket
    /// backend is made in it).
    pub fn bind(&self, plan: &Plan, f: &Fronted, run_dir: &Path) -> Result<Bound, BindError> {
        let one = plan.only(&f.name);
        let backend = |inst: &str, l: &ListenerPlan| backend_of(f, inst, l, run_dir);
        let first = doze_bind::bind(&one, &self.reg, &backend);
        let bound = match first {
            Ok(b) => b,
            Err(e) => match &e.source {
                doze_names::Error::Held { host, pid, owner } => {
                    let who = format!("{host} is held by {owner} (pid {pid})");
                    if self.addressing == Addressing::Own {
                        return Err(BindError::Held(e.to_string()));
                    }
                    // On a shared address a lost name is not fatal (Go's `noteHeld`): the instance is reached by port.
                    say!("names: {who}");
                    self.note_held(&f.name, who);
                    let off = Plan {
                        addressing: Addressing::Off,
                        ..one.clone()
                    };
                    doze_bind::bind(&off, &self.reg, &backend).map_err(|e| other(&e))?
                }
                _ => return Err(other(&e)),
            },
        };
        if let Some(i) = bound.instances.get(&f.name) {
            for h in &i.held {
                say!("names: {h}");
                self.note_held(&f.name, h.clone());
            }
            for m in &i.moved {
                say!("{}: {m}", f.name);
            }
            for (l, b) in &i.listeners {
                if !b.granted.hosts.is_empty() {
                    say!(
                        "front door: http://{} → {} ({l})",
                        b.granted.hosts.join(", http://"),
                        b.public
                    );
                }
            }
        }
        Ok(bound)
    }

    /// Twins an app's own domains onto its name (`dev.acme.com` resolves where the app's name does and the front
    /// door sends it to the same listener), with the certificate the person brought. A domain another stack holds is
    /// said and skipped.
    pub fn domains(&self, f: &Fronted, bound: &Bound) -> Vec<Lease> {
        let mut out = Vec::new();
        let Some(i) = bound.instances.get(&f.name) else {
            return out;
        };
        let (Some(lease), Some(main)) =
            (&i.lease, i.listeners.get(doze_module_proto::network::MAIN))
        else {
            return out;
        };
        // Where its name's route goes (`doze_bind::bind`): the public address, which the network daemon translates.
        let target = main.public.to_string();
        for d in &f.domains {
            let twin = match lease.twin(&Name::domain(d)) {
                Ok(t) => t,
                Err(doze_names::Error::Held { host, pid, owner }) => {
                    let who = format!("{host} is held by {owner} (pid {pid})");
                    say!("domains: {who}");
                    self.note_held(&f.name, who);
                    continue;
                }
                Err(e) => {
                    say!("domains: could not register {d}: {e}");
                    continue;
                }
            };
            if let Err(e) = twin.route(&target) {
                say!("domains: {d}: {e}");
            }
            if !f.cert.is_empty()
                && let Err(e) = twin.certificate(&f.cert, &f.key)
            {
                say!("domains: {d}: {e}");
            }
            let shown = if doze_names::domains::wildcard(d) {
                format!("*.{}", doze_names::domains::route_domain(d))
            } else {
                d.clone()
            };
            say!("front door: https://{shown} → {}", f.name);
            out.push(twin);
        }
        out
    }

    /// Records the stack's declared domains (so a route no stack declares any more is pruned at the next `doze
    /// trust`).
    pub fn record_domains(&self, domains: &[String]) {
        if !self.addressing.named() {
            return;
        }
        let routes = doze_names::routes::Routes::open(&self.home);
        if let Err(e) = routes.record(&self.project, domains) {
            say!("domains: could not record them: {e}");
        }
    }

    /// Joins the resolver and the front door (names on only): this daemon answers for the whole zone while it holds
    /// them, and stands by while another program does.
    pub fn serve(&self) {
        if !self.addressing.named() {
            return;
        }
        let log: doze_names::resolver::Log = std::sync::Arc::new(|line| say!("{line}"));
        let mut rc = doze_names::resolver::ResolverConfig::from_registry(&self.reg);
        rc.log = log.clone();
        let addr = rc.addr.clone();
        let resolver = doze_names::resolver::serve(rc);
        let mut fc = doze_names::front_door::FrontConfig::for_zone(
            &self.reg,
            &doze_names::ca::ca_dir(&self.home),
        );
        fc.log = log;
        let door = doze_names::front_door::serve(fc);
        crate::settle::serving(&self.reg);
        say!(
            "names: *.{}.{} (zone on {addr})",
            doze_names::label(&self.stack),
            doze_names::SUFFIX
        );
        *crate::lock(&self.serving) = Some(Serving { resolver, door });
    }

    /// Leaves the stack's names resolving while it is down (`Registry::park`), so a client that asks between a
    /// `doze down` and the next `up` gets the address the name will have again (its connection refused meanwhile),
    /// never a "no such name" a cache may hold on to. Called as the daemon stops, before its names are released.
    pub fn park(&self) {
        if !self.addressing.named() {
            return;
        }
        match self.reg.park() {
            Ok(0) => {}
            Ok(n) => say!(
                "names: {n} name{} kept resolving while the stack is down",
                if n == 1 { "" } else { "s" }
            ),
            Err(e) => say!("names: could not keep the names resolving: {e}"),
        }
    }

    /// Leaves the resolver and the front door to the other programs, and gives the stack's name back.
    pub async fn close(&self) {
        let serving = crate::lock(&self.serving).take();
        if let Some(s) = serving {
            s.door.close().await;
            s.resolver.close().await;
        }
        if let Some(l) = &self.lease {
            let _ = l.release();
        }
    }

    pub(crate) fn note_held(&self, instance: &str, who: String) {
        crate::lock(&self.held).insert(instance.to_owned(), who);
    }

    /// Which of `instance`'s names another program holds, said for status and doctor.
    pub fn held(&self, instance: &str) -> Option<String> {
        crate::lock(&self.held).get(instance).cloned()
    }

    pub(crate) fn forget(&self, instance: &str) {
        crate::lock(&self.held).remove(instance);
    }

    /// The front door's URL for a routed host, as the registry has it now (port-less while a doze program holds :80).
    pub fn url_for(&self, host: &str) -> Option<String> {
        self.addressing
            .named()
            .then(|| self.reg.url_for(host))
            .flatten()
    }
}

/// Where the engine listens for one listener: the main listener's as decided for the instance (a module's socket file
/// or a loopback port), another's a socket file in its run folder named for the listener (Go's `<listener>.sock`),
/// or a loopback port chosen at each start where the engine cannot listen on a socket or the path is too long.
fn backend_of(f: &Fronted, inst: &str, l: &ListenerPlan, run_dir: &Path) -> Address {
    if l.name == doze_module_proto::network::MAIN {
        return f.main_backend.clone();
    }
    let any = Address::Tcp(doze_engine::modules::ANY_LOOPBACK.into());
    if l.tcp_backend || !cfg!(unix) {
        return any;
    }
    let p = run_dir.join(inst).join(format!("{}.sock", l.name));
    let p = p.to_string_lossy().into_owned();
    if p.len() < crate::planners::SOCKET_PATH_MAX {
        Address::Unix(p)
    } else {
        any
    }
}

/// The folder doze.hcl is in, absolute and with symlinks resolved (Go's `projectDir`).
fn project_dir(cfg: &doze_config::Config) -> String {
    let dir = PathBuf::from(&cfg.dir);
    std::fs::canonicalize(&dir)
        .unwrap_or(dir)
        .to_string_lossy()
        .into_owned()
}

/// The environment that points what the stack runs at the doze authority (Go's `caEnv`), when some app declares a
/// domain without a certificate of its own: the authority is made then (Go's `ensureDomains`), and the bundle holds
/// the machine's roots and it, since `SSL_CERT_FILE` replaces a client's store rather than adding to it. A stack
/// whose domains all bring their own files trusts nothing of doze's.
pub fn ca_env(cfg: &doze_config::Config) -> BTreeMap<String, String> {
    let wanted = cfg.defaults.names
        && cfg
            .instances
            .iter()
            .any(|i| i.enabled && !i.domains.is_empty() && i.cert.is_empty());
    if !wanted {
        return BTreeMap::new();
    }
    let dir = doze_names::ca::ca_dir(Path::new(&cfg.home));
    let ca = match doze_names::ca::Ca::load_or_create(&dir) {
        Ok(ca) => ca,
        Err(e) => {
            say!("domains: no certificate authority ({e}); https is off for the minted domains");
            return BTreeMap::new();
        }
    };
    match ca.ensure_bundle() {
        Ok(bundle) => doze_config::stack::ca_env(&ca.root_path(), &bundle),
        Err(e) => {
            say!("domains: could not write the trust bundle: {e}");
            BTreeMap::new()
        }
    }
}
