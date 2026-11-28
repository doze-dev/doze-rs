//! A service opens and closes on a blocking thread, not on the serving one.
//!
//! Opening reads the database (SQS loads every message, Step Functions every execution; redb repairs a file a `kill -9` left), and
//! closing may compact it: whole seconds on a large store, during which a server with one serving thread would answer nothing.
//! [`Service::open`] and [`Service::close`] are futures, run here with `Handle::block_on` on a thread of the blocking pool: what they
//! await (none of today's services awaits anything there) is driven by the serving thread as usual. The actor's calls wait in its
//! inbox meanwhile.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::oneshot;

use super::{Service, Shared};

/// Opens the service on a blocking thread. `Err` is the open's error, or the runtime stopping before it could run.
pub(super) async fn opened<S: Service>(shared: &Arc<Shared<S>>) -> Result<S, String> {
    let shared = Arc::clone(shared);
    let opened = blocking(async move { S::open(&shared.ctx, shared.name).await }).await;
    match opened {
        Some(opened) => opened.map_err(|e| e.to_string()),
        None => Err("the server is stopping".to_owned()),
    }
}

/// Closes the service on a blocking thread, and waits until it has.
pub(super) async fn close<S: Service>(svc: S) {
    // WHY: `None` is a runtime stopping before the close could run: the service is dropped, which releases its files all the same.
    blocking(svc.close()).await;
}

/// Runs `work` on a blocking thread and answers its output (`None`: the runtime stopped first); a panic in it is the caller's.
async fn blocking<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> Option<T> {
    let (answer, answered) = oneshot::channel();
    let job: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(async move {
        // WHY: a caller that stopped waiting needs no answer.
        answer.send(work.await).ok();
    });
    run_boxed(job).await;
    answered.await.ok()
}

/// The one instance of the blocking hand-off (not one per service: each would carry its own copy of tokio's `block_on`).
async fn run_boxed(job: Pin<Box<dyn Future<Output = ()> + Send>>) {
    let handle = Handle::current();
    if let Err(e) = tokio::task::spawn_blocking(move || handle.block_on(job)).await
        && e.is_panic()
    {
        std::panic::resume_unwind(e.into_panic());
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::{Ctx, Lazy, ManualClock, Service, Tasks};

    /// Opens (and closes) only once the gate in its context lets it: a slow database.
    struct Slow;

    fn gate(ctx: &Ctx) -> Arc<Mutex<mpsc::Receiver<()>>> {
        let ext = Arc::clone(ctx.ext.as_ref().unwrap());
        ext.downcast::<Mutex<mpsc::Receiver<()>>>().unwrap()
    }

    impl Service for Slow {
        type Request = ();
        type Response = ();
        type Error = Infallible;

        async fn open(ctx: &Ctx, _: &'static str) -> Result<Self, Infallible> {
            gate(ctx).lock().unwrap().recv().unwrap();
            Ok(Self)
        }

        async fn handle(&mut self, (): ()) {}
    }

    /// Answers at once.
    struct Quick;

    impl Service for Quick {
        type Request = ();
        type Response = ();
        type Error = Infallible;

        async fn open(_: &Ctx, _: &'static str) -> Result<Self, Infallible> {
            Ok(Self)
        }

        async fn handle(&mut self, (): ()) {}
    }

    fn ctx(ext: Option<Arc<dyn std::any::Any + Send + Sync>>) -> Arc<Ctx> {
        Arc::new(Ctx {
            dir: std::path::PathBuf::new(),
            idle: Duration::from_secs(60),
            tasks: Tasks::new(),
            clock: Arc::new(ManualClock::at(0)),
            ext,
        })
    }

    /// One serving thread: while one service takes two seconds to open, another is called and answers.
    #[tokio::test(flavor = "current_thread")]
    async fn a_slow_open_does_not_hold_up_another_service() {
        let (open_now, waiting) = mpsc::channel::<()>();
        let slow = Arc::new(Lazy::<Slow>::new(
            "slow",
            ctx(Some(Arc::new(Mutex::new(waiting)))),
        ));
        let quick = Lazy::<Quick>::new("quick", ctx(None));
        // The slow open may finish two seconds from now, told from a thread of its own.
        let started = Instant::now();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            open_now.send(()).unwrap();
        });
        let opening = tokio::spawn({
            let slow = Arc::clone(&slow);
            async move { slow.call(()).await.unwrap() }
        });
        // Let the slow open start.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        quick.call(()).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "answered while the other service opened: {:?}",
            started.elapsed()
        );
        opening.await.unwrap();
        releaser.join().unwrap();
    }
}
