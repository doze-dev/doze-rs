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

/// Has another program hold `port` for a few seconds, as a stray dev server would (`nc`); the shot names it in the checks
/// before Up. Stopped by its own handle, never by name.
fn hold_port(port: u16) {
    let child = std::process::Command::new("nc")
        .args(["-l", "127.0.0.1", &port.to_string()])
        .spawn();
    if let Ok(mut c) = child {
        std::thread::sleep(Duration::from_millis(200));
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(8));
            let _ = c.kill();
            let _ = c.wait();
        });
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

/// Does `act`; says how long to wait before the shot.
fn start(root: &mut Root, act: &str, window: &mut Window, cx: &mut Context<Root>) -> u64 {
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
        "stop" => stack(cx, &key(root, "shop"), StackAction::Down),
        "remove" => stack(cx, &acme, StackAction::Remove),
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
        "busy" | "library-busy" => {
            stack(cx, &acme, StackAction::UpLast);
            return 500;
        }
        "start" => {
            if let Some(k) = &acme {
                acts.update(cx, |a, cx| a.choose_start(k, cx));
            }
        }
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
