//! A window draws only what someone could see change. While frames keep coming, even one a second, the GPU driver
//! keeps about 150 MB for the process (docs/port/weight.md, "the idle window"): a dozing row's countdown read again
//! every second held an open, untouched window at 225 MB. So a window in use counts down to the second; one on screen
//! but untouched writes its words to the minute and draws when they change; a hidden one draws nothing. These run the
//! app's time and the hub's clock on together, so a countdown counts down here as it does on screen, and the fixture
//! daemon sends its meters every second of that time (a `-stale` one goes quiet).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, WindowVisibility, point, px};

use super::tests::window;
use super::{RENDERS, Root, attention};
use crate::source::Source;

fn renders() -> u64 {
    RENDERS.with(std::cell::Cell::get)
}

/// One step of time for the app and the hub alike, as a frame loop would: timers fire, frames asked for are
/// delivered, dirty windows draw. Each whole second, a live daemon's meters.
fn step(src: &Source, vcx: &mut VisualTestContext, ms: u64) {
    src.skew(ms);
    vcx.executor().advance_clock(Duration::from_millis(ms));
    vcx.run_until_parked();
    vcx.update(|window, cx| window.simulate_next_frame(cx));
    vcx.run_until_parked();
}

/// Lets `ms` of time pass a tenth of a second at a time. Returns the frames drawn.
fn pass(src: &Source, vcx: &mut VisualTestContext, ms: u64) -> u64 {
    let before = renders();
    for i in 1..=ms / 100 {
        step(src, vcx, 100);
        if i % 10 == 0 {
            src.beat();
        }
    }
    renders() - before
}

/// A stack's page, settled: its first batches in, its fades done.
fn settled<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
) -> (Source, Entity<Root>, &'a mut VisualTestContext) {
    let (src, root, vcx) = window(cx, name);
    pass(&src, vcx, 2_000);
    (src, root, vcx)
}

fn larder(cx: &mut TestAppContext) -> (Source, Entity<Root>, &mut VisualTestContext) {
    settled(cx, "larder")
}

/// The dozing instance (a countdown that moves with time alone, else these tests prove nothing): its name, and what
/// its row says, to the minute, as a window glanced at writes it.
fn dozing(root: &Entity<Root>, vcx: &mut VisualTestContext) -> (String, String) {
    vcx.update(|_, cx| {
        let r = root.read(cx);
        let k = r.key().cloned().unwrap();
        let st = r.hub.stack(&k).unwrap();
        let snap = st.words();
        let v = snap
            .instances
            .iter()
            .find(|v| v.state == doze_display::State::Idle && v.idle_since.is_some())
            .expect("larder has a dozing instance");
        let (said, _) = doze_display::coarse(|| doze_display::because(v, &snap, r.hub.now()));
        (v.name.clone(), said)
    })
}

/// The stale banner over the stack's page, if it shows.
fn banner(root: &Entity<Root>, vcx: &mut VisualTestContext) -> String {
    vcx.update(|_, cx| root.read(cx).overview.read(cx).home.banner.text.clone())
}

/// Everything the stack's page shows (its head, rows and banner, and the Map's cards), as text.
fn shown(root: &Entity<Root>, vcx: &mut VisualTestContext) -> String {
    vcx.update(|_, cx| {
        let o = root.read(cx).overview.read(cx);
        format!("{:?}\n{:?}", o.home, o.page)
    })
}

/// Lets `ms` pass a tenth of a second at a time, asserting that every frame drawn is a change of what the page says
/// (one frame each). Returns the frames, and when (ms from now) the page changed.
fn watched(
    src: &Source,
    root: &Entity<Root>,
    vcx: &mut VisualTestContext,
    ms: u64,
) -> (u64, Vec<u64>) {
    let (mut page, mut changes, mut frames) = (shown(root, vcx), Vec::new(), 0);
    for i in 1..=ms / 100 {
        let before = renders();
        step(src, vcx, 100);
        if i % 10 == 0 {
            src.beat();
        }
        let drew = renders() - before;
        let now = shown(root, vcx);
        if now == page {
            assert_eq!(drew, 0, "a frame at {} ms with nothing changed", i * 100);
        } else {
            // One change can land in two parts within the moment (the page, then Needs you, built off the UI thread):
            // two draws here, one on a screen that draws at most once a display refresh.
            assert!(drew <= 2, "{drew} frames for one change at {} ms", i * 100);
            changes.push(i * 100);
            page = now;
        }
        frames += drew;
    }
    (frames, changes)
}

#[gpui::test]
fn a_window_glanced_at_draws_only_when_its_words_change(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    assert!(
        !vcx.update(|w, _| w.is_window_active()),
        "not the key window"
    );
    assert!(attention::visible(), "on screen: the window is glanced at");
    let start = dozing(&root, vcx).1;
    assert!(
        start.starts_with("sleeps in ") && start.ends_with('m'),
        "to the minute: {start}"
    );
    // Three minutes, a tenth of a second at a time: every frame is a change of what the page says, and what it says
    // changes only as a word to the minute does (the countdown running out, an "up 18m" turning 19m), not every second.
    let (frames, changes) = watched(&src, &root, vcx, 180_000);
    assert_ne!(dozing(&root, vcx).1, start, "the countdown never moved");
    assert!(frames >= 1, "the countdown ran out, and nothing was drawn");
    eprintln!("glanced at for 180 s: {frames} frames, changes at {changes:?} ms");
    assert!(
        changes.len() <= 5,
        "{} changes in 180 s (at {changes:?} ms)",
        changes.len()
    );
    // Never two changes within a few seconds of each other unless both are due: no second-by-second reading.
    let close = changes.windows(2).filter(|w| w[1] - w[0] < 5_000).count();
    assert!(close <= 1, "changes {changes:?} ms");
    assert_eq!(banner(&root, vcx), "", "a live daemon is not stale");
}

#[gpui::test]
fn a_hidden_window_draws_nothing_and_catches_up_when_shown(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    let before = dozing(&root, vcx).1;
    vcx.simulate_visibility_change(WindowVisibility::Hidden);
    vcx.run_until_parked();
    assert!(!src.hub.visible(), "a hidden window asks for no meters");
    let page = shown(&root, vcx);
    assert_eq!(pass(&src, vcx, 150_000), 0, "frames drawn while hidden");
    assert_eq!(shown(&root, vcx), page, "a hidden page is not read");
    // Shown again (the redraw comes as the platform says so), what changed meanwhile is read and drawn.
    let first = renders();
    vcx.simulate_visibility_change(WindowVisibility::Visible);
    let drawn = renders() - first + pass(&src, vcx, 1_000);
    assert!(src.hub.visible(), "shown, the meters come back");
    assert!(drawn >= 1, "shown again, what changed is drawn");
    assert_ne!(shown(&root, vcx), page, "the page caught up");
    assert_ne!(
        dozing(&root, vcx).1,
        before,
        "the countdown moved on meanwhile"
    );
}

#[gpui::test]
fn a_frozen_daemon_says_so_on_a_window_glanced_at_in_one_frame(cx: &mut TestAppContext) {
    let (src, root, vcx) = settled(cx, "larder-stale");
    // The daemon went quiet when its script ended, two seconds ago: not old yet.
    assert_eq!(banner(&root, vcx), "", "fresh at first");
    let drawn = pass(&src, vcx, 10_000);
    assert_eq!(
        banner(&root, vcx),
        "last update <1m ago",
        "the banner, to the minute"
    );
    assert_eq!(drawn, 1, "one frame says it, and nothing ticks after");
    // It turns "1m ago" when the minute comes; in that minute only words to the minute change (the banner's, the
    // countdown running out, the rows' "up"), a frame each, and nothing between them.
    let (_, changes) = watched(&src, &root, vcx, 60_000);
    eprintln!("the minute after the banner: changes at {changes:?} ms");
    assert!(changes.len() <= 4, "changes at {changes:?} ms");
    assert_eq!(banner(&root, vcx), "last update 1m ago");
}

#[gpui::test]
fn a_window_in_use_counts_down_to_the_second_and_to_the_minute_when_left(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    // Glanced at: "sleeps in <1m" holds for the rest of the countdown.
    assert_eq!(pass(&src, vcx, 3_000), 0, "frames while glanced at");
    // A touch: the countdown to the second at once, then every second.
    vcx.update(|w, _| w.activate_window());
    vcx.run_until_parked();
    let used = pass(&src, vcx, 5_000);
    assert!(used >= 4, "{used} frames in 5 s of use");
    let (_, said) = vcx.update(|_, cx| {
        let r = root.read(cx);
        let st = r.hub.stack(r.key().unwrap()).unwrap();
        let snap = st.words();
        let v = snap
            .instances
            .iter()
            .find(|v| v.state == doze_display::State::Idle && v.idle_since.is_some())
            .unwrap();
        ((), doze_display::because(v, &snap, r.hub.now()))
    });
    assert!(
        shown(&root, vcx).contains(&said),
        "in use, to the second: {said}"
    );
    // Left alone: once no longer in use, the words go to the minute in one frame, then nothing while they hold.
    pass(&src, vcx, attention::FOR.as_millis() as u64);
    assert!(
        shown(&root, vcx).contains("sleeps in <1m"),
        "left alone, to the minute"
    );
    assert_eq!(pass(&src, vcx, 3_000), 0, "frames once left alone");
}

#[gpui::test]
fn an_instance_page_reads_its_countdown_once_a_second_not_more(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    let name = dozing(&root, vcx).0;
    vcx.update(|w, _| w.activate_window());
    vcx.update(|window, cx| root.update(cx, |r, cx| r.open_instance(&name, window, cx)));
    vcx.run_until_parked();
    pass(&src, vcx, 2_000);
    let before = crate::instance::tick::TICKS.with(std::cell::Cell::get);
    pass(&src, vcx, 10_000);
    let ticks = crate::instance::tick::TICKS.with(std::cell::Cell::get) - before;
    // One loop: ten reads in ten seconds (the old loop started another on each read, doubling every second).
    assert!((9..=11).contains(&ticks), "{ticks} reads in 10 s");
}

#[gpui::test]
fn hands_off_keeps_a_persons_keys_and_pointer_out(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    crate::handsoff::set();
    vcx.update(|_, cx| crate::handsoff::guard_keys(cx));
    vcx.update(|w, _| w.refresh());
    vcx.run_until_parked();
    let over = |vcx: &mut VisualTestContext| vcx.update(|_, cx| root.read(cx).overlay.is_some());
    // A person's ⌘K reaches nothing; the driver's does.
    vcx.simulate_keystrokes("cmd-k");
    assert!(!over(vcx), "a person's key opened something");
    crate::handsoff::sending(|| vcx.simulate_keystrokes("cmd-k"));
    assert!(over(vcx), "the driver's key did not get through");
    // A person's pointer is not even heard: the window stays untouched.
    attention::key(true, vcx.executor().now());
    pass(&src, vcx, attention::FOR.as_millis() as u64 + 1_000);
    vcx.simulate_mouse_move(point(px(400.), px(300.)), None, gpui::Modifiers::default());
    assert!(
        !attention::attended(vcx.executor().now()),
        "a person's pointer got in"
    );
    crate::handsoff::sending(|| {
        vcx.simulate_mouse_move(point(px(410.), px(300.)), None, gpui::Modifiers::default())
    });
    assert!(
        attention::attended(vcx.executor().now()),
        "the driver's pointer did not get through"
    );
}

#[gpui::test]
fn the_pointer_over_a_window_that_is_not_key_draws_nothing(cx: &mut TestAppContext) {
    let (_src, root, vcx) = larder(cx);
    dozing(&root, vcx);
    let before = renders();
    for i in 0..20 {
        let x = 300. + 10. * i as f32;
        vcx.simulate_mouse_move(point(px(x), px(300.)), None, gpui::Modifiers::default());
    }
    vcx.run_until_parked();
    vcx.update(|window, cx| window.simulate_next_frame(cx));
    vcx.run_until_parked();
    assert_eq!(
        renders() - before,
        0,
        "frames drawn for a pointer over a window that is not key"
    );
}

#[gpui::test]
fn the_meters_stream_while_the_window_shows(cx: &mut TestAppContext) {
    let (src, _root, vcx) = larder(cx);
    assert!(src.hub.visible(), "on screen, untouched: the meters come");
    vcx.update(|w, _| w.activate_window());
    vcx.run_until_parked();
    assert!(src.hub.visible(), "in use: the meters come");
    vcx.simulate_visibility_change(WindowVisibility::Hidden);
    vcx.run_until_parked();
    assert!(!src.hub.visible(), "hidden: no meters");
    vcx.simulate_visibility_change(WindowVisibility::Visible);
    vcx.run_until_parked();
    assert!(src.hub.visible(), "shown again: they come back");
}
