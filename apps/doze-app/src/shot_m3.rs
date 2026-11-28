//! `doze-app shot --act …` for Settings (M3): the states a fixture's machine cannot show by itself, made up
//! (`settings::Staged`), so a shot shows the real page in each state without touching this Mac: parts not in place
//! with Repair, what stacks left behind with the danger ask, an update out, none, or GitHub unreachable.

use doze_desk::library::{Check, Doctor};
use doze_desk::settings::leftovers::{Authority, Machine, Proc, StateDir};
use doze_desk::settings::{DozeAt, Update};
use gpui::{Context, Window};

use crate::settings::Staged;
use crate::shell::Root;

/// Does `act` if it is one of these; says how long to wait before the shot.
pub(crate) fn start(
    root: &mut Root,
    act: &str,
    window: &mut Window,
    cx: &mut Context<Root>,
) -> Option<u64> {
    let staged = match act {
        "repair" | "repairing" => unset(true),
        "repair-terminal" => unset(false),
        "left" | "danger" | "left-many" => Staged {
            machine: Some(machine(act == "left-many")),
            admin: true,
            ..Staged::default()
        },
        "update-new" => update("0.3.0", ""),
        "update-none" => update(crate::VERSION.trim_start_matches('v'), ""),
        "update-offline" => update("", "could not reach GitHub to check"),
        "run-sheet" | "run-done" => return Some(run_sheet(root, act == "run-done", window, cx)),
        _ => return None,
    };
    let s = root.pages.settings.clone()?;
    s.update(cx, |s, cx| {
        s.staged = staged;
        s.left = None;
        let at = s.section;
        s.show(at, false, cx);
        s.look(cx);
        s.check(true, cx);
        if act == "repairing" {
            s.repairing = true;
        }
        if matches!(act, "left" | "danger" | "left-many") {
            s.scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(-10_000.)));
        }
        if act == "danger" {
            s.asking = s
                .left
                .as_ref()
                .and_then(|l| l.iter().find(|x| x.danger))
                .map(|x| x.id.clone())
                .unwrap_or_default();
        }
    });
    let _ = window;
    Some(300)
}

/// A machine with its names, a domain and the PATH not in place (and a stale route), Repair offered when `admin`.
fn unset(admin: bool) -> Staged {
    let check = |section: &'static str, name: &str, ok: bool, text: &str| Check {
        name: name.into(),
        ok,
        text: text.into(),
        section,
    };
    Staged {
        doctor: Some(Doctor {
            checks: vec![
                check(
                    "names",
                    "network",
                    false,
                    "the doze network daemon is not running",
                ),
                check(
                    "names",
                    "resolver route",
                    true,
                    "/etc/resolver/doze → 127.0.0.1:5323",
                ),
                check(
                    "domains",
                    "route shop.test",
                    false,
                    "/etc/resolver/shop.test missing — shop.test will not resolve here",
                ),
                check(
                    "domains",
                    "certificate authority",
                    true,
                    "trusted in the System keychain · doze local CA · expires 2036-11-01",
                ),
                check(
                    "domains",
                    "stale route old-shop.test",
                    false,
                    "no stack declares old-shop.test any more; doze trust removes its route",
                ),
            ],
        }),
        at: Some(DozeAt {
            on_path: None,
            own: Some("/Applications/Doze.app/Contents/MacOS/doze".into()),
        }),
        machine: Some(Machine {
            home: "/Users/you/.doze".into(),
            routes: vec!["old-shop.test".into()],
            admin,
            user_home: Some("/Users/you".into()),
            ..Machine::default()
        }),
        admin,
        ..Staged::default()
    }
}

/// What a made-up machine left behind: a daemon for a deleted folder, an orphaned engine (nine of them when `many`),
/// a state folder, a stale route, the authority still trusted.
fn machine(many: bool) -> Machine {
    let home = std::path::PathBuf::from("/Users/you/.doze");
    let plugin = |pid: u32| Proc {
        pid,
        ppid: 1,
        since: "Nov 12 09:14".into(),
        exe: "/Users/you/.doze/modules/doze/valkey/8.0.2/bin/valkey-plugin".into(),
        ..Proc::default()
    };
    let mut procs = vec![Proc {
        pid: 4182,
        ppid: 1,
        since: "Nov 11 18:02".into(),
        exe: "/opt/homebrew/bin/doze".into(),
        daemon: "/Users/you/code/old-shop/doze.hcl".into(),
        config_there: false,
    }];
    procs.extend((0..if many { 9 } else { 1 }).map(|i| plugin(4190 + i)));
    Machine {
        known: Vec::new(),
        procs,
        dirs: vec![StateDir {
            dir: home.join("projects/old-shop-1a2b3c"),
            listed: false,
            running: false,
            size: "412 MB".into(),
        }],
        routes: vec!["old-shop.test".into()],
        authority: Some(Authority {
            dir: home.join("ca"),
            trusted: true,
            used: false,
        }),
        admin: true,
        user_home: Some("/Users/you".into()),
        home,
    }
}

fn update(latest: &str, error: &str) -> Staged {
    let current = "0.2.1".to_owned();
    Staged {
        update: Some(Update {
            newer: !latest.is_empty() && doze_desk::settings::update::older(&current, latest),
            latest: latest.into(),
            published: if latest.is_empty() {
                String::new()
            } else {
                "10 Nov 2026".into()
            },
            url: "https://github.com/doze-dev/doze/releases/latest".into(),
            error: error.into(),
            checked: if error.is_empty() {
                "14:05".into()
            } else {
                String::new()
            },
            current,
            dev: false,
        }),
        ..Staged::default()
    }
}

/// The run sheet over Settings › Machine: Repair's plan for a machine with its parts not in place, before it runs, or
/// after a run that printed and ended well (made up: a shot runs nothing).
fn run_sheet(root: &mut Root, ran: bool, window: &mut Window, cx: &mut Context<Root>) -> u64 {
    let steps = vec![
        doze_desk::settings::Step {
            what: "the .doze names".into(),
            dir: None,
            args: vec!["dns-setup".into()],
            script: String::new(),
            prune: false,
        },
        doze_desk::settings::Step {
            what: "shop's domains".into(),
            dir: Some(std::env::temp_dir().join("shop")),
            args: vec!["trust".into()],
            script: String::new(),
            prune: false,
        },
        doze_desk::settings::Step {
            what: "put doze on your PATH".into(),
            dir: None,
            args: Vec::new(),
            script: "mkdir -p /usr/local/bin\nln -sf '/Applications/Doze.app/Contents/MacOS/doze' /usr/local/bin/doze"
                .into(),
            prune: false,
        },
    ];
    root.open_admin(doze_desk::settings::Job::Repair(steps), window, cx);
    if let Some(a) = root.pages.admin.clone() {
        a.update(cx, |v, cx| crate::overlays::admin::stage(v, ran, cx));
    }
    300
}
