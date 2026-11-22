//! One stack as a page sees it.

use crate::conn::Link;
use crate::console::ConsoleView;
use crate::series::Series;
use doze_control::{Event, InstanceView};
use doze_projects::StackKey;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// One stack (Go `hub.StackView`). Running stacks come from their daemon; stopped ones from the project record doze
/// wrote when they were up. The heavy parts are shared, so taking a view is cheap; comparing two says whether a
/// page must redraw.
#[derive(Debug, Clone, PartialEq)]
pub struct StackView {
    /// Identifies the stack across refreshes: its state folder.
    pub key: StackKey,
    /// The stack's name.
    pub name: String,
    /// The folder its doze.hcl is in.
    pub dir: String,
    /// How long a service sits idle before it sleeps.
    pub idle_timeout: Duration,
    /// A daemon holds the control socket.
    pub running: bool,
    /// The connection to a running daemon.
    pub link: Link,
    /// A page watches this stack's meters, so a frame is due every second and silence means something.
    pub metered: bool,
    /// When the daemon read doze.hcl; `None` from an older daemon.
    pub loaded: Option<SystemTime>,
    /// The stack as last seen, in name order.
    pub instances: Arc<Vec<InstanceView>>,
    /// The journal kept, oldest first.
    pub events: Arc<Vec<Event>>,
    /// Each instance's meters over time, while a page watches stats.
    pub series: Arc<BTreeMap<String, Series>>,
    /// The module consoles pages are showing, by instance.
    pub consoles: Arc<BTreeMap<String, ConsoleView>>,
    /// How many log lines were ever added (the logs page's version); the lines are read with [`crate::Hub::logs`].
    pub logs_added: u64,
    /// From the project record, for a stopped stack.
    pub last_up: Option<SystemTime>,
    pub last_down: Option<SystemTime>,
    /// What `doze up` started last time, from the project record; empty means everything.
    pub last_set: Vec<String>,
    /// When this stack's daemon started (when it read doze.hcl), or when the hub first saw it: "up 14m".
    pub since: Option<SystemTime>,
    /// This app has an action in flight on the stack (up, down, restart, a wake that starts it; [`crate::Hub::set_acting`]):
    /// its daemon starting or exiting under it is the action's own doing, not a daemon that stopped answering.
    pub acting: bool,
}

impl StackView {
    /// A stack with nothing known yet but its key.
    pub fn new(key: StackKey) -> StackView {
        StackView {
            key,
            name: String::new(),
            dir: String::new(),
            idle_timeout: Duration::ZERO,
            running: false,
            link: Link::default(),
            metered: false,
            loaded: None,
            instances: Arc::default(),
            events: Arc::default(),
            series: Arc::default(),
            consoles: Arc::default(),
            logs_added: 0,
            last_up: None,
            last_down: None,
            last_set: Vec::new(),
            since: None,
            acting: false,
        }
    }

    /// The stack runs and its daemon is not answering: down for [`crate::LOST_AFTER`], with no action of this app on
    /// it. What Needs you, the Library's pill, the frozen rows and the menu bar all go by.
    pub fn daemon_lost(&self, now: SystemTime) -> bool {
        self.running && !self.acting && self.link.lost(now)
    }

    /// The connection as the pages say it: a drop that is not (yet) [`StackView::daemon_lost`] is connecting.
    pub fn conn_shown(&self, now: SystemTime) -> crate::Conn {
        match self.link.shown(now) {
            crate::Conn::Down if !self.daemon_lost(now) => crate::Conn::Connecting,
            c => c,
        }
    }

    /// Every service of a running, answering stack is asleep: it is up, and the next connection wakes what it needs.
    /// Tasks, and services turned off or paused, do not count either way.
    pub fn all_asleep(&self) -> bool {
        use doze_display::{Kind, Look, look};
        if !self.running || !self.link.is_live() {
            return false;
        }
        let snap = self.words();
        let mut asleep = 0;
        for i in snap.instances.iter().filter(|i| i.kind != Kind::Task) {
            match look(i) {
                Look::Asleep => asleep += 1,
                Look::Off | Look::Paused => {}
                _ => return false,
            }
        }
        asleep > 0
    }

    /// The instance named `name`.
    pub fn instance(&self, name: &str) -> Option<&InstanceView> {
        self.instances.iter().find(|v| v.name == name)
    }

    /// The stack as the words read it.
    pub fn words(&self) -> doze_display::Snapshot {
        crate::convert::snapshot(self.idle_timeout, &self.instances)
    }
}

/// What a stack that is up with every service asleep reads ([`StackView::all_asleep`]): not running, not stopped.
pub const ALL_ASLEEP: &str = "asleep · wakes on use";
