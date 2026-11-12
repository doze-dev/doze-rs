//! A service that costs nothing until it is used.
//!
//! - It is registered at startup as a [`Lazy`]: a small handle to a small record, with no task, no timer
//!   and no thread.
//! - The first [`Lazy::call`] spawns an actor task that opens the service ([`Service::open`]) and owns it:
//!   requests arrive on an inbox and are handled one at a time with `&mut self`.
//! - After [`Ctx::idle`] without a request the actor closes its inbox, answers what is already queued,
//!   closes the service ([`Service::close`]: this is where a database file is released) and ends.
//! - The next call finds a closed inbox and spawns a new actor, which first waits for the old one to
//!   finish closing, so a file is never open twice.
//!
//! Nothing polls: a live idle service owns exactly one timer, and a reaped one owns nothing. The slot
//! mutex guards only which actor is current (a few words, never held across an `.await`); the service's
//! own state is owned by the actor task alone.

use std::fmt;
use std::future::Future;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::{Clock, Tasks};

/// What every service shares: where its files live, how long it may sit idle, and the task set its actor
/// runs in.
#[derive(Debug)]
pub struct Ctx {
    /// The service's directory.
    pub dir: PathBuf,
    /// How long without a request before the actor closes the service.
    pub idle: Duration,
    /// Actors are spawned here, so a shutdown stops and waits for them.
    pub tasks: Tasks,
    /// The time, as a value a test can hold.
    pub clock: Arc<dyn Clock>,
}

/// A service is a plain struct that owns its state. `open` loads it, `handle` serves one request.
pub trait Service: Send + Sized + 'static {
    /// What a caller sends.
    type Request: Send + 'static;
    /// What it gets back.
    type Response: Send + 'static;
    /// Why the service could not open.
    type Error: fmt::Display + Send + 'static;

    /// Loads the service. Runs on the actor, after any earlier incarnation has closed.
    fn open(
        ctx: &Ctx,
        name: &'static str,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send;
    /// Serves one request.
    fn handle(&mut self, req: Self::Request) -> impl Future<Output = Self::Response> + Send;
    /// Releases files and flushes. Runs before the next incarnation may open.
    fn close(self) -> impl Future<Output = ()> + Send {
        async {}
    }
}

/// Why a call produced no response.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The service could not be opened; the message is the open error.
    #[error("service failed to open: {0}")]
    Open(String),
    /// The actor went away while the request was in flight (the runtime is shutting down).
    #[error("service went away")]
    Gone,
}

struct Call<S: Service> {
    req: S::Request,
    reply: oneshot::Sender<Result<S::Response, Error>>,
}

enum Slot<S: Service> {
    Empty,
    Live {
        id: u64,
        inbox: mpsc::Sender<Call<S>>,
        closed: oneshot::Receiver<()>,
    },
}

struct State<S: Service> {
    next_id: u64,
    slot: Slot<S>,
}

struct Shared<S: Service> {
    name: &'static str,
    ctx: Arc<Ctx>,
    state: Mutex<State<S>>,
}

impl<S: Service> Shared<S> {
    fn lock(&self) -> MutexGuard<'_, State<S>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// An actor that has fully ended clears its own slot, unless a newer one already took it.
    fn release(&self, id: u64) {
        let mut state = self.lock();
        if matches!(&state.slot, Slot::Live { id: live, .. } if *live == id) {
            state.slot = Slot::Empty;
        }
    }
}

/// The registered handle. Cheap to create, free until the first call.
pub struct Lazy<S: Service>(Arc<Shared<S>>);

impl<S: Service> fmt::Debug for Lazy<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lazy({})", self.0.name)
    }
}

/// A caller retries when it loses the race with a reap; this bounds a pathological loop.
const ATTEMPTS: usize = 3;
const INBOX: usize = 64;

impl<S: Service> Lazy<S> {
    /// Registers a service. Nothing runs until the first call.
    #[must_use]
    pub fn new(name: &'static str, ctx: Arc<Ctx>) -> Self {
        let state = State {
            next_id: 0,
            slot: Slot::Empty,
        };
        Self(Arc::new(Shared {
            name,
            ctx,
            state: Mutex::new(state),
        }))
    }

    /// True while an actor is running (for tests and the console, not for routing).
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(&self.0.lock().slot, Slot::Live { inbox, .. } if !inbox.is_closed())
    }

    /// Sends a request, starting the service if it is not running, and waits for the answer.
    ///
    /// # Errors
    /// The service could not open, or went away.
    pub async fn call(&self, mut req: S::Request) -> Result<S::Response, Error> {
        for _ in 0..ATTEMPTS {
            let inbox = self.inbox();
            let (reply, answer) = oneshot::channel();
            match inbox.send(Call { req, reply }).await {
                Ok(()) => return answer.await.map_err(|_| Error::Gone)?,
                // Lost the race with a reap: the inbox closed after we looked. Take the request back out
                // of the error and go round again (a fresh actor will be spawned).
                Err(mpsc::error::SendError(call)) => req = call.req,
            }
        }
        Err(Error::Gone)
    }

    /// The live inbox, spawning an actor if there is none. Never held across an await.
    fn inbox(&self) -> mpsc::Sender<Call<S>> {
        let mut state = self.0.lock();
        if let Slot::Live { inbox, .. } = &state.slot
            && !inbox.is_closed()
        {
            return inbox.clone();
        }
        // Empty, or an actor is still closing: the new one must wait for it to finish.
        let previous = match mem::replace(&mut state.slot, Slot::Empty) {
            Slot::Live { closed, .. } => Some(closed),
            Slot::Empty => None,
        };
        let id = state.next_id;
        state.next_id += 1;
        let (inbox, rx) = mpsc::channel(INBOX);
        let (closed_tx, closed) = oneshot::channel();
        self.0
            .ctx
            .tasks
            .spawn(actor(Arc::clone(&self.0), id, rx, previous, closed_tx));
        state.slot = Slot::Live {
            id,
            inbox: inbox.clone(),
            closed,
        };
        inbox
    }
}

async fn actor<S: Service>(
    shared: Arc<Shared<S>>,
    id: u64,
    mut inbox: mpsc::Receiver<Call<S>>,
    previous: Option<oneshot::Receiver<()>>,
    // Dropped when this actor ends: that is the "closed" signal the next actor waits for.
    _closed: oneshot::Sender<()>,
) {
    if let Some(previous) = previous {
        // WHY: it resolves, with an error, when the old actor has fully ended; the error is that signal.
        drop(previous.await);
    }
    match S::open(&shared.ctx, shared.name).await {
        Ok(mut svc) => {
            run(&shared.ctx, &mut svc, &mut inbox).await;
            // Idle, stopping, or every handle dropped: refuse new work, finish what is queued, release.
            inbox.close();
            while let Some(call) = inbox.recv().await {
                serve(&mut svc, call).await;
            }
            svc.close().await;
        }
        Err(e) => refuse(&mut inbox, &e.to_string()).await,
    }
    drop(inbox);
    shared.release(id);
}

/// Serves requests until the service has been idle too long, the task set is stopping, or the handles are gone.
async fn run<S: Service>(ctx: &Ctx, svc: &mut S, inbox: &mut mpsc::Receiver<Call<S>>) {
    let stopping = ctx.tasks.stopping();
    while let Some(Ok(Some(call))) = stopping
        .run_until_cancelled(tokio::time::timeout(ctx.idle, inbox.recv()))
        .await
    {
        serve(svc, call).await;
    }
}

async fn serve<S: Service>(svc: &mut S, call: Call<S>) {
    let response = svc.handle(call.req).await;
    if call.reply.send(Ok(response)).is_err() {
        // WHY: the caller gave up waiting; the request was served all the same.
    }
}

async fn refuse<S: Service>(inbox: &mut mpsc::Receiver<Call<S>>, why: &str) {
    inbox.close();
    while let Some(call) = inbox.recv().await {
        if call.reply.send(Err(Error::Open(why.to_owned()))).is_err() {
            // WHY: the caller gave up waiting.
        }
    }
}

#[cfg(test)]
mod tests;
