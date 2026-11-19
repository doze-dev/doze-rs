//! The window wired end to end, in a gpui test window over a fixture daemon (a throwaway home, never the real one):
//! ⌘K's verbs reach acting and ask where Go asks, Needs you's fixes run, and the Library's space looks before Up.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{Root, Screen};
use crate::actions::{OpenBell, OpenPalette};
use crate::overlays::Overlay;
use crate::source::Source;

fn boot(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        let _ = crate::theme::apply(
            cx,
            crate::theme::Choice::default(),
            gpui::WindowAppearance::Dark,
        );
        let keys = crate::actions::keymap(cx);
        cx.bind_keys(keys);
    });
}

/// A window on the fixture `name`, once the hub has its stack live and the acme stack beside it.
pub(super) fn window<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
) -> (Source, Entity<Root>, &'a mut VisualTestContext) {
    boot(cx);
    let src = Source::fixture(name, None).unwrap();
    src.hub.set_visible(true);
    let key = src.first.clone().unwrap();
    let start = Instant::now();
    while !src.hub.stack(&key).is_some_and(|s| s.link.is_live())
        || !src.hub.stacks().iter().any(|s| s.name == "acme")
    {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{name}: no stack"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let (hub, first, facts) = (src.hub.clone(), src.first.clone(), src.facts.clone());
    let (root, vcx) = cx.add_window_view(|window, cx| Root::new(hub, first, facts, window, cx));
    vcx.update(|_, cx| root.update(cx, |r, cx| r.refresh(cx)));
    vcx.run_until_parked();
    (src, root, vcx)
}

/// What is open over the page.
fn over(root: &Entity<Root>, vcx: &mut VisualTestContext) -> &'static str {
    vcx.update(|_, cx| match &root.read(cx).overlay {
        Some(Overlay::Palette(_)) => "palette",
        Some(Overlay::Help) => "help",
        Some(Overlay::Dialog(_)) => "dialog",
        Some(Overlay::Preflight(_)) => "preflight",
        Some(Overlay::Start(_)) => "start",
        Some(Overlay::Bell(_)) => "bell",
        Some(Overlay::Admin(_)) => "admin",
        None => "",
    })
}

/// Waits (the daemon answers a verb after 700 ms) until the daemon was asked `verb`.
fn asked(src: &Source, vcx: &mut VisualTestContext, verb: &str) {
    let start = Instant::now();
    while !src.verbs().iter().any(|v| v == verb) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "never asked {verb}: {:?}",
            src.verbs()
        );
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn palette(root: &Entity<Root>, vcx: &mut VisualTestContext, words: &str) {
    vcx.update(|window, cx| root.update(cx, |r, cx| r.on_palette(&OpenPalette, window, cx)));
    vcx.run_until_parked();
    vcx.simulate_input(words);
    vcx.run_until_parked();
}

#[gpui::test]
fn palette_sleep_db_asks_as_go_does_and_yes_reaches_the_daemon(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "shop");
    palette(&root, vcx, "sleep db");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(over(&root, vcx), "dialog");
    let d = vcx.update(|_, cx| root.read(cx).dialog().unwrap().read(cx).ask.dialog.clone());
    // Go's sleep dialog for db, which api uses: sleep both (highlighted), db only, turn it off, or cancel.
    assert_eq!((d.kind.as_str(), d.title.as_str()), ("sleep", "Sleep db?"));
    let labels: Vec<&str> = d.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Sleep db and api", "Sleep db only", "Turn db off", "Cancel"]
    );
    assert!(d.needed && d.default == 0);
    assert!(src.verbs().is_empty());
    vcx.simulate_keystrokes("enter");
    asked(&src, vcx, "sleep api");
    asked(&src, vcx, "sleep db");
    assert_eq!(over(&root, vcx), "");
}

#[gpui::test]
fn palette_sleep_cache_goes_straight_as_go_does(cx: &mut TestAppContext) {
    // Go's row says "⏎ sleep now": nothing uses cache, so nothing is asked.
    let (src, root, vcx) = window(cx, "shop");
    palette(&root, vcx, "sleep ca");
    vcx.simulate_keystrokes("enter");
    asked(&src, vcx, "sleep cache");
    assert_eq!(over(&root, vcx), "");
}

#[gpui::test]
fn palette_cmd_enter_on_a_crashing_service_restarts_it(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "crash");
    palette(&root, vcx, "api");
    vcx.simulate_keystrokes("cmd-enter");
    asked(&src, vcx, "restart api");
}

#[gpui::test]
fn needs_you_restart_on_a_crashing_api_restarts_it(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "crash");
    let start = Instant::now();
    let at = loop {
        let at = vcx.update(|_, cx| {
            root.read(cx)
                .attn
                .list
                .items
                .iter()
                .position(|i| i.instance == "api")
        });
        if let Some(at) = at {
            break at;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "api never needed you"
        );
        vcx.update(|_, cx| root.update(cx, |r, cx| r.rebuild_attention(cx)));
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    };
    let fix = vcx.update(|_, cx| root.read(cx).attn.list.items[at].fixes[0].label.clone());
    assert_eq!(fix, "Restart");
    vcx.update(|window, cx| window.dispatch_action(Box::new(OpenBell), cx));
    vcx.run_until_parked();
    assert_eq!(over(&root, vcx), "bell");
    for _ in 0..at {
        vcx.simulate_keystrokes("down");
    }
    vcx.simulate_keystrokes("enter");
    asked(&src, vcx, "restart api");
}

#[gpui::test]
fn library_space_on_a_stopped_stack_looks_before_up(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    // acme starts api on 3000: something else holds it (this test, or whatever already does), so the checks stop.
    let _held = std::net::TcpListener::bind(("127.0.0.1", 3000)).ok();
    vcx.update(|window, cx| root.update(cx, |r, cx| r.go(Screen::Library, window, cx)));
    vcx.run_until_parked();
    let acme = vcx.update(|_, cx| {
        let l = root.read(cx).library.read(cx);
        l.lib
            .stacks
            .iter()
            .find(|s| s.name == "acme")
            .unwrap()
            .key
            .clone()
    });
    vcx.update(|_, cx| {
        let l = root.read(cx).library.clone();
        l.update(cx, |l, cx| l.select(&acme, cx));
    });
    vcx.simulate_keystrokes("space");
    let start = Instant::now();
    while over(&root, vcx) != "preflight" {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no checks before Up"
        );
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let blocked = vcx.update(|_, cx| match &root.read(cx).overlay {
        Some(Overlay::Preflight(p)) => p.read(cx).check.found.blocked,
        _ => false,
    });
    assert!(blocked);
}

/// acme's key, and Up on it run straight (the checks aside), until the fixture's stand-in doze has failed it.
fn failed_up(
    src: &Source,
    root: &Entity<Root>,
    vcx: &mut VisualTestContext,
) -> doze_desk::StackKey {
    let acme = src
        .hub
        .stacks()
        .into_iter()
        .find(|s| s.name == "acme")
        .unwrap()
        .key;
    let acts = vcx.update(|_, cx| root.read(cx).acts.clone());
    let k = acme.clone();
    vcx.update(|_, cx| {
        acts.update(cx, |a, cx| {
            a.run_stack(&k, doze_desk::act::StackAction::UpLast, cx);
        });
    });
    let toasts = |vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| {
            acts.read(cx)
                .notices
                .toasts()
                .iter()
                .map(|t| t.notice.text.clone())
                .collect::<Vec<_>>()
        })
    };
    assert!(toasts(vcx).iter().any(|t| t.contains("bringing acme up")));
    let start = Instant::now();
    while vcx.update(|_, cx| !acts.read(cx).failed.contains_key(&acme)) {
        assert!(start.elapsed() < Duration::from_secs(15), "Up never failed");
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    // The toast that said it was under way stays as an ordinary toast (Go `s.Say`); the failure adds none: it is the
    // banner's and Needs you's.
    assert_eq!(toasts(vcx), ["bringing acme up as last time…"]);
    acme
}

/// Waits until Needs you holds acme's failure.
fn failure_listed(root: &Entity<Root>, vcx: &mut VisualTestContext) {
    let start = Instant::now();
    while !vcx.update(|_, cx| {
        root.read(cx)
            .attn
            .list
            .items
            .iter()
            .any(|i| i.kind == "failed")
    }) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no failure in Needs you"
        );
        vcx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    vcx.run_until_parked();
}

#[gpui::test]
fn a_failed_up_on_its_page_is_the_banner_alone(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "shop");
    let acme = src
        .hub
        .stacks()
        .into_iter()
        .find(|s| s.name == "acme")
        .unwrap()
        .key;
    vcx.update(|window, cx| root.update(cx, |r, cx| r.go(Screen::Stack(acme), window, cx)));
    vcx.run_until_parked();
    failed_up(&src, &root, vcx);
    failure_listed(&root, vcx);
    let alerts = vcx.update(|_, cx| root.read(cx).attn.alerts.len());
    assert_eq!(alerts, 0, "the page's banner says it; no alert over it");
}

#[gpui::test]
fn a_failed_up_elsewhere_alerts(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "shop");
    vcx.update(|window, cx| root.update(cx, |r, cx| r.go(Screen::Library, window, cx)));
    vcx.run_until_parked();
    failed_up(&src, &root, vcx);
    failure_listed(&root, vcx);
    let kinds: Vec<&str> =
        vcx.update(|_, cx| root.read(cx).attn.alerts.iter().map(|a| a.kind).collect());
    assert_eq!(kinds, ["failed"]);
}
