use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use super::*;

fn ctx(idle: u64) -> Arc<Ctx> {
    Arc::new(Ctx {
        dir: PathBuf::new(),
        idle: Duration::from_secs(idle),
        tasks: Tasks::new(),
        clock: Arc::new(crate::ManualClock::at(0)),
        ext: None,
    })
}

static OPENS: AtomicUsize = AtomicUsize::new(0);
static CLOSES: AtomicUsize = AtomicUsize::new(0);

struct Counter(u32);

impl Service for Counter {
    type Request = u32;
    type Response = u32;
    type Error = Infallible;

    async fn open(_: &Ctx, _: &'static str) -> Result<Self, Infallible> {
        OPENS.fetch_add(1, SeqCst);
        Ok(Self(0))
    }

    async fn handle(&mut self, add: u32) -> u32 {
        self.0 += add;
        self.0
    }

    async fn close(self) {
        CLOSES.fetch_add(1, SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn it_starts_on_the_first_call_is_reaped_when_idle_and_starts_again() {
    let svc = Lazy::<Counter>::new("c", ctx(30));
    assert!(!svc.is_live());
    assert_eq!(OPENS.load(SeqCst), 0);

    assert_eq!(svc.call(5).await.unwrap(), 5);
    assert!(svc.is_live());
    tokio::time::sleep(Duration::from_secs(29)).await;
    assert!(svc.is_live(), "29 s idle of 30: still live");
    assert_eq!(svc.call(1).await.unwrap(), 6, "state kept while live");

    tokio::time::sleep(Duration::from_secs(31)).await;
    assert!(!svc.is_live(), "reaped after 30 s idle");
    assert_eq!((OPENS.load(SeqCst), CLOSES.load(SeqCst)), (1, 1));

    assert_eq!(svc.call(1).await.unwrap(), 1, "fresh state: started again");
    assert_eq!(OPENS.load(SeqCst), 2);
}

#[test]
fn a_registered_service_is_a_handful_of_bytes() {
    let handle = std::mem::size_of::<Lazy<Counter>>();
    let shared = std::mem::size_of::<Shared<Counter>>();
    // The Arc header is two words more.
    assert!(handle + shared + 16 <= 160, "{handle} + {shared}");
}

static RACE_OPENS: AtomicUsize = AtomicUsize::new(0);

struct Racer;

impl Service for Racer {
    type Request = ();
    type Response = ();
    type Error = Infallible;

    async fn open(_: &Ctx, _: &'static str) -> Result<Self, Infallible> {
        RACE_OPENS.fetch_add(1, SeqCst);
        Ok(Self)
    }

    async fn handle(&mut self, (): ()) {}
}

/// Calls that land at the reap boundary are never lost and never run on a dead actor.
#[tokio::test(start_paused = true)]
async fn calls_at_the_reap_boundary_are_not_lost() {
    let svc = Lazy::<Racer>::new("r", ctx(1));
    for i in 0..200u64 {
        svc.call(()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(990 + i % 20)).await;
    }
    assert!(RACE_OPENS.load(SeqCst) >= 2);
}

struct Broken;

impl Service for Broken {
    type Request = ();
    type Response = ();
    type Error = &'static str;

    async fn open(_: &Ctx, _: &'static str) -> Result<Self, &'static str> {
        Err("disk full")
    }

    async fn handle(&mut self, (): ()) {}
}

#[tokio::test(start_paused = true)]
async fn an_open_failure_reaches_the_caller_and_the_next_call_tries_again() {
    let svc = Lazy::<Broken>::new("b", ctx(1));
    for _ in 0..2 {
        assert!(matches!(svc.call(()).await, Err(Error::Open(m)) if m.contains("disk full")));
    }
}

static STOP_CLOSES: AtomicUsize = AtomicUsize::new(0);

struct Stopper;

impl Service for Stopper {
    type Request = ();
    type Response = ();
    type Error = Infallible;

    async fn open(_: &Ctx, _: &'static str) -> Result<Self, Infallible> {
        Ok(Self)
    }

    async fn handle(&mut self, (): ()) {}

    async fn close(self) {
        STOP_CLOSES.fetch_add(1, SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn a_shutdown_closes_a_live_service_before_it_returns() {
    let ctx = ctx(3600);
    let svc = Lazy::<Stopper>::new("s", Arc::clone(&ctx));
    svc.call(()).await.unwrap();
    assert!(svc.is_live());
    ctx.tasks.shutdown().await;
    assert_eq!(STOP_CLOSES.load(SeqCst), 1);
    assert!(!svc.is_live());
}
