//! A window left alone on a LIVE stack (docs/port/weight.md, "A live stack's idle window"): a real daemon's meters move
//! every second and it sends its instances again every few seconds with nothing changed, so a window that took each of
//! those in drew a frame every one to three seconds and kept the GPU driver's ~150 MB for good. Glanced at, it now
//! takes a change of state at once and the rest on a 30 s grid (`clock::glance`), so it draws at most about two
//! frames a minute, and still shows the meters moving.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::tests::window;
use super::{RENDERS, Root, Screen};
use crate::source::Source;

fn renders() -> u64 {
    RENDERS.with(std::cell::Cell::get)
}

/// Lets `secs` of a live stack pass a tenth of a second at a time: the app's time and the hub's clock on together, and
/// each second the daemon's moving meters (every third, its instances again). Returns the frames drawn.
fn live(src: &Source, vcx: &mut VisualTestContext, from: u64, secs: u64) -> u64 {
    let before = renders();
    for s in from..from + secs {
        for _ in 0..10 {
            src.skew(100);
            vcx.executor().advance_clock(Duration::from_millis(100));
            vcx.run_until_parked();
            vcx.update(|window, cx| window.simulate_next_frame(cx));
            vcx.run_until_parked();
        }
        assert!(src.live_second(s), "the fixture daemon sends its meters");
    }
    renders() - before
}

fn vitals(root: &Entity<Root>, vcx: &mut VisualTestContext) -> String {
    vcx.update(|_, cx| format!("{:?}", root.read(cx).overview.read(cx).home.vitals))
}

#[gpui::test]
fn a_window_glanced_at_on_a_live_stack_draws_twice_a_minute_and_still_moves(
    cx: &mut TestAppContext,
) {
    let (src, root, vcx) = window(cx, "larder");
    live(&src, vcx, 0, 5);
    let start = vitals(&root, vcx);
    // Three minutes of a stack that never sits still.
    let frames = live(&src, vcx, 5, 180);
    eprintln!("a live stack glanced at for 180 s: {frames} frames");
    assert!(
        frames <= 7,
        "{frames} frames in 180 s (two a minute, and a word turning)"
    );
    assert!(
        frames >= 3,
        "{frames} frames in 180 s: the meters never moved on screen"
    );
    assert_ne!(vitals(&root, vcx), start, "the vitals froze");
}

#[gpui::test]
fn a_service_page_on_a_live_stack_draws_twice_a_minute(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "larder");
    vcx.update(|window, cx| {
        root.update(cx, |r, cx| {
            let name = r.hub.stack(r.key().unwrap()).unwrap().instances[0]
                .name
                .clone();
            r.open_instance(&name, window, cx);
        });
    });
    live(&src, vcx, 0, 5);
    assert!(vcx.update(|_, cx| matches!(root.read(cx).screen, Screen::Instance(..))));
    let frames = live(&src, vcx, 5, 180);
    eprintln!("a service page on a live stack, 180 s: {frames} frames");
    assert!(frames <= 7, "{frames} frames in 180 s");
}

#[gpui::test]
fn a_hidden_window_on_a_live_stack_draws_nothing(cx: &mut TestAppContext) {
    let (src, _root, vcx) = window(cx, "larder");
    live(&src, vcx, 0, 5);
    vcx.simulate_visibility_change(gpui::WindowVisibility::Hidden);
    vcx.run_until_parked();
    // Hidden, the hub sends no meters: the daemon's pushes reach no stream, so this counts frames alone.
    let before = renders();
    for _ in 0..600 {
        src.skew(100);
        vcx.executor().advance_clock(Duration::from_millis(100));
        vcx.run_until_parked();
        vcx.update(|window, cx| window.simulate_next_frame(cx));
        vcx.run_until_parked();
    }
    assert_eq!(renders() - before, 0, "frames drawn while hidden");
}
