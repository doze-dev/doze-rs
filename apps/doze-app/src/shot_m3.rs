//! `doze-app shot --act …` for Activity and Settings (M3): the states a fixture's machine cannot show by itself, made
//! up (`settings::Staged`), so a shot shows the real page in each state without touching this Mac: parts not in place
//! with Repair, what stacks left behind with the danger ask, an update out, none, or GitHub unreachable. For Activity:
//! a journal of a week written into the fixture's own (temporary) state folder, and a stretch under the pointer.

use std::time::{Duration, SystemTime};

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
        "journal" | "hover" => return Some(journal(root, act == "hover", cx)),
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
            elsewhere: Some("/Users/you/go/bin/doze".into()),
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

/// A week of the shown stack's services waking and sleeping, written as the daemon writes its journal, into the
/// fixture's own state folder; Activity reads it again. With `hover`, the pointer is put on the first service's first
/// stretch.
fn journal(root: &mut Root, hover: bool, cx: &mut Context<Root>) -> u64 {
    let Some(key) = root.key().cloned() else {
        return 0;
    };
    let Some(st) = root.hub().stack(&key) else {
        return 0;
    };
    let names: Vec<String> = st
        .instances
        .iter()
        .filter(|v| v.kind != "task")
        .map(|v| v.name.clone())
        .collect();
    let now = root.hub().now();
    let _ = std::fs::create_dir_all(key.run_dir());
    let _ = std::fs::write(key.journal(), lines(&names, now));
    if let Some(a) = root.pages.activity.clone() {
        a.update(cx, |a, cx| {
            a.read(cx);
            if hover {
                // Once the journal has been read and laid out (a read resets what the pointer was over).
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(1800))
                        .await;
                    let _ = this.update(cx, |a, cx| {
                        // The pointer on the middle of the first stretch, as a person would put it there.
                        let b = a.tracks.borrow().get(&0).copied();
                        let from = a
                            .row_at(0)
                            .and_then(|r| r.spans.first())
                            .map(|s| (s.from + s.to) / 2.0);
                        if let (Some(b), Some(f)) = (b, from) {
                            let x = b.origin.x + b.size.width * (f as f32);
                            a.point(0, gpui::point(x, b.origin.y + b.size.height / 2.), cx);
                        }
                    });
                })
                .detach();
            }
        });
    }
    2500
}

/// The journal's lines: each service wakes for a reason every few hours, is ready a moment later, and sleeps when
/// idle; now and then one crashes; the stack goes down one night.
fn lines(names: &[String], now: SystemTime) -> String {
    let at = |h: f64| {
        let t = now - Duration::from_secs_f64(h.max(0.0) * 3600.0);
        doze_control::Timestamp::from_system_time(t)
            .as_str()
            .to_owned()
    };
    let (mut out, mut seq) = (String::new(), 0_u64);
    let mut push = |h: f64, kind: &str, who: &str, extra: &str| {
        seq += 1;
        out.push_str(&format!(
            "{{\"seq\":{seq},\"time\":\"{}\",\"kind\":\"{kind}\",\"instance\":\"{who}\"{extra}}}\n",
            at(h)
        ));
    };
    let causes = [
        r#","cause":{"kind":"connection"}"#,
        r#","cause":{"kind":"dependency","by":"web"}"#,
        r#","cause":{"kind":"command"}"#,
    ];
    for (i, n) in names.iter().enumerate() {
        let mut h = 167.0 - (i % 7) as f64 * 1.3;
        let mut k = i;
        while h > 0.3 {
            let cause = causes.get(k % 3).copied().unwrap_or_default();
            push(h, "wake", n, cause);
            push(
                h - 0.01,
                "ready",
                n,
                &format!(",\"took\":{}", 120_000_000 + (k % 9) * 90_000_000),
            );
            if k % 23 == 7 {
                push(
                    h - 0.4,
                    "crash",
                    n,
                    r#","detail":"exit status 1: connection refused""#,
                );
            } else {
                push(
                    h - 0.3 - (k % 4) as f64 * 0.5,
                    "sleep",
                    n,
                    r#","cause":{"kind":"idle"}"#,
                );
            }
            h -= 3.5 + (k % 5) as f64 * 1.7;
            k += 1;
        }
    }
    let mut v: Vec<&str> = out.lines().collect();
    v.sort_by_key(|l| {
        l.split("\"time\":\"")
            .nth(1)
            .map(|s| s.get(..20).unwrap_or(s))
            .unwrap_or("")
    });
    v.join("\n") + "\n"
}

/// Activity as the shot asks: its span (`1h`, `24h`, `7d`), one service alone, the find box with words in it.
pub(crate) fn activity(
    root: &mut Root,
    span: Option<&str>,
    only: Option<&str>,
    find: Option<&str>,
    window: &mut Window,
    cx: &mut Context<Root>,
) {
    let Some(a) = root.pages.activity.clone() else {
        return;
    };
    a.update(cx, |a, cx| {
        if let Some(s) = span {
            a.set_span(doze_desk::activity::Span::parse(s), cx);
        }
        if let Some(n) = only {
            a.set_only(n, cx);
        }
        if let Some(f) = find {
            a.find_words(f, window, cx);
        }
    });
}
