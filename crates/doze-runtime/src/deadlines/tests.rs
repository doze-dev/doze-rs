use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Semaphore;

use super::*;
use crate::{ManualClock, Tasks};

/// Counts its ticks.
struct Ticker {
    ticks: Arc<AtomicUsize>,
}

impl Service for Ticker {
    type Request = ();
    type Response = usize;
    type Error = std::convert::Infallible;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, Self::Error> {
        let ticks = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<AtomicUsize>().ok())
            .unwrap_or_default();
        Ok(Self { ticks })
    }

    async fn handle(&mut self, (): ()) -> usize {
        self.ticks.fetch_add(1, Ordering::SeqCst) + 1
    }
}

struct Rig {
    ticks: Arc<AtomicUsize>,
    clock: Arc<ManualClock>,
    tasks: Tasks,
    ctx: Arc<Ctx>,
    actor: Arc<Lazy<Ticker>>,
}

fn rig(now: i64) -> Rig {
    let ticks = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(ManualClock::at(now));
    let tasks = Tasks::new();
    let ctx = Arc::new(Ctx {
        dir: std::env::temp_dir(),
        idle: Duration::from_secs(3600),
        tasks: tasks.clone(),
        clock: clock.clone(),
        ext: Some(ticks.clone()),
    });
    let actor = Arc::new(Lazy::<Ticker>::new("ticker", Arc::clone(&ctx)));
    Rig {
        ticks,
        clock,
        tasks,
        ctx,
        actor,
    }
}

impl Rig {
    fn ticks(&self) -> usize {
        self.ticks.load(Ordering::SeqCst)
    }
}

#[tokio::test(start_paused = true)]
async fn a_deadline_ticks_once_and_the_task_ends_when_nothing_is_due() {
    let r = rig(1_000);
    let deadlines = Deadlines::<Ticker>::new(
        Arc::clone(&r.ctx),
        || (),
        Box::new(|n| Box::pin(async move { (n < 2).then_some(0) })),
    );
    deadlines.arm("r", &r.actor, None);
    assert!(!deadlines.armed("r"), "nothing due: no task");
    deadlines.arm("r", &r.actor, Some(5_000));
    assert!(deadlines.armed("r"));
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(r.ticks(), 0, "not due yet");
    r.clock.set(5_000);
    // The task naps in pieces of at most the deadline's distance: advance past the original four seconds.
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(r.ticks(), 1, "due");
    // The answer said "due again at once": the task waits the floor, by the service's clock.
    r.clock.set(6_000);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(r.ticks(), 2, "due again once");
    assert!(
        !deadlines.armed("r"),
        "nothing due after the second: the task ended"
    );
    r.tasks.shutdown().await;
}

/// A wake-up whose save is slow says "nothing due"; a call made meanwhile arms a deadline. The call's deadline is kept, not
/// overwritten by the wake-up's older answer.
#[tokio::test(start_paused = true)]
async fn a_deadline_armed_while_a_wake_up_saves_is_not_lost() {
    let r = rig(1_000);
    let saves = Arc::new(Semaphore::new(0));
    let gate = Arc::clone(&saves);
    let deadlines = Deadlines::<Ticker>::new(
        Arc::clone(&r.ctx),
        || (),
        Box::new(move |_| {
            let gate = Arc::clone(&gate);
            Box::pin(async move {
                // The save: held until the test lets it finish.
                gate.acquire().await.unwrap().forget();
                None
            })
        }),
    );
    deadlines.arm("r", &r.actor, Some(1_000));
    while r.ticks() == 0 {
        tokio::task::yield_now().await;
    }
    // The wake-up is saving. A call answered now has something due at 5 s.
    deadlines.arm("r", &r.actor, Some(5_000));
    saves.add_permits(1);
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(
        deadlines.armed("r"),
        "the call's deadline survives the wake-up's stale answer"
    );
    r.clock.set(5_000);
    saves.add_permits(1);
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(r.ticks(), 2, "and it comes due");
    assert!(!deadlines.armed("r"));
    r.tasks.shutdown().await;
}

/// The machine sleeps for an hour: the monotonic sleep does not move, the wall clock does. The deadline is still met within
/// [`LONGEST_NAP`] of waking, not a day later.
#[tokio::test(start_paused = true)]
async fn a_deadline_passed_while_the_machine_slept_is_seen_within_the_longest_nap() {
    let r = rig(0);
    let deadlines = Deadlines::<Ticker>::new(
        Arc::clone(&r.ctx),
        || (),
        Box::new(|_| Box::pin(async { None })),
    );
    deadlines.arm("r", &r.actor, Some(3_600_000));
    tokio::task::yield_now().await;
    // Asleep: an hour passes on the wall clock and none on the monotonic one.
    r.clock.set(3_600_000);
    tokio::time::sleep(LONGEST_NAP + Duration::from_secs(1)).await;
    assert_eq!(r.ticks(), 1);
    assert!(!deadlines.armed("r"), "nothing armed: no task, no timer");
    r.tasks.shutdown().await;
}

/// [`Due`] hands its wake-up the key, what it was armed with last, and the deadline.
#[tokio::test(start_paused = true)]
async fn a_due_key_wakes_with_what_it_was_last_armed_with() {
    let r = rig(0);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    let due = Due::<&'static str>::new(
        Arc::clone(&r.ctx),
        Duration::from_millis(100),
        Box::new(move |key, with, at| {
            log.lock().unwrap().push((key, with, at));
            Box::pin(async { None })
        }),
    );
    due.arm("eu", &"first", Some(1_000));
    due.arm("eu", &"second", Some(2_000));
    r.clock.set(2_000);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        *seen.lock().unwrap(),
        vec![("eu".to_owned(), "second", 2_000)]
    );
    assert!(!due.armed("eu"));
    r.tasks.shutdown().await;
}

/// The clock moves past a deadline under the task's sleep; a call that arms the same deadline again wakes it at once.
#[tokio::test(start_paused = true)]
async fn arming_a_deadline_already_past_wakes_the_task_at_once() {
    let r = rig(0);
    let deadlines = Deadlines::<Ticker>::new(
        Arc::clone(&r.ctx),
        || (),
        Box::new(|_| Box::pin(async { None })),
    );
    deadlines.arm("r", &r.actor, Some(60_000));
    tokio::task::yield_now().await;
    r.clock.set(61_000);
    deadlines.arm("r", &r.actor, Some(60_000));
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(r.ticks(), 1);
    r.tasks.shutdown().await;
}

/// Two calls answered in order, armed in the other order (the first awaited its save longer): the older answer's `None` must not
/// end the deadline the newer one armed. Before, the last arm won and the deadline was lost.
#[tokio::test(start_paused = true)]
async fn an_older_answer_armed_late_does_not_overwrite_a_newer_one() {
    let r = rig(1_000);
    let deadlines = Deadlines::<Ticker>::new(
        Arc::clone(&r.ctx),
        || (),
        Box::new(|_| Box::pin(async { None })),
    );
    let (_, older) = r.actor.call_numbered(()).await.unwrap();
    let (_, newer) = r.actor.call_numbered(()).await.unwrap();
    assert!(newer > older, "answers are numbered in the order given");
    deadlines.arm_ordered("r", &r.actor, Some(5_000), newer);
    deadlines.arm_ordered("r", &r.actor, None, older);
    assert!(deadlines.armed("r"), "the stale None was dropped");
    let before = r.ticks();
    r.clock.set(5_000);
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(r.ticks(), before + 1, "the newer deadline fired");
    r.tasks.shutdown().await;
}
