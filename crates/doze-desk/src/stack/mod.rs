//! One running stack: its state, and the thread that keeps it (PLAN.md §4.3).

mod state;
mod worker;

pub(crate) use state::StackState;

use crate::changed::Parts;
use crate::clock::Clock;
use crate::coalesce::Input;
use crate::ring::LogRing;
use crate::view::StackView;
use async_channel::Sender;
use doze_control::{Backoff, Client};
use doze_projects::StackKey;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

/// Which optional topics pages watch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Wanted {
    pub(crate) stats: bool,
    pub(crate) logs: bool,
}

/// What the hub tells a stack's thread.
#[derive(Debug)]
pub(crate) enum Cmd {
    /// The topics wanted now (a watch count crossed zero).
    Topics(Wanted),
    /// A console starts or stops being shown.
    Console {
        inst: String,
        shown: bool,
    },
    /// A shown console follows this stream panel, and this row of its table.
    Follow {
        inst: String,
        panel: String,
        sector: String,
    },
    Stop,
}

/// What every stack's thread shares with the hub.
#[derive(Debug, Clone)]
pub(crate) struct Ctx {
    pub(crate) clock: Clock,
    pub(crate) backoff: Backoff,
    pub(crate) out: Sender<Input>,
}

/// A running stack, as the hub holds it. Dropping it stops its thread.
#[derive(Debug)]
pub(crate) struct Stack {
    state: Arc<Mutex<StackState>>,
    cmds: Sender<Cmd>,
    client: Client,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Stack {
    /// Starts following the daemon `client` dials, streaming what is `wanted` and the consoles `shown`.
    pub(crate) fn start(
        key: StackKey,
        client: Client,
        wanted: Wanted,
        shown: &[String],
        ctx: Ctx,
    ) -> Stack {
        let name = format!("doze-desk {}", key.slug());
        let mut st = StackState::new(key, ctx.clock.now());
        st.set_metered(wanted.stats);
        let state = Arc::new(Mutex::new(st));
        let (cmds, rx) = async_channel::unbounded();
        for inst in shown {
            let _ = cmds.try_send(Cmd::Console {
                inst: inst.clone(),
                shown: true,
            });
        }
        let w = worker::Worker::new(Arc::clone(&state), client.clone(), wanted, rx, ctx);
        if let Err(e) = thread::Builder::new().name(name).spawn(move || w.run()) {
            let mut s = lock(&state);
            s.view.link.conn = crate::Conn::Down;
            s.view.link.error = format!("could not start a thread: {e}");
        }
        Stack {
            state,
            cmds,
            client,
        }
    }

    /// The stack as pages see it now.
    pub(crate) fn view(&self) -> StackView {
        lock(&self.state).view.clone()
    }

    /// Reads the log ring.
    pub(crate) fn with_ring<R>(&self, f: impl FnOnce(&LogRing) -> R) -> R {
        f(&lock(&self.state).ring)
    }

    /// Whether a verb can be sent now.
    pub(crate) fn is_live(&self) -> bool {
        lock(&self.state).view.link.is_live()
    }

    /// The client that dials this stack's daemon.
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) fn command(&self, c: Cmd) {
        let _ = self.cmds.try_send(c);
    }

    /// Keeps what the project record says was started last time.
    pub(crate) fn set_record(&self, last_set: &[String]) -> Parts {
        lock(&self.state).set_record(last_set)
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = self.cmds.try_send(Cmd::Stop);
    }
}
