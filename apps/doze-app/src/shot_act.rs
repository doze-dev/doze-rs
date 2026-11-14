//! `doze-app shot --act …` (M2): something done before the shot, through the same calls a key or a click makes, so
//! the shot shows the real dialog, sheet, toast, banner or page: a verb that asks, a stack action in flight or failed
//! (the fixture's stand-in doze takes two seconds, then fails as Go's fixture server does), a folder with no doze.hcl.

use std::path::PathBuf;
use std::time::Duration;

use doze_control::Verb;
use doze_desk::StackKey;
use doze_desk::act::StackAction;
use doze_desk::page::console::Action;
use gpui::{AsyncApp, Context, Window, WindowHandle};

use crate::shell::Root;

/// A stack's key, by its name.
fn key(root: &Root, name: &str) -> Option<StackKey> {
    root.hub()
        .stacks()
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.key)
}

/// The running stack shown.
fn shown(root: &Root) -> Option<StackKey> {
    root.key().cloned()
}

/// A folder for the empty page, under the system's temp folder; with a package.json and a compose file when `found`.
fn folder(found: bool) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("doze-app-shot-{}", std::process::id()))
        .join(if found { "shopfront" } else { "notes" });
    let _ = std::fs::create_dir_all(&dir);
    if found {
        let _ = std::fs::write(
            dir.join("package.json"),
            r#"{"name":"shopfront","scripts":{"dev":"vite"}}"#,
        );
        let _ = std::fs::write(
            dir.join("docker-compose.yml"),
            "services:\n  db:\n    image: postgres:17\n",
        );
    }
    dir
}

/// Has another program hold `port` for a few seconds, as a stray dev server would (`nc`); the checks before Up name it.
/// Stopped by its own pid, never by name.
fn hold_port(port: u16) {
    // The shell stops its own `nc` (by the pid it started) after a few seconds, even when the shot has exited first.
    let script = format!("nc -l 127.0.0.1 {port} & p=$!; sleep 6; kill $p");
    if std::process::Command::new("sh")
        .args(["-c", &script])
        .spawn()
        .is_ok()
    {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn produce() -> Action {
    Action {
        id: "produce".into(),
        label: "Produce a message".into(),
        key: 'P',
        destructive: false,
        hint: "message value".into(),
        resource: "orders".into(),
    }
}

/// A stack action from its Library row, as its switch, its menu or ⌫ asks (the row is selected).
fn lib_act(root: &mut Root, name: &str, action: StackAction, cx: &mut Context<Root>) {
    let row = root
        .library
        .read(cx)
        .lib
        .stacks
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.key.clone());
    if let Some(k) = row {
        root.library.update(cx, |l, cx| l.act(&k, action, cx));
    }
}

/// Does `act`; says how long to wait before the shot.
fn start(root: &mut Root, act: &str, window: &mut Window, cx: &mut Context<Root>) -> u64 {
    if let Some(wait) = crate::shot_m3::start(root, act, window, cx) {
        return wait;
    }
    let acts = root.acts.clone();
    let (here, acme) = (shown(root), key(root, "acme"));
    let verb = |cx: &mut Context<Root>, name: &str, v: Verb| {
        if let Some(k) = &here {
            acts.update(cx, |a, cx| a.verb(k, name, v, cx));
        }
    };
    let stack = |cx: &mut Context<Root>, k: &Option<StackKey>, a: StackAction| {
        if let Some(k) = k {
            acts.update(cx, |x, cx| x.stack(k, a, cx));
        }
    };
    match act {
        "sleep" => verb(cx, "db", Verb::Sleep),
        "restart" => verb(cx, "db", Verb::Restart),
        "reset" | "reset-typed" => verb(cx, "db", Verb::Reset),
        "verb-busy" => {
            verb(cx, "cache", Verb::Sleep);
            return 150;
        }
        "stop" => lib_act(root, "shop", StackAction::Down, cx),
        "remove" => lib_act(root, "acme", StackAction::Remove, cx),
        "library-busy" => {
            lib_act(root, "acme", StackAction::UpLast, cx);
            return 500;
        }
        "action" | "action-typed" => {
            if let Some(k) = &here {
                acts.update(cx, |a, cx| a.action(k, "events", &produce(), cx));
            }
        }
        "copy" => window.dispatch_action(Box::new(crate::actions::CopyAddress), cx),
        "toast-fail" | "recent" | "failed" => {
            stack(cx, &acme, StackAction::UpLast);
            return 3200;
        }
        "preflight" => {
            hold_port(3000);
            stack(cx, &acme, StackAction::UpLast);
            return 1200;
        }
        "busy" => {
            stack(cx, &acme, StackAction::UpLast);
            return 500;
        }
        "start" => {
            if let Some(k) = &acme {
                acts.update(cx, |a, cx| a.choose_start(k, cx));
            }
        }
        // Tab from the page: the keys go to the first name, which selects its service (Go `Name` onFocus).
        "tab" | "tab-enter" => {
            window.focus_next(cx);
            window.focus_next(cx);
        }
        // The Library's machine line with two things left behind and Repair to offer.
        "library-left" => root.library.update(cx, |l, cx| {
            l.left = 2;
            l.repairable = true;
            cx.notify();
        }),
        "empty" | "add" => root.open_dir(&folder(false), None, window, cx),
        "empty-found" => root.open_dir(&folder(true), None, window, cx),
        _ => eprintln!("doze-app shot: no act {act}"),
    }
    300
}

/// What follows once it has settled: the name typed, the Recent popover, tile 2.
fn then(root: &mut Root, act: &str, window: &mut Window, cx: &mut Context<Root>) {
    let typed = match act {
        "reset-typed" => Some("db"),
        "action-typed" => Some(r#"{"id": 42}"#),
        _ => None,
    };
    if let (Some(text), Some(d)) = (typed, root.dialog()) {
        d.update(cx, |d, cx| d.type_in(text, window, cx));
    }
    match act {
        "recent" => root.acts.update(cx, |a, cx| a.toggle_recent(cx)),
        "add" => window.dispatch_action(Box::new(crate::actions::Way2), cx),
        // ⏎ on the name or the card Tab reached: a card is selected by it.
        "tab-enter" => window.dispatch_action(Box::new(crate::actions::Confirm), cx),
        _ => {}
    }
}

/// Runs `--act`, if one was asked for, and waits for it to settle.
pub(crate) async fn run(cx: &mut AsyncApp, h: WindowHandle<Root>, act: Option<&str>) {
    let Some(act) = act else { return };
    let wait = h
        .update(cx, |root, window, cx| start(root, act, window, cx))
        .unwrap_or(0);
    cx.background_executor()
        .timer(Duration::from_millis(wait))
        .await;
    let _ = h.update(cx, |root, window, cx| then(root, act, window, cx));
    cx.background_executor()
        .timer(Duration::from_millis(250))
        .await;
}
