use super::*;

#[tokio::test(start_paused = true)]
async fn nobody_listening_means_no_task() {
    let t = Ticker::new(Duration::from_secs(2));
    assert!(!t.running());
    assert_eq!(t.viewers(), 0);
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert!(!t.running());
}

#[tokio::test(start_paused = true)]
async fn the_first_viewer_starts_the_task_and_the_last_to_leave_stops_it() {
    let t = Ticker::new(Duration::from_secs(2));
    let a = t.watch();
    assert!(t.running());
    let b = t.watch();
    assert_eq!(t.viewers(), 2);
    drop(a);
    assert!(t.running(), "one viewer is left");
    drop(b);
    assert!(!t.running());
    assert_eq!(t.viewers(), 0);
    // And again: a later viewer starts a new one.
    let c = t.watch();
    assert!(t.running());
    drop(c);
    assert!(!t.running());
}

#[tokio::test(start_paused = true)]
async fn a_viewer_hears_a_tick_every_period() {
    let t = Ticker::new(Duration::from_secs(2));
    let mut v = t.watch();
    let start = tokio::time::Instant::now();
    assert!(v.changed().await);
    assert_eq!(start.elapsed(), Duration::from_secs(2));
    assert!(v.changed().await);
    assert_eq!(start.elapsed(), Duration::from_secs(4));
}

#[tokio::test(start_paused = true)]
async fn a_bump_reaches_every_viewer_at_once() {
    let t = Ticker::new(Duration::from_secs(3600));
    let (mut a, mut b) = (t.watch(), t.watch());
    t.bump();
    assert!(a.changed().await && b.changed().await);
}

/// The signal's value: it moves only when the task (or a `bump`) moves it.
fn value(t: &Ticker) -> u64 {
    *t.tx.borrow()
}

#[tokio::test(start_paused = true)]
async fn the_task_really_stops_when_the_last_viewer_goes_and_does_not_tick_on_its_own_after() {
    let t = Ticker::new(Duration::from_secs(2));
    let v = t.watch();
    tokio::time::sleep(Duration::from_secs(9)).await;
    let while_watched = value(&t);
    assert!(while_watched >= 3, "the task ticks while somebody listens");
    drop(v);
    // A task that was only forgotten (its handle dropped, not aborted) would go on ticking for ever.
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert_eq!(
        value(&t),
        while_watched,
        "the task outlived its last viewer"
    );
    // A viewer after that starts a task that ticks again.
    let _again = t.watch();
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(value(&t) > while_watched);
}

#[tokio::test(start_paused = true)]
async fn a_task_that_has_finished_is_not_running() {
    let t = Ticker::new(Duration::from_secs(2));
    let _v = t.watch();
    assert!(t.running());
    // Whatever ended the task (a panic, the runtime), the signal does not claim a timer that is not there.
    let task = t
        .producer
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .task
        .take()
        .unwrap();
    task.abort();
    let _ = task.await;
    t.producer
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .task = Some(tokio::spawn(async {}));
    tokio::task::yield_now().await;
    assert!(!t.running(), "a finished handle is not a running task");
}
