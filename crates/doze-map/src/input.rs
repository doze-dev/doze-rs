//! What the Map is drawn from: the stack's instances and what each needs, as doze.hcl's
//! references give them, read into calls (app to app), uses (app to a module or service) and
//! setup tasks.

use std::collections::HashMap;

/// The block that declared an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Kind {
    /// Your code: a card in the calls, or in the lane when nothing refers to it.
    #[default]
    App,
    /// Runs once before an app: a mark on the app's card, or a card of its own while it runs or
    /// after it failed.
    Task,
    /// From the doze registry: a card in the shared band.
    Module,
    /// Brought in by you: a card in the shared band.
    Service,
}

impl Kind {
    pub(crate) fn shared(self) -> bool {
        matches!(self, Kind::Module | Kind::Service)
    }
}

/// One instance of the stack, in display order (the order the input lists them is the order the
/// Map breaks ties by).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Instance {
    /// Its name, unique in the stack.
    pub name: String,
    /// The block that declared it.
    pub kind: Kind,
    /// The instances it refers to, in the order doze.hcl does.
    pub needs: Vec<String>,
    /// The run command (it gives the card's language badge).
    pub run: String,
    /// The module's engine (`kafka`, `postgres` …): the lane's caption reads it.
    pub engine: String,
    /// A task that is running or whose last run failed: a card rather than a mark.
    pub busy: bool,
    /// The group it belongs to, when the caller knows one (the instance's `group`, or the label
    /// of the `for_each` block that stamped it). See [`crate::Grouping`].
    pub group: Option<String>,
    /// How many instances this one stands for: 1, or more for a collapsed group's card.
    pub members: u32,
}

impl Instance {
    /// An instance of `kind` named `name` that needs `needs`.
    #[must_use]
    pub fn new(name: &str, kind: Kind, needs: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            kind,
            needs: needs.iter().map(|s| (*s).to_string()).collect(),
            members: 1,
            ..Self::default()
        }
    }
}

/// The modules an app consumes events from: an app nothing calls that uses one works from events.
const EVENT_ENGINES: [&str; 6] = ["kafka", "redpanda", "nats", "rabbitmq", "sqs", "pulsar"];

/// The stack read as calls and uses, by index into the instances.
#[derive(Debug)]
pub(crate) struct Flow<'a> {
    pub(crate) ins: &'a [Instance],
    /// Every app, in display order.
    pub(crate) apps: Vec<usize>,
    /// Modules and services, in display order.
    pub(crate) shared: Vec<usize>,
    /// App to the apps and task cards it calls, in reference order; and back.
    pub(crate) calls: Vec<Vec<usize>>,
    pub(crate) callers: Vec<Vec<usize>>,
    /// App to the shared it uses, in reference order; and back, in display order.
    pub(crate) uses: Vec<Vec<usize>>,
    pub(crate) users: Vec<Vec<usize>>,
    /// App to the tasks it lists in `after`.
    pub(crate) tasks: Vec<Vec<usize>>,
    /// Tasks drawn as cards.
    pub(crate) carded: Vec<bool>,
}

impl<'a> Flow<'a> {
    /// Sorts every reference into a call, a use or a task. A name listed twice counts once (the
    /// first); a need that names nothing, or the instance itself, is skipped.
    pub(crate) fn read(ins: &'a [Instance]) -> Self {
        let n = ins.len();
        let mut by_name: HashMap<&str, usize> = HashMap::with_capacity(n);
        for (i, v) in ins.iter().enumerate() {
            by_name.entry(v.name.as_str()).or_insert(i);
        }
        let mut f = Flow {
            ins,
            apps: Vec::new(),
            shared: Vec::new(),
            calls: vec![Vec::new(); n],
            callers: vec![Vec::new(); n],
            uses: vec![Vec::new(); n],
            users: vec![Vec::new(); n],
            tasks: vec![Vec::new(); n],
            carded: vec![false; n],
        };
        for (i, v) in ins.iter().enumerate() {
            if by_name.get(v.name.as_str()) != Some(&i) {
                continue;
            }
            match v.kind {
                Kind::App => f.apps.push(i),
                k if k.shared() => f.shared.push(i),
                _ => {}
            }
        }
        for a in f.apps.clone() {
            f.read_needs(a, &by_name);
        }
        f
    }

    fn read_needs(&mut self, a: usize, by_name: &HashMap<&str, usize>) {
        let Some(v) = self.ins.get(a) else { return };
        let mut seen = Vec::new();
        for need in &v.needs {
            let Some(&d) = by_name.get(need.as_str()) else {
                continue;
            };
            if d == a || seen.contains(&d) {
                continue;
            }
            seen.push(d);
            let Some(dv) = self.ins.get(d) else { continue };
            match dv.kind {
                Kind::App => link(&mut self.calls, &mut self.callers, a, d),
                k if k.shared() => link(&mut self.uses, &mut self.users, a, d),
                _ => {
                    push(&mut self.tasks, a, d);
                    if dv.busy {
                        // Running or failed: a card of its own, after the app.
                        set(&mut self.carded, d);
                        link(&mut self.calls, &mut self.callers, a, d);
                    }
                }
            }
        }
    }

    pub(crate) fn name(&self, i: usize) -> &'a str {
        self.ins.get(i).map_or("", |v| v.name.as_str())
    }
    pub(crate) fn inst(&self, i: usize) -> Option<&'a Instance> {
        self.ins.get(i)
    }
    pub(crate) fn calls(&self, i: usize) -> &[usize] {
        list(&self.calls, i)
    }
    pub(crate) fn callers(&self, i: usize) -> &[usize] {
        list(&self.callers, i)
    }
    pub(crate) fn uses(&self, i: usize) -> &[usize] {
        list(&self.uses, i)
    }
    pub(crate) fn users(&self, i: usize) -> &[usize] {
        list(&self.users, i)
    }
    pub(crate) fn is_carded(&self, i: usize) -> bool {
        self.carded.get(i).copied().unwrap_or(false)
    }
    /// The app has tasks folded into a mark on its card (not drawn as cards).
    pub(crate) fn folds(&self, i: usize) -> bool {
        list(&self.tasks, i).iter().any(|&t| !self.is_carded(t))
    }
    /// Every app's every use: the lines the band saves drawing.
    pub(crate) fn uses_all(&self) -> usize {
        self.apps.iter().map(|&a| self.uses(a).len()).sum()
    }
    /// Uses an event engine.
    pub(crate) fn on_events(&self, app: usize) -> bool {
        self.uses(app).iter().any(|&s| {
            self.inst(s)
                .is_some_and(|v| EVENT_ENGINES.contains(&v.engine.as_str()))
        })
    }
    /// The carded tasks, by name (the Go map's order for them).
    pub(crate) fn carded_by_name(&self) -> Vec<usize> {
        let mut out: Vec<usize> = (0..self.ins.len()).filter(|&i| self.is_carded(i)).collect();
        out.sort_by(|&a, &b| self.name(a).cmp(self.name(b)));
        out
    }
}

pub(crate) fn list(v: &[Vec<usize>], i: usize) -> &[usize] {
    v.get(i).map_or(&[], Vec::as_slice)
}

fn push(v: &mut [Vec<usize>], i: usize, x: usize) {
    if let Some(l) = v.get_mut(i) {
        l.push(x);
    }
}

fn set(v: &mut [bool], i: usize) {
    if let Some(b) = v.get_mut(i) {
        *b = true;
    }
}

fn link(fwd: &mut [Vec<usize>], back: &mut [Vec<usize>], a: usize, b: usize) {
    push(fwd, a, b);
    push(back, b, a);
}
