//! Asking the daemon to do something while a person may press Ctrl-C (Go wraps every command in
//! `signal.NotifyContext`): the verb is sent, and its answer waited for in short steps with the interrupt looked at
//! between them, so Ctrl-C or SIGTERM ends the run at once with exit 130 and, under `--json`, its envelope. The daemon
//! finishes what it was asked; only the waiting stops.

use crate::ctx::Ctx;
use crate::output::Mode;
use doze_control::{Connection, Error, Request, Verb};
use doze_os::interrupt::Interrupt;
use std::time::{Duration, Instant};

/// How often the wait looks at the interrupt.
const STEP: Duration = Duration::from_millis(100);

/// Why a verb did not finish.
#[derive(Debug)]
pub(crate) enum Stop {
    /// Ctrl-C or SIGTERM.
    Interrupted,
    /// The daemon said no, or did not answer in time.
    Failed(Error),
}

/// A connection, the interrupt and how long one verb may take.
#[derive(Debug)]
pub(crate) struct Asker {
    pub(crate) conn: Connection,
    interrupt: Interrupt,
    timeout: Duration,
}

impl Asker {
    /// Takes Ctrl-C over for the rest of the run.
    pub(crate) fn new(conn: Connection, ctx: &Ctx<'_>, mode: &Mode) -> Asker {
        Asker {
            conn,
            interrupt: ctx.host.watch_interrupt(),
            timeout: mode.timeout,
        }
    }

    /// Runs `verb` on `target` (empty: the daemon's "all") and waits for it, at most `--timeout`.
    pub(crate) fn verb(&self, verb: Verb, target: &str) -> Result<(), Stop> {
        if self.interrupt.raised() {
            return Err(Stop::Interrupted);
        }
        let pending = self
            .conn
            .send(Request::verb(verb, target))
            .map_err(Stop::Failed)?;
        let deadline = Instant::now() + self.timeout;
        loop {
            if self.interrupt.raised() {
                return Err(Stop::Interrupted);
            }
            if let Some(answer) = pending.poll(STEP) {
                return answer.map(drop).map_err(Stop::Failed);
            }
            if Instant::now() >= deadline {
                return Err(Stop::Failed(Error::Timeout(self.timeout)));
            }
        }
    }
}
