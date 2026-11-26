use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use super::*;
use crate::Rested;

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

static REST_OPENS: AtomicUsize = AtomicUsize::new(0);
static REST_CLOSES: AtomicUsize = AtomicUsize::new(0);

struct Rester(u32);

impl Service for Rester {
    type Request = u32;
    type Response = u32;
    type Error = Infallible;

    async fn open(_: &Ctx, _: &'static str) -> Result<Self, Infallible> {
        REST_OPENS.fetch_add(1, SeqCst);
        Ok(Self(0))
    }

    async fn handle(&mut self, add: u32) -> u32 {
        self.0 += add;
        self.0
    }

    async fn close(self) {
        REST_CLOSES.fetch_add(1, SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn a_service_told_to_rest_closes_as_it_does_when_idle_and_opens_again_on_the_next_call() {
    let ctx = ctx(3600);
    let rests = ctx.tasks.rests();
    let svc = Lazy::<Rester>::keyed("rester", "eu-west-1", Arc::clone(&ctx));
    assert_eq!(rests.is_live("rester", "eu-west-1"), Some(false));
    assert_eq!(svc.call(5).await.unwrap(), 5);
    assert_eq!(rests.is_live("rester", "eu-west-1"), Some(true));

    // The idle timer is an hour away: the close is the rest's, and it has finished when the rest returns.
    assert_eq!(rests.rest("rester", "eu-west-1").await, Rested::Closed);
    assert!(!svc.is_live());
    assert_eq!(
        (REST_OPENS.load(SeqCst), REST_CLOSES.load(SeqCst)),
        (1, 1),
        "the service was closed, not dropped"
    );
    assert_eq!(rests.is_live("rester", "eu-west-1"), Some(false));

    assert_eq!(svc.call(1).await.unwrap(), 1, "fresh state: opened again");
    assert_eq!(REST_OPENS.load(SeqCst), 2);
    assert!(svc.is_live());
}

#[tokio::test(start_paused = true)]
async fn a_service_that_is_resting_or_was_never_made_is_said_so() {
    let ctx = ctx(3600);
    let rests = ctx.tasks.rests();
    let svc = Lazy::<Counter>::keyed("counter-rests", "us-east-1", Arc::clone(&ctx));
    assert_eq!(
        rests.rest("counter-rests", "us-east-1").await,
        Rested::AlreadyResting,
        "made and never called: it holds nothing"
    );
    assert_eq!(
        rests.rest("counter-rests", "eu-west-1").await,
        Rested::Unknown
    );
    assert_eq!(rests.rest("nothing", "us-east-1").await, Rested::Unknown);
    assert_eq!(rests.is_live("nothing", "us-east-1"), None);
    drop(svc);
    assert_eq!(
        rests.rest("counter-rests", "us-east-1").await,
        Rested::Unknown,
        "a service that no longer exists is not kept alive by the registry"
    );
}

#[tokio::test(start_paused = true)]
async fn the_registry_lists_what_was_made_by_name_and_key() {
    let ctx = ctx(3600);
    let rests = ctx.tasks.rests();
    let _b = Lazy::<Racer>::keyed("b", "us-east-1", Arc::clone(&ctx));
    let a = Lazy::<Racer>::keyed("a", "us-east-1", Arc::clone(&ctx));
    let _a2 = Lazy::<Racer>::keyed("a", "eu-west-1", Arc::clone(&ctx));
    a.call(()).await.unwrap();
    let all = rests.seen();
    let seen: Vec<(&str, &str, bool)> = all
        .iter()
        .map(|s| (s.name, s.key.as_str(), s.live))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("a", "eu-west-1", false),
            ("a", "us-east-1", true),
            ("b", "us-east-1", false)
        ]
    );
}

static QUEUED: AtomicUsize = AtomicUsize::new(0);

struct Slow;

impl Service for Slow {
    type Request = ();
    type Response = usize;
    type Error = Infallible;

    async fn open(_: &Ctx, _: &'static str) -> Result<Self, Infallible> {
        Ok(Self)
    }

    async fn handle(&mut self, (): ()) -> usize {
        tokio::time::sleep(Duration::from_secs(2)).await;
        QUEUED.fetch_add(1, SeqCst) + 1
    }
}

/// A request already in the inbox when the rest comes is answered before the service closes (what the idle close does too).
#[tokio::test(start_paused = true)]
async fn what_is_queued_when_a_service_is_told_to_rest_is_still_answered() {
    let ctx = ctx(3600);
    let rests = ctx.tasks.rests();
    let svc = Arc::new(Lazy::<Slow>::keyed("slow", "", Arc::clone(&ctx)));
    let (first, second) = (Arc::clone(&svc), Arc::clone(&svc));
    let one = tokio::spawn(async move { first.call(()).await });
    let two = tokio::spawn(async move { second.call(()).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(rests.rest("slow", "").await, Rested::Closed);
    assert_eq!(one.await.unwrap().unwrap(), 1);
    assert_eq!(two.await.unwrap().unwrap(), 2);
    assert!(!svc.is_live());
}
