//! An idle window draws no frames. While frames keep coming, even one a second, the GPU driver keeps about 150 MB
//! for the process (docs/port/weight.md, "the idle window"): a dozing row's countdown read again every second held an
//! open, untouched window at 225 MB. These run the app's time and the hub's clock on together, so a countdown counts
//! down here as it does on screen.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, point, px};

use super::tests::window;
use super::{RENDERS, Root, attention};
use crate::source::Source;

fn renders() -> u64 {
    RENDERS.with(std::cell::Cell::get)
}

/// Lets `ms` of time pass a tenth of a second at a time, for the app and the hub alike, as a frame loop would: timers
/// fire, frames asked for are delivered, dirty windows draw. Returns the frames drawn.
fn pass(src: &Source, vcx: &mut VisualTestContext, ms: u64) -> u64 {
    let before = renders();
    for _ in 0..ms / 100 {
        src.skew(100);
        vcx.executor().advance_clock(Duration::from_millis(100));
        vcx.run_until_parked();
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    renders() - before
}

/// The larder stack's page, settled: its first batches in, its fades done.
fn larder(cx: &mut TestAppContext) -> (Source, Entity<Root>, &mut VisualTestContext) {
    let (src, root, vcx) = window(cx, "larder");
    pass(&src, vcx, 5_000);
    (src, root, vcx)
}

/// The page shows a countdown that moves with time alone (else these tests prove nothing): the dozing instance.
fn counting_down(root: &Entity<Root>, vcx: &mut VisualTestContext) -> String {
    vcx.update(|_, cx| {
        let r = root.read(cx);
        let k = r.key().cloned().unwrap();
        let st = r.hub.stack(&k).unwrap();
        st.words()
            .instances
            .iter()
            .find(|v| v.state == doze_display::State::Idle && v.idle_since.is_some())
            .map(|v| v.name.clone())
            .expect("larder has a dozing instance")
    })
}

#[gpui::test]
fn an_untouched_window_draws_no_frames_while_its_countdown_runs(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    counting_down(&root, vcx);
    assert!(
        !vcx.update(|w, _| w.is_window_active()),
        "not the key window"
    );
    assert_eq!(
        pass(&src, vcx, 30_000),
        0,
        "frames drawn by an untouched window"
    );
}

#[gpui::test]
fn a_window_in_use_counts_down_then_stops_drawing_when_left_alone(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    counting_down(&root, vcx);
    vcx.update(|w, _| w.activate_window());
    vcx.run_until_parked();
    // In use: the countdown is read again every second, and each change is a frame.
    let used = pass(&src, vcx, 10_000);
    assert!(used >= 5, "{used} frames in 10 s of use");
    // Left alone: once the window is no longer in use, nothing more.
    pass(&src, vcx, attention::FOR.as_millis() as u64);
    assert_eq!(pass(&src, vcx, 30_000), 0, "frames drawn once left alone");
    // A touch brings it back at once.
    vcx.simulate_mouse_move(point(px(400.), px(300.)), None, gpui::Modifiers::default());
    assert!(
        pass(&src, vcx, 3_000) > 0,
        "a touch reads the countdown again"
    );
}

#[gpui::test]
fn an_instance_page_reads_its_countdown_once_a_second_not_more(cx: &mut TestAppContext) {
    let (src, root, vcx) = larder(cx);
    let name = counting_down(&root, vcx);
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
    let (src, root, vcx) = larder(cx);
    counting_down(&root, vcx);
    let before = renders();
    for i in 0..20 {
        let x = 300. + 10. * i as f32;
        vcx.simulate_mouse_move(point(px(x), px(300.)), None, gpui::Modifiers::default());
    }
    pass(&src, vcx, 3_000);
    assert_eq!(
        renders() - before,
        0,
        "frames drawn for a pointer over a window that is not key"
    );
}

#[gpui::test]
fn the_meters_stream_only_while_the_window_is_in_use(cx: &mut TestAppContext) {
    let (src, _root, vcx) = larder(cx);
    assert!(!src.hub.visible(), "an untouched window asks for no meters");
    vcx.update(|w, _| w.activate_window());
    vcx.run_until_parked();
    assert!(src.hub.visible(), "a window in use gets its meters");
    pass(&src, vcx, attention::FOR.as_millis() as u64 + 1_000);
    assert!(!src.hub.visible(), "left alone, the meters stop");
    vcx.simulate_mouse_move(point(px(400.), px(300.)), None, gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(src.hub.visible(), "a touch brings them back");
}
