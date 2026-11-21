//! The hub: every stack on this machine, running or stopped, and the one way the rest of the app reads them (Go
//! `internal/desk/hub`). Nothing else in the app talks to a daemon's subscription.

mod refresh;
mod verbs;
mod watch;

pub use verbs::VERB_TIMEOUT;
pub use watch::{ConsoleWatch, Watch};

use crate::changed::{Batch, Changed};
use crate::clock::Clock;
use crate::coalesce::{self, Coalescer, Input, Policy};
use crate::name::name_from_key;
use crate::ring::LogRing;
use crate::stack::{Ctx, Stack, lock};
use crate::view::StackView;
use async_channel::{Receiver, Sender};
use doze_control::{Backoff, Client, LogFrame, Topic};
use doze_projects::{Project, StackKey};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

type DialFn = dyn Fn(&StackKey, &Path) -> Client + Send + Sync;

/// How a hub is set up. [`Options::new`] is a real hub on a doze home.
#[derive(Clone)]
pub struct Options {
    /// doze's folder: `$DOZE_HOME`, else `~/.doze`.
    pub home: PathBuf,
    /// The client for a stack's control socket. Tests point it at a fixture daemon.
    pub dial: Arc<DialFn>,
    /// How often [`Hub::start`] looks for stacks that appeared or went away.
    pub every: Duration,
    pub clock: Clock,
    /// How long a follower waits before it dials again.
    pub backoff: Backoff,
    /// How often the UI hears of changes.
    pub policy: Policy,
}

impl fmt::Debug for Options {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("home", &self.home)
            .field("every", &self.every)
            .field("clock", &self.clock)
            .field("backoff", &self.backoff)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl Options {
    /// A hub on the doze home `home`, looking every two seconds, dialling each stack's socket as the `app`.
    pub fn new(home: impl Into<PathBuf>) -> Options {
        Options {
            home: home.into(),
            dial: Arc::new(|_, sock| Client::socket(sock).named("app")),
            every: Duration::from_secs(2),
            clock: Clock::system(),
            backoff: Backoff::default(),
            policy: Policy::default(),
        }
    }
}

/// Every stack, watched. Cheap to clone; the last clone dropped stops every stack's thread.
#[derive(Debug, Clone)]
pub struct Hub {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    o: Options,
    inner: Mutex<Inner>,
    input: Sender<Input>,
    updates: Receiver<Batch>,
    /// Closed when the hub goes, which ends the discovery thread.
    _alive: Sender<()>,
    alive: Receiver<()>,
}

#[derive(Debug, Default)]
struct Inner {
    stacks: BTreeMap<StackKey, Stack>,
    stopped: BTreeMap<StackKey, Project>,
    /// Refresh has run once; the first run always reports.
    refreshed: bool,
    /// How many pages watch each optional topic of each stack, kept here so a stack that appears, or whose daemon
    /// is replaced, streams what is wanted from the start.
    wants: HashMap<StackKey, HashMap<Topic, usize>>,
    /// How many pages show each instance's console, by stack.
    consoles: HashMap<StackKey, BTreeMap<String, usize>>,
    /// No window shows (closed, minimised, behind another, on a sleeping display): the meters are not streamed,
    /// whatever pages watch (Go: a hidden page blurs). Logs are, into the ring the pages read.
    hidden: bool,
}

impl Hub {
    /// A hub that looks for stacks only when [`Hub::refresh`] is called.
    pub fn new(o: Options) -> Hub {
        let (input, rx) = async_channel::unbounded();
        let (out, updates) = async_channel::bounded(1);
        let policy = o.policy;
        let _ = thread::Builder::new()
            .name("doze-desk coalesce".into())
            .spawn(move || coalesce::run(Coalescer::new(policy), &rx, &out));
        let (alive_tx, alive) = async_channel::bounded(1);
        Hub {
            shared: Arc::new(Shared {
                o,
                inner: Mutex::new(Inner::default()),
                input,
                updates,
                _alive: alive_tx,
                alive,
            }),
        }
    }

    /// A hub that looks for stacks now and then every [`Options::every`], until it is dropped.
    pub fn start(o: Options) -> Hub {
        let every = o.every;
        let hub = Hub::new(o);
        hub.refresh();
        let weak = Arc::downgrade(&hub.shared);
        let alive = hub.shared.alive.clone();
        let _ = thread::Builder::new()
            .name("doze-desk discover".into())
            .spawn(move || {
                // `alive` never carries a message: waiting on it is a sleep that ends early when the hub goes.
                while crate::select::block_on(alive.recv(), Some(Instant::now() + every)).is_none()
                {
                    match weak.upgrade() {
                        Some(shared) => Hub { shared }.refresh(),
                        None => return,
                    }
                }
            });
        hub
    }

    /// The batches of changes, at most one a frame (see [`Policy`]). One consumer: the UI.
    pub fn updates(&self) -> &Receiver<Batch> {
        &self.shared.updates
    }

    /// The UI was shown or hidden; while hidden it hears of nothing (unless the policy says otherwise).
    pub fn set_visible(&self, visible: bool) {
        let _ = self.shared.input.try_send(Input::Visible(visible));
        let mut inner = lock(&self.shared.inner);
        if inner.hidden == !visible {
            return;
        }
        inner.hidden = !visible;
        for (key, s) in &inner.stacks {
            s.command(crate::stack::Cmd::Topics(refresh::wanted(&inner, key)));
        }
    }

    /// Whether the UI shows (the last [`Hub::set_visible`]).
    pub fn visible(&self) -> bool {
        !lock(&self.shared.inner).hidden
    }

    /// What time the hub says it is: the wall clock, or a test's or a fixture's, so a page's "up 14m" or countdown agrees
    /// with the hub.
    pub fn now(&self) -> std::time::SystemTime {
        self.shared.o.clock.now()
    }

    /// doze's folder: where projects, and the app's own prefs, live.
    pub fn home(&self) -> &Path {
        &self.shared.o.home
    }

    fn changed(&self, c: Changed) {
        let _ = self.shared.input.try_send(Input::Changed(c));
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            clock: self.shared.o.clock.clone(),
            backoff: self.shared.o.backoff,
            out: self.shared.input.clone(),
        }
    }

    /// Every stack known, running ones first, each group by name (Go `Stacks`).
    pub fn stacks(&self) -> Vec<StackView> {
        let inner = lock(&self.shared.inner);
        let mut out: Vec<StackView> = inner.stacks.values().map(running_view).collect();
        out.extend(inner.stopped.iter().map(|(key, p)| stopped_view(key, p)));
        drop(inner);
        out.sort_by(|a, b| {
            b.running
                .cmp(&a.running)
                .then_with(|| doze_display::natural_cmp(&a.name, &b.name))
                .then_with(|| a.key.cmp(&b.key))
        });
        out
    }

    /// One stack, running or stopped.
    pub fn stack(&self, key: &StackKey) -> Option<StackView> {
        let inner = lock(&self.shared.inner);
        if let Some(s) = inner.stacks.get(key) {
            return Some(running_view(s));
        }
        inner.stopped.get(key).map(|p| stopped_view(key, p))
    }

    /// Reads a running stack's log ring; `None` for a stack that is not running.
    pub fn with_logs<R>(&self, key: &StackKey, f: impl FnOnce(&LogRing) -> R) -> Option<R> {
        let inner = lock(&self.shared.inner);
        inner.stacks.get(key).map(|s| s.with_ring(f))
    }

    /// What an instance (`""`: every instance) printed, oldest first, as far back as the hub keeps (Go `Logs`).
    /// Copies: a page drawing the ring reads it in place with [`Hub::with_logs`].
    pub fn logs(&self, key: &StackKey, instance: &str) -> Vec<LogFrame> {
        self.with_logs(key, |r| r.tail(instance, usize::MAX))
            .unwrap_or_default()
    }

    /// How many lines an instance (`""`: every instance) ever added (Go `LogsVersion`).
    pub fn logs_version(&self, key: &StackKey, instance: &str) -> u64 {
        self.with_logs(key, |r| r.added(instance)).unwrap_or(0)
    }
}

fn running_view(s: &Stack) -> StackView {
    let mut v = s.view();
    if v.name.is_empty() && v.dir.is_empty() {
        // A daemon from an older doze says neither: its state folder still carries the name.
        v.name = name_from_key(v.key.dir());
    }
    v
}

fn stopped_view(key: &StackKey, p: &Project) -> StackView {
    let mut v = StackView::new(key.clone());
    v.name.clone_from(&p.name);
    v.dir = Path::new(&p.config)
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    v.last_up = p.last_up.map(std::time::SystemTime::from);
    v.last_down = p.last_down.map(std::time::SystemTime::from);
    v.last_set.clone_from(&p.last_set);
    v
}
