//! The window closed lets the hub's log ring go (docs/port/weight.md, "A live stack's idle window": the closed app
//! holds no lines nobody can see); opened again, a page with logs reads the daemon's backlog and shows it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::inside::Inside;
use super::tests::window;
use super::{Root, Screen};

/// The lines All logs shows once they have stopped coming: some, then the same count for a quarter of a second
/// (the backlog may come in more than one batch).
fn settled(root: &Entity<Root>, vcx: &mut VisualTestContext) -> usize {
    let (start, mut last, mut since) = (Instant::now(), 0, Instant::now());
    loop {
        let n = shown(root, vcx, 1);
        if n != last {
            (last, since) = (n, Instant::now());
        } else if n > 0 && since.elapsed() > Duration::from_millis(250) {
            return n;
        }
        if start.elapsed() > Duration::from_secs(5) {
            return n;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The lines All logs shows, once at least `want` have come (a moment of the wall clock at most: the hub's threads
/// read the fixture daemon, and the backlog may come in more than one batch). More than `want` returns at once, so a
/// line shown twice is still seen.
fn shown(root: &Entity<Root>, vcx: &mut VisualTestContext, want: usize) -> usize {
    let start = Instant::now();
    loop {
        vcx.run_until_parked();
        let n = vcx.update(|_, cx| match &root.read(cx).inside {
            Some(Inside::Logs(l)) => l.read(cx).index.len(),
            _ => 0,
        });
        if n >= want || start.elapsed() > Duration::from_secs(5) {
            return n;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
fn reopened_after_a_close_all_logs_shows_the_lines_again(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "shop");
    vcx.update(|window, cx| root.update(cx, |r, cx| r.open_logs(window, cx)));
    assert!(vcx.update(|_, cx| matches!(root.read(cx).screen, Screen::Logs(_))));
    let before = settled(&root, vcx);
    assert!(before > 0, "All logs shows the fixture's lines");
    let key = src.first.clone().unwrap();

    // Closed, as the app closes it (`app::run`'s window-closed hook): the window's pages go, the hub hides.
    vcx.update(|window, _| window.remove_window());
    drop(root);
    cx.run_until_parked();
    cx.update(|_| {}); // effects flushed: the window's entities released
    src.hub.set_visible(false);
    let start = Instant::now();
    while src.hub.with_logs(&key, |r| r.len()) != Some(0) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the ring kept its lines; logs watchers {}",
            src.hub.watchers(&key, doze_control::Topic::Logs)
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        src.hub.stack(&key).unwrap().link.is_live(),
        "the menu bar still follows the stack"
    );

    // Opened again, on All logs: the daemon's backlog, shown.
    let (hub, first, facts) = (src.hub.clone(), src.first.clone(), src.facts.clone());
    let (root, vcx) = cx.add_window_view(|window, cx| Root::new(hub, first, facts, window, cx));
    src.hub.set_visible(true);
    vcx.update(|window, cx| root.update(cx, |r, cx| r.open_logs(window, cx)));
    assert_eq!(
        shown(&root, vcx, before),
        before,
        "the same lines, once each"
    );
}
