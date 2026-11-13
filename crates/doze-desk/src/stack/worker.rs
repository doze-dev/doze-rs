//! A stack's thread. It owns the follower of the long-lived subscription (instances and the journal, stats and logs
//! while wanted) and one follower per console a page shows, folds what they say into the stack's state, and says
//! what changed. The followers reconnect by themselves; the thread only tells them what to follow.

use super::{Cmd, Ctx, StackState, Wanted, lock};
use crate::changed::{Changed, Parts};
use crate::coalesce::Input;
use crate::ring::BACKLOG;
use crate::select::{self, Woke};
use async_channel::Receiver;
use doze_control::{Client, Follow, FollowEvent, JOURNAL_FROM_START, Subscription, Topic};
use doze_projects::StackKey;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// How many queued frames are folded before the change is said: a flood of log lines is one change, not thousands.
const DRAIN: usize = 512;
/// How long the stack's description may take after a connect.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);

/// One console a page shows, and the stream it follows.
#[derive(Debug)]
struct ConsoleSub {
    inst: String,
    panel: String,
    sector: String,
    follow: Follow,
}

#[derive(Debug)]
pub(super) struct Worker {
    key: StackKey,
    state: Arc<Mutex<StackState>>,
    client: Client,
    wanted: Wanted,
    cmds: Receiver<Cmd>,
    ctx: Ctx,
    main: Follow,
    consoles: Vec<ConsoleSub>,
    turn: usize,
}

/// What to ask for: instances and the journal always; stats and logs while a page wants them (Go `subscription`).
/// The follower resumes the journal after the last event it delivered.
pub(crate) fn subscription(w: Wanted) -> Subscription {
    let mut topics = vec![Topic::Instances, Topic::Journal];
    topics.extend(w.stats.then_some(Topic::Stats));
    topics.extend(w.logs.then_some(Topic::Logs));
    Subscription {
        topics,
        since: JOURNAL_FROM_START,
        backlog: if w.logs { BACKLOG } else { 0 },
        ..Subscription::default()
    }
}

fn console_subscription(inst: &str, panel: &str, sector: &str) -> Subscription {
    Subscription {
        topics: vec![Topic::Console],
        names: vec![inst.to_owned()],
        panel: panel.to_owned(),
        selector: sector.to_owned(),
        ..Subscription::default()
    }
}

impl Worker {
    pub(super) fn new(
        state: Arc<Mutex<StackState>>,
        client: Client,
        wanted: Wanted,
        cmds: Receiver<Cmd>,
        ctx: Ctx,
    ) -> Worker {
        let main = client.follow(subscription(wanted), ctx.backoff);
        let key = lock(&state).view.key.clone();
        Worker {
            key,
            state,
            client,
            wanted,
            cmds,
            ctx,
            main,
            consoles: Vec::new(),
            turn: 0,
        }
    }

    pub(super) fn run(mut self) {
        loop {
            let woke = {
                let events: Vec<&Receiver<FollowEvent>> = std::iter::once(self.main.events())
                    .chain(self.consoles.iter().map(|c| c.follow.events()))
                    .collect();
                select::any(&self.cmds, &events, self.turn)
            };
            self.turn = self.turn.wrapping_add(1);
            match woke {
                Woke::Command(None | Some(Cmd::Stop)) => return,
                Woke::Command(Some(c)) => self.command(c),
                Woke::Event(0, Some(e)) => self.main_events(e),
                Woke::Event(0, None) => self.restart_main(),
                Woke::Event(i, Some(e)) => self.console_event(i - 1, e),
                Woke::Event(i, None) => self.console_closed(i - 1),
            }
        }
    }

    fn emit(&self, parts: Parts) {
        if !parts.is_empty() {
            let c = Changed::stack(&self.key, parts);
            let _ = self.ctx.out.try_send(Input::Changed(c));
        }
    }

    /// Folds `first` and whatever else is queued (up to [`DRAIN`]), then says what changed once.
    fn main_events(&mut self, first: FollowEvent) {
        let mut parts = Parts::NONE;
        let mut next = Some(first);
        for _ in 0..DRAIN {
            let Some(e) = next.take() else { break };
            parts |= self.main_event(&e);
            next = self.main.events().try_recv().ok();
        }
        if let Some(e) = next {
            parts |= self.main_event(&e);
        }
        self.emit(parts);
    }

    fn main_event(&mut self, e: &FollowEvent) -> Parts {
        let now = self.ctx.clock.now();
        match e {
            FollowEvent::Frame(f) => lock(&self.state).apply(f, now),
            FollowEvent::Live => {
                let parts = lock(&self.state).link(e, now);
                parts | self.describe()
            }
            _ => lock(&self.state).link(e, now),
        }
    }

    /// Asks the daemon what the stack is, after each connect, on a short-lived connection of its own.
    fn describe(&self) -> Parts {
        let client = self.client.clone().with_timeout(Some(SNAPSHOT_TIMEOUT));
        match client.dial().and_then(|c| c.snapshot()) {
            Ok(snap) => lock(&self.state).snapshot(&snap),
            Err(_) => Parts::NONE,
        }
    }

    /// The follower ended by itself (it could not start its thread): start another, after a pause.
    fn restart_main(&mut self) {
        thread::sleep(self.ctx.backoff.max);
        self.main = self
            .client
            .follow(subscription(self.wanted), self.ctx.backoff);
    }

    fn command(&mut self, c: Cmd) {
        let parts = match c {
            Cmd::Topics(w) => self.topics(w),
            Cmd::Console { inst, shown } => self.show(inst, shown),
            Cmd::Follow {
                inst,
                panel,
                sector,
            } => self.follow(&inst, panel, sector),
            Cmd::Stop => Parts::NONE,
        };
        self.emit(parts);
    }

    /// The topics wanted changed: the subscription is remade on the same connection.
    fn topics(&mut self, w: Wanted) -> Parts {
        if w == self.wanted {
            return Parts::NONE;
        }
        self.wanted = w;
        self.main.resubscribe(subscription(w));
        lock(&self.state).set_metered(w.stats)
    }

    fn show(&mut self, inst: String, shown: bool) -> Parts {
        let at = self.consoles.iter().position(|c| c.inst == inst);
        match (at, shown) {
            (None, true) => {
                let follow = self
                    .client
                    .follow(console_subscription(&inst, "", ""), self.ctx.backoff);
                let parts = lock(&self.state).set_console(&inst, true);
                self.consoles.push(ConsoleSub {
                    inst,
                    panel: String::new(),
                    sector: String::new(),
                    follow,
                });
                parts
            }
            (Some(i), false) => {
                self.consoles.remove(i);
                lock(&self.state).set_console(&inst, false)
            }
            _ => Parts::NONE,
        }
    }

    /// Follows another stream panel, or another row of its table. A new subscription, so no frame of the old one
    /// can be folded into the new stream.
    fn follow(&mut self, inst: &str, panel: String, sector: String) -> Parts {
        let Some(sub) = self.consoles.iter_mut().find(|c| c.inst == inst) else {
            return Parts::NONE;
        };
        if sub.panel == panel && sub.sector == sector {
            return Parts::NONE;
        }
        sub.follow = self.client.follow(
            console_subscription(inst, &panel, &sector),
            self.ctx.backoff,
        );
        let parts = lock(&self.state).refollow(inst, &panel);
        (sub.panel, sub.sector) = (panel, sector);
        parts
    }

    fn console_event(&mut self, i: usize, e: FollowEvent) {
        let Some(sub) = self.consoles.get(i) else {
            return;
        };
        let parts = match &e {
            FollowEvent::Frame(f) => match &f.console {
                Some(c) => lock(&self.state).console(&sub.inst, c, self.ctx.clock.instant()),
                None => Parts::NONE,
            },
            FollowEvent::Down { error, .. } => lock(&self.state).console_failed(&sub.inst, error),
            FollowEvent::Connecting { .. } | FollowEvent::Live => Parts::NONE,
        };
        self.emit(parts);
    }

    fn console_closed(&mut self, i: usize) {
        if i < self.consoles.len() {
            let sub = self.consoles.remove(i);
            let parts =
                lock(&self.state).console_failed(&sub.inst, "the console's subscription ended");
            self.emit(parts);
        }
    }
}
