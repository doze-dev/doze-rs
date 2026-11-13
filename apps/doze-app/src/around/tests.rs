//! Around the clock with no window, over a fixture daemon (a throwaway home, never the real one): a crash after launch
//! is said in the app when there is no bundle, the menu's Needs you lists it with its fix, and pressing that fix from
//! the menu reaches the daemon.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::{Duration, Instant};

use doze_control::Frame;
use doze_desk::tray::Cmd;
use gpui::TestAppContext;

use crate::source::Source;

fn until(cx: &mut TestAppContext, what: &str, mut ok: impl FnMut(&mut TestAppContext) -> bool) {
    let start = Instant::now();
    while !ok(cx) {
        assert!(start.elapsed() < Duration::from_secs(10), "never: {what}");
        cx.update(super::on_batch);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The shop fixture, live, with the app's round-the-clock state started and primed.
fn started(cx: &mut TestAppContext) -> Source {
    let src = Source::fixture("shop", None).unwrap();
    let key = src.first.clone().unwrap();
    let start = Instant::now();
    while !src.hub.stack(&key).is_some_and(|s| s.link.is_live()) {
        assert!(start.elapsed() < Duration::from_secs(10), "no stack");
        std::thread::sleep(Duration::from_millis(20));
    }
    let (hub, facts) = (src.hub.clone(), src.facts.clone());
    cx.update(|cx| super::start(cx, &hub, &facts));
    cx.run_until_parked();
    src
}

/// api crashes, as the daemon would say it.
fn crash(src: &Source) {
    let key = src.first.clone().unwrap();
    let mut api = src
        .hub
        .stack(&key)
        .unwrap()
        .instance("api")
        .unwrap()
        .clone();
    api.state = "crashing".into();
    api.last_error = "panic: boom".into();
    (api.restart_count, api.restart_max) = (2, 5);
    src.push(&Frame {
        topic: "instances".into(),
        instances: vec![api],
        ..Frame::default()
    });
}

fn said(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        crate::act::acts(cx)
            .map(|a| {
                a.read(cx)
                    .notices
                    .recent()
                    .map(|n| n.text.clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

#[gpui::test]
fn a_crash_after_launch_is_said_in_the_app_once_and_needs_you_lists_it(cx: &mut TestAppContext) {
    let src = started(cx);
    assert!(said(cx).is_empty(), "what was so at launch is not news");
    crash(&src);
    until(cx, "the crash said", |cx| {
        said(cx)
            .iter()
            .any(|s| s == "api crashed · shop — panic: boom")
    });
    let red = cx.update(|cx| super::around(cx).map(|a| a.list.red));
    assert_eq!(red, Some(1));
    // The same episode: said once.
    for _ in 0..5 {
        cx.update(super::on_batch);
        cx.run_until_parked();
    }
    assert_eq!(
        said(cx)
            .iter()
            .filter(|s| s.starts_with("api crashed"))
            .count(),
        1
    );
}

#[gpui::test]
fn the_menus_fix_restarts_the_crashing_service(cx: &mut TestAppContext) {
    let src = started(cx);
    crash(&src);
    until(cx, "Needs you lists api", |cx| {
        cx.update(|cx| super::around(cx).is_some_and(|a| a.list.red == 1))
    });
    let fix = cx.update(|cx| {
        let a = super::around(cx).unwrap();
        let (stacks, now) = (a.hub.stacks(), a.hub.now());
        let (entries, _, _) = crate::tray::model(&stacks, &a.list, now);
        fn find(e: &[doze_desk::tray::Entry]) -> Option<Cmd> {
            e.iter().find_map(|e| match e {
                doze_desk::tray::Entry::Item {
                    cmd: c @ Cmd::Fix(_),
                    ..
                } => Some(c.clone()),
                doze_desk::tray::Entry::Submenu { entries, .. } => find(entries),
                _ => None,
            })
        }
        find(&entries)
    });
    let Some(Cmd::Fix(f)) = fix else {
        unreachable!("Needs you's line has its fix")
    };
    assert_eq!(f.1.label, "Restart");
    cx.update(|cx| super::picks::run(cx, Cmd::Fix(f)));
    let start = Instant::now();
    while !src.verbs().iter().any(|v| v == "restart api") {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            src.verbs()
        );
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}
