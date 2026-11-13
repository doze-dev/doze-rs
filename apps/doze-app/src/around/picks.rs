//! A line picked in the menu-bar item's menu (Go `app/tray.go`'s handlers), and a fix pressed from outside the window
//! (Go `FixItem`). Everything goes through acting, as the window's buttons do: a stack's Up looks first, a sleep that
//! would affect others asks; what asks opens the window to ask there (`super::asked`).

use doze_control::Verb;
use doze_desk::StackKey;
use doze_desk::act::StackAction;
use doze_desk::attention::{Fix, Item};
use doze_desk::notify::{Outside, outside};
use doze_desk::tray::{Cmd, StackCmd};
use gpui::App;

use super::{around, notes};
use crate::actions::{OpenBell, OpenPalette};
use crate::shell::Screen;

/// The menu's line `id` was chosen.
pub(super) fn picked(cx: &mut App, id: u32) {
    let Some(cmd) = around(cx)
        .and_then(|a| a.menu.as_ref())
        .and_then(|m| m.cmd(id))
    else {
        return;
    };
    run(cx, cmd);
}

pub(super) fn run(cx: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::OpenDoze => {
            crate::app::open(cx);
        }
        Cmd::Palette => cx.dispatch_action(&OpenPalette),
        // Settings and its About (where Check for Updates lives) are one page, at its link.
        Cmd::Settings | Cmd::Updates => crate::app::follow_args(cx, &["doze://settings".into()]),
        Cmd::Quit => cx.quit(),
        Cmd::OpenStack(k) => notes::show_instance(cx, &key_str(&k), ""),
        Cmd::OpenUrl(u) => cx.open_url(&u),
        Cmd::Stack(k, c) => stack(cx, &k, c),
        Cmd::Service(k, name, verb) => acts(cx, |a, cx| a.verb(&k, &name, verb, cx)),
        Cmd::Instance(k, name) => notes::show_instance(cx, &key_str(&k), &name),
        Cmd::Logs(k, None) => go(cx, Screen::Logs(k)),
        Cmd::Logs(k, Some(name)) => {
            if let Some(h) = crate::app::open(cx) {
                let _ = h.update(cx, |root, window, cx| {
                    root.go(Screen::Instance(k, name), window, cx);
                    root.open_tab(Some("logs"), window, cx);
                });
            }
        }
        Cmd::Fix(fix) => {
            let (it, f) = *fix;
            if !press(cx, &it, &f) {
                show(cx, &it);
            }
        }
        Cmd::Show(it) => show(cx, &it),
        Cmd::Bell => {
            if let Some(h) = crate::app::open(cx) {
                let _ = h.update(cx, |root, window, cx| root.on_bell(&OpenBell, window, cx));
            }
        }
    }
}

fn key_str(k: &StackKey) -> String {
    k.dir().to_string_lossy().into_owned()
}

/// A stack's action from its submenu: Sleep everything sends `sleep` with no target (Go `Do(key, "sleep", "")`);
/// Restart goes straight to the CLI, as Go's menu did; Down asks only where the window would; Up looks first.
fn stack(cx: &mut App, k: &StackKey, c: StackCmd) {
    let k = k.clone();
    match c {
        StackCmd::SleepAll => acts(cx, |a, cx| a.verb(&k, "", Verb::Sleep, cx)),
        StackCmd::Restart => acts(cx, |a, cx| a.run_stack(&k, StackAction::Restart, cx)),
        StackCmd::Down => acts(cx, |a, cx| a.stack(&k, StackAction::Down, cx)),
        StackCmd::UpLast => acts(cx, |a, cx| a.stack(&k, StackAction::UpLast, cx)),
        StackCmd::UpAll => acts(cx, |a, cx| a.stack(&k, StackAction::UpAll, cx)),
    }
}

fn acts(cx: &mut App, f: impl FnOnce(&mut crate::act::Acts, &mut gpui::Context<crate::act::Acts>)) {
    if let Some(a) = crate::act::acts(cx) {
        a.update(cx, f);
    }
}

fn go(cx: &mut App, screen: Screen) {
    if let Some(h) = crate::app::open(cx) {
        let _ = h.update(cx, |root, window, cx| root.go(screen, window, cx));
    }
}

/// The page that explains an item: its instance, its stack, or Needs you itself.
fn show(cx: &mut App, it: &Item) {
    match &it.show {
        Some(p) if p.page == "instance" || p.page == "home" => {
            notes::show_instance(cx, &p.key, &p.instance);
        }
        _ => run(cx, Cmd::Bell),
    }
}

/// Presses `f` on `it` from outside the window; `false` when it asks first, so its page should open instead.
pub(super) fn press(cx: &mut App, it: &Item, f: &Fix) -> bool {
    match outside(it, f) {
        Outside::Page => return false,
        Outside::Verb(k, name, v) => acts(cx, |a, cx| a.run_verbs(&k, v, vec![name], cx)),
        Outside::Holder(k, name, pid) => acts(cx, |a, cx| a.stop_holder_for(&k, &name, pid, cx)),
        Outside::Stack(k, action) => acts(cx, |a, cx| a.stack(&k, action, cx)),
        Outside::Dismiss(k) => {
            let dir = key_str(&k);
            if let Some(a) = around(cx) {
                a.steady.quiet(|i| i.key == dir);
            }
            acts(cx, |a, cx| a.dismiss_failure(&k, cx));
        }
    }
    true
}
