//! What one running stack's daemon has said, folded as the Go hub's `apply` folded it. Each fold says which parts
//! of the stack it changed.

use crate::changed::Parts;
use crate::console::{ConsoleView, Rates};
use crate::convert;
use crate::events;
use crate::ring::LogRing;
use crate::view::StackView;
use doze_control::{ConsoleFrame, FollowEvent, Frame, InstanceView, Snapshot, Topic};
use doze_projects::StackKey;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// One running stack's state: the view pages read, and the log ring and the console rates behind it.
#[derive(Debug)]
pub(crate) struct StackState {
    pub(crate) view: StackView,
    pub(crate) ring: LogRing,
    rates: HashMap<String, Rates>,
    /// The tables a console's new subscription will replay, by instance (see [`ConsoleView::apply`]).
    replays: HashMap<String, Vec<String>>,
    /// When the hub first saw the daemon.
    first_seen: SystemTime,
}

impl StackState {
    pub(crate) fn new(key: StackKey, now: SystemTime) -> StackState {
        let mut view = StackView::new(key);
        view.running = true;
        view.since = Some(now);
        StackState {
            view,
            ring: LogRing::new(),
            rates: HashMap::new(),
            replays: HashMap::new(),
            first_seen: now,
        }
    }

    /// What the daemon said the stack is, after each connect: its name, folder, idle time and when it read
    /// doze.hcl. The instances come from the subscription's own snapshot, which follows it.
    pub(crate) fn snapshot(&mut self, s: &Snapshot) -> Parts {
        let v = &mut self.view;
        let before = (v.name.clone(), v.dir.clone(), v.idle_timeout, v.loaded);
        v.name.clone_from(&s.stack);
        v.dir.clone_from(&s.dir);
        v.idle_timeout = s.idle_timeout.to_std();
        v.loaded = convert::time(&s.loaded);
        // The daemon read doze.hcl when it started, which may be long before the hub saw it: "up" counts from then.
        v.since = Some(match v.loaded {
            Some(l) if l < self.first_seen => l,
            _ => self.first_seen,
        });
        if before == (v.name.clone(), v.dir.clone(), v.idle_timeout, v.loaded) {
            Parts::NONE
        } else {
            Parts::META
        }
    }

    /// What the follower said about its connection.
    pub(crate) fn link(&mut self, e: &FollowEvent, now: SystemTime) -> Parts {
        if self.view.link.on(e, now) {
            Parts::CONN
        } else {
            Parts::NONE
        }
    }

    /// Folds a topic frame in.
    pub(crate) fn apply(&mut self, f: &Frame, now: SystemTime) -> Parts {
        let was_live = self.view.link.is_live();
        self.view.link.frame(now);
        let conn = if was_live { Parts::NONE } else { Parts::CONN };
        conn | match f.known_topic() {
            Some(Topic::Instances) => self.instances(f),
            Some(Topic::Stats) => self.stats(f),
            Some(Topic::Journal) => self.journal(f),
            Some(Topic::Logs) => self.log(f),
            Some(Topic::Console) | None => Parts::NONE,
        }
    }

    fn instances(&mut self, f: &Frame) -> Parts {
        let list = Arc::make_mut(&mut self.view.instances);
        if f.snapshot {
            list.clone_from(&f.instances);
            list.sort_by(|a, b| a.name.cmp(&b.name));
        } else {
            for v in &f.instances {
                match list.iter_mut().find(|x| x.name == v.name) {
                    Some(slot) => slot.clone_from(v),
                    None => {
                        list.push(v.clone());
                        list.sort_by(|a, b| a.name.cmp(&b.name));
                    }
                }
            }
        }
        list.retain(|v| !f.removed.contains(&v.name));
        Parts::INSTANCES
    }

    fn stats(&mut self, f: &Frame) -> Parts {
        let at = convert::time(&f.at);
        let mut parts = Parts::NONE;
        for st in &f.stats {
            if Arc::make_mut(&mut self.view.series)
                .entry(st.name.clone())
                .or_default()
                .push(st, at)
            {
                parts |= Parts::STATS;
            }
            if let Some(row) = row_of(&mut self.view.instances, &st.name, st) {
                row.ram = st.ram;
                row.cpu = st.cpu;
                row.conns = st.conns;
                parts |= Parts::STATS;
            }
        }
        parts
    }

    fn journal(&mut self, f: &Frame) -> Parts {
        match &f.event {
            Some(e) if events::add(Arc::make_mut(&mut self.view.events), e) => Parts::JOURNAL,
            _ => Parts::NONE,
        }
    }

    fn log(&mut self, f: &Frame) -> Parts {
        match &f.log {
            Some(l) if self.ring.push(l) => {
                self.view.logs_added = self.ring.added("");
                Parts::LOGS
            }
            _ => Parts::NONE,
        }
    }

    /// A page watches the meters, or no page does.
    pub(crate) fn set_metered(&mut self, metered: bool) -> Parts {
        if self.view.metered == metered {
            return Parts::NONE;
        }
        self.view.metered = metered;
        Parts::CONN
    }

    /// What the project record says was started last time.
    pub(crate) fn set_record(&mut self, last_set: &[String]) -> Parts {
        if self.view.last_set == last_set {
            return Parts::NONE;
        }
        self.view.last_set = last_set.to_vec();
        Parts::META
    }

    /// A console starts being shown (an empty one until its spec arrives), or stops (forgetting its rows).
    pub(crate) fn set_console(&mut self, inst: &str, shown: bool) -> Parts {
        let consoles = &self.view.consoles;
        if consoles.contains_key(inst) == shown {
            return Parts::NONE;
        }
        let consoles = Arc::make_mut(&mut self.view.consoles);
        if shown {
            consoles.insert(inst.to_owned(), ConsoleView::default());
        } else {
            consoles.remove(inst);
            self.rates.remove(inst);
            self.replays.remove(inst);
        }
        Parts::CONSOLE
    }

    /// A console's frame.
    pub(crate) fn console(&mut self, inst: &str, f: &ConsoleFrame, now: Instant) -> Parts {
        if !self.view.consoles.contains_key(inst) {
            return Parts::NONE;
        }
        let replay = !f.rows.is_empty()
            && self.replays.get_mut(inst).is_some_and(|p| {
                let had = p.contains(&f.panel);
                p.retain(|x| x != &f.panel);
                had
            });
        let rates = self.rates.entry(inst.to_owned()).or_default();
        let changed = Arc::make_mut(&mut self.view.consoles)
            .get_mut(inst)
            .is_some_and(|c| c.apply(rates, f, now, replay));
        if changed { Parts::CONSOLE } else { Parts::NONE }
    }

    /// A console's subscription failed.
    pub(crate) fn console_failed(&mut self, inst: &str, error: &str) -> Parts {
        if !self.view.consoles.contains_key(inst) {
            return Parts::NONE;
        }
        let changed = Arc::make_mut(&mut self.view.consoles)
            .get_mut(inst)
            .is_some_and(|c| c.failed(error));
        if changed { Parts::CONSOLE } else { Parts::NONE }
    }

    /// The console follows another stream: that panel's rows start again, and the new subscription replays the
    /// tables held.
    pub(crate) fn refollow(&mut self, inst: &str, panel: &str) -> Parts {
        if let Some(c) = self.view.consoles.get(inst) {
            self.replays
                .insert(inst.to_owned(), c.tables.keys().cloned().collect());
        }
        let held = self
            .view
            .consoles
            .get(inst)
            .is_some_and(|c| c.streams.contains_key(panel));
        if !held {
            return Parts::NONE;
        }
        if let Some(c) = Arc::make_mut(&mut self.view.consoles).get_mut(inst) {
            c.streams.remove(panel);
        }
        Parts::CONSOLE
    }
}

/// The row of the instance a reading is for, if its meters differ from the reading's.
fn row_of<'a>(
    list: &'a mut Arc<Vec<InstanceView>>,
    name: &str,
    st: &doze_control::StatView,
) -> Option<&'a mut InstanceView> {
    let differs = list
        .iter()
        .any(|v| v.name == name && (v.ram != st.ram || v.cpu != st.cpu || v.conns != st.conns));
    if !differs {
        return None;
    }
    Arc::make_mut(list).iter_mut().find(|v| v.name == name)
}
