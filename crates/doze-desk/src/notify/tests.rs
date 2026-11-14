//! Go's rules tests (`notify/rules_test.go`), each held as it was, and the fix and notification words.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::StackKey;
use crate::act::StackAction;
use crate::attention::{Attention, Fix, FixKind, Item};
use doze_control::{Event, InstanceView, Timestamp, Verb};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Go's `epoch`: 2026-10-11 14:04:40 UTC.
fn epoch() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_727_480)
}

fn ts(t: SystemTime) -> Value {
    serde_json::to_value(Timestamp::from_system_time(t)).unwrap()
}

fn on(key: &str, name: &str, vs: &[Value]) -> StackView {
    let mut st = StackView::new(StackKey::new(key));
    (st.name, st.dir, st.running) = (name.into(), format!("~/code/{name}"), true);
    st.instances = Arc::new(serde_json::from_value::<Vec<InstanceView>>(json!(vs)).unwrap());
    st
}

fn stack(vs: &[Value]) -> Vec<StackView> {
    vec![on("shop-1", "shop", vs)]
}

fn hidden(_: &str, _: &str) -> bool {
    false
}

fn shown(_: &str, _: &str) -> bool {
    true
}

fn fine() -> Value {
    json!({"name": "api", "kind": "app", "state": "active"})
}

fn crash() -> Value {
    json!({"name": "api", "kind": "app", "state": "crashing", "last_error": "panic: STRIPE_KEY is required\n  main.loadConfig",
        "restart_count": 3, "restart_max": 5, "next_restart": ts(epoch() + Duration::from_secs(4))})
}

#[test]
fn a_crash_is_said_once_per_episode_and_never_while_on_screen() {
    let mut r = Rules::new();
    let at = epoch();
    // The first look says nothing: what is already so is on screen, not news.
    assert!(
        r.next(&stack(&[crash()]), &hidden, at).is_empty(),
        "first look"
    );
    assert!(
        r.next(&stack(&[fine()]), &hidden, at).is_empty(),
        "recovery"
    );
    let n = r.next(&stack(&[crash()]), &hidden, at);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].title, "api crashed · shop");
    assert_eq!(n[0].body, "panic: STRIPE_KEY is required");
    assert_eq!(n[0].sub, "try 3 of 5 · next in 4s");
    assert_eq!(n[0].id, "shop-1/api/crash");
    // Still crashing, still the same episode: nothing more; nor between the tries, when it boots again.
    let later = at + Duration::from_secs(1);
    assert!(
        r.next(&stack(&[crash()]), &hidden, later).is_empty(),
        "second look"
    );
    let booting = json!({"name": "api", "kind": "app", "state": "booting"});
    r.next(&stack(&[booting]), &hidden, at);
    assert!(
        r.next(&stack(&[crash()]), &hidden, at).is_empty(),
        "the next try"
    );
    // Up again, then down again: a new episode.
    r.next(&stack(&[fine()]), &hidden, at);
    assert_eq!(
        r.next(&stack(&[crash()]), &hidden, at).len(),
        1,
        "new episode"
    );
    // On screen: seen, but not said; and not said later either.
    r.next(&stack(&[fine()]), &hidden, at);
    assert!(
        r.next(&stack(&[crash()]), &shown, at).is_empty(),
        "on screen"
    );
    assert!(
        r.next(&stack(&[crash()]), &hidden, at).is_empty(),
        "after looking away"
    );
    // Gave up: a failed instance says so.
    r.next(&stack(&[fine()]), &hidden, at);
    let gave_up =
        json!({"name": "api", "kind": "app", "state": "failed", "last_error": "exit status 2"});
    let n = r.next(&stack(&[gave_up]), &hidden, at);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].title, "api failed · shop");
    assert_eq!(n[0].sub, "it stopped and waits for you");
}

fn task(result: &str, exit: i64, ran: SystemTime, took_ms: u64) -> Value {
    json!({"name": "migrate", "kind": "task", "state": result,
        "task": {"result": result, "exit_code": exit, "ran_at": ts(ran), "took_ms": took_ms}})
}

#[test]
fn a_failed_task_is_said_once_per_run() {
    let mut r = Rules::new();
    let at = epoch();
    let ok = task("done", 0, at - Duration::from_secs(3600), 1200);
    let failed = task("failed", 1, at, 400);
    assert!(
        r.next(&stack(std::slice::from_ref(&failed)), &hidden, at)
            .is_empty(),
        "first look"
    );
    r.next(&stack(&[ok]), &hidden, at);
    let n = r.next(&stack(std::slice::from_ref(&failed)), &hidden, at);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].title, "migrate failed (exit 1) after 400ms · shop");
    assert_eq!(n[0].instance, "migrate");
    assert_eq!(
        n[0].body,
        "task \"migrate\" failed: exit 1; its last lines are on its page"
    );
    assert!(
        r.next(&stack(&[failed]), &hidden, at).is_empty(),
        "same run said again"
    );
    let again = task("failed", 1, at + Duration::from_secs(60), 400);
    assert_eq!(
        r.next(&stack(&[again]), &hidden, at).len(),
        1,
        "a new failed run"
    );
    let done = task("done", 0, at + Duration::from_secs(120), 900);
    assert!(
        r.next(&stack(&[done]), &hidden, at).is_empty(),
        "a run that went well"
    );
}

#[test]
fn a_stack_seen_later_is_primed_on_its_own() {
    let mut r = Rules::new();
    let at = epoch();
    let boom = json!({"name": "api", "kind": "app", "state": "crashing", "last_error": "boom"});
    r.next(&stack(std::slice::from_ref(&boom)), &hidden, at);
    // A second stack appears later, already crashing: on screen, not news.
    let mut both = stack(std::slice::from_ref(&boom));
    both.push(on("data-1", "data", &[boom]));
    assert!(
        r.next(&both, &hidden, at).is_empty(),
        "a stack first seen crashing"
    );
    let not_ready = json!({"name": "api", "kind": "app", "state": "failed", "last_error": "did not become ready within 1m30s"});
    r.next(&stack(&[fine()]), &hidden, at);
    let n = r.next(&stack(&[not_ready]), &hidden, at);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].title, "api did not become ready within 1m30s · shop");
    assert_eq!(
        (n[0].body.as_str(), n[0].sub.as_str()),
        ("its logs usually say why", "ready check in doze.hcl")
    );
}

fn event(t: SystemTime, kind: &str) -> Event {
    serde_json::from_value(json!({"seq": 1, "time": ts(t), "kind": kind, "instance": "api"}))
        .unwrap()
}

#[test]
fn a_wake_stuck_for_five_minutes_with_two_quiet_is_said_once() {
    let mut r = Rules::new();
    let at = epoch();
    let waking = json!({"name": "api", "kind": "app", "state": "booting"});
    let mut st = on("shop-1", "shop", &[waking]);
    r.next(std::slice::from_ref(&st), &hidden, at);
    st.events = Arc::new(vec![event(at, "wake")]);
    // Four minutes in: not yet.
    let soon = at + Duration::from_secs(4 * 60);
    assert!(
        r.next(std::slice::from_ref(&st), &hidden, soon).is_empty(),
        "four minutes"
    );
    let late = at + Duration::from_secs(6 * 60);
    let n = r.next(std::slice::from_ref(&st), &hidden, late);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].id, "shop-1/api/stuck");
    assert_eq!(n[0].title, "api is still waking · shop");
    assert!(
        n[0].body.starts_with("for 6m, with no word from it for 6m"),
        "{}",
        n[0].body
    );
    assert!(
        r.next(std::slice::from_ref(&st), &hidden, late).is_empty(),
        "once per wake"
    );
}

#[test]
fn instances_that_are_gone_end_their_episodes() {
    let mut r = Rules::new();
    let at = epoch();
    r.next(&stack(&[fine()]), &hidden, at);
    assert_eq!(r.next(&stack(&[crash()]), &hidden, at).len(), 1);
    r.next(&stack(&[]), &hidden, at);
    assert_eq!(
        r.next(&stack(&[crash()]), &hidden, at).len(),
        1,
        "back, crashing again"
    );
}

fn crash_item(fixes: Vec<Fix>) -> Item {
    Item {
        id: "instance:shop-1/api".into(),
        red: true,
        key: "shop-1".into(),
        instance: "api".into(),
        fixes,
        ..Item::default()
    }
}

#[test]
fn the_notification_carries_the_first_fix_it_can_press() {
    let restart = Fix::new("Restart", FixKind::Do).verb("restart");
    let holder = Fix {
        pid: 4121,
        ..Fix::new("Stop pid 4121 and start api", FixKind::Holder)
    };
    let note = Note {
        id: "shop-1/api/crash".into(),
        key: "shop-1".into(),
        instance: "api".into(),
        title: "api crashed · shop".into(),
        body: "boom".into(),
        sub: "try 3 of 5".into(),
        ..Note::default()
    };
    let a = Attention {
        items: vec![crash_item(vec![
            Fix::new("Logs", FixKind::Show),
            holder.clone(),
        ])],
        ..Attention::default()
    };
    let n = native(&note, &a);
    assert_eq!(
        (n.tag.as_str(), n.body.as_str()),
        ("shop-1/api/crash", "boom\ntry 3 of 5")
    );
    assert_eq!(n.action.as_deref(), Some("Stop pid 4121 and start api"));
    assert_eq!(n.fix.as_ref().map(|(_, f)| f.pid), Some(4121));
    let only_show = Attention {
        items: vec![crash_item(vec![Fix::new("Logs", FixKind::Show)])],
        ..Attention::default()
    };
    assert_eq!(native(&note, &only_show).action, None);
    let mut fixed = crash_item(vec![restart]);
    fixed.fixed = true;
    let gone = Attention {
        items: vec![fixed],
        ..Attention::default()
    };
    assert_eq!(
        native(&note, &gone).action,
        None,
        "a fixed item offers nothing"
    );
}

#[test]
fn outside_presses_only_what_asks_nothing() {
    let it = crash_item(Vec::new());
    let k = StackKey::new("shop-1");
    let press = |f: Fix| outside(&it, &f);
    assert_eq!(
        press(Fix::new("Restart", FixKind::Do).verb("restart")),
        Outside::Verb(k.clone(), "api".into(), Verb::Restart)
    );
    assert_eq!(
        press(Fix::new("Run again", FixKind::Do).verb("task")),
        Outside::Verb(k.clone(), "api".into(), Verb::Task)
    );
    assert_eq!(
        press(Fix {
            pid: 9,
            ..Fix::new("Stop", FixKind::Holder)
        }),
        Outside::Holder(k.clone(), "api".into(), 9)
    );
    assert_eq!(
        press(Fix::new("Start", FixKind::Stack).verb("up-last")),
        Outside::Stack(k.clone(), StackAction::UpLast)
    );
    assert_eq!(
        press(Fix::new("Restart", FixKind::Stack).verb("restart")),
        Outside::Stack(k.clone(), StackAction::Restart)
    );
    assert_eq!(
        press(Fix::new("Dismiss", FixKind::Dismiss)),
        Outside::Dismiss(k)
    );
    assert_eq!(
        press(Fix::new("Remove", FixKind::Stack).verb("remove")),
        Outside::Page,
        "remove asks first"
    );
    assert_eq!(
        press(Fix::new("Reset", FixKind::Do).verb("reset")),
        Outside::Page
    );
    assert_eq!(press(Fix::new("Set up", FixKind::Repair)), Outside::Page);
    assert_eq!(press(Fix::new("Clean up…", FixKind::Clean)), Outside::Clean);
}

#[test]
fn a_notes_id_names_its_stack_and_instance() {
    let place = |id: &str| place_of(id);
    let want = Some(("/h/projects/shop-1".to_owned(), "api".to_owned()));
    assert_eq!(place("/h/projects/shop-1/api/crash"), want);
    assert_eq!(place("/h/projects/shop-1/api/stuck"), want);
    let task = Some(("/h/projects/shop-1".to_owned(), "migrate".to_owned()));
    assert_eq!(
        place("/h/projects/shop-1/migrate/1791727480000000000/Failed"),
        task
    );
    assert_eq!(place("said/3"), None);
    assert_eq!(place("api/crash"), None);
}

#[test]
fn an_episode_that_ends_names_its_note_for_withdrawal() {
    let mut r = Rules::new();
    let at = epoch();
    r.next(&stack(&[fine()]), &hidden, at);
    assert_eq!(r.next(&stack(&[crash()]), &hidden, at).len(), 1);
    assert!(r.ended().is_empty(), "still crashing");
    r.next(&stack(&[fine()]), &hidden, at);
    assert_eq!(r.ended(), ["shop-1/api/crash", "shop-1/api/stuck"]);
    assert!(r.ended().is_empty(), "asked once");
    // Gone ends it too.
    r.next(&stack(&[crash()]), &hidden, at);
    r.next(&stack(&[]), &hidden, at);
    assert_eq!(r.ended(), ["shop-1/api/crash", "shop-1/api/stuck"]);
    // A failed run, then a good one: the failed run's note goes.
    r.next(
        &stack(&[task("done", 0, at - Duration::from_secs(60), 100)]),
        &hidden,
        at,
    );
    let n = r.next(&stack(&[task("failed", 1, at, 400)]), &hidden, at);
    assert_eq!(n.len(), 1);
    let id = n[0].id.clone();
    r.ended();
    r.next(
        &stack(&[task("done", 0, at + Duration::from_secs(60), 100)]),
        &hidden,
        at,
    );
    assert_eq!(r.ended(), [id]);
}
