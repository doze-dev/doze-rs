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
