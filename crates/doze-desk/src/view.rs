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
        }
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
