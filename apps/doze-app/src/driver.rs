//! A scripted driver for development (the `fixtures` feature only): `DOZE_APP_SCRIPT=FILE` names a file of steps the
//! app plays against its own window, so a click, a key or a hover can be tried on a real daemon without the system's
//! Accessibility permission (which a script driving the mouse would need). What happens is written to
//! `DOZE_APP_SCRIPT_LOG` (else `FILE.log`), a line per step or event: `<ms since start> <kind> <text>`.
//!
//! The elements a step can name are the probes the pages leave on what is worth pressing ([`Probe::probe`]): an id
//! and the words it shows. While a script runs, nothing leaves the app: a URL to open, a file to show and text to copy
//! are written to the log instead (`open …`, `copy …`), and every toast is (`toast …`).
//!
//! Steps, one a line (`#` starts a comment):
//!
//! | step | does |
//! |---|---|
//! | `wait MS` | waits |
//! | `wait-for WORDS [MS]` | waits (10 s unless MS) until a probe's id or words contain WORDS |
//! | `click WORDS` / `dclick WORDS` / `hover WORDS` | presses (twice) or points at the first probe that names WORDS |
//! | `click-at X Y` / `move X Y` | presses or points at a place in the window |
//! | `key KEYS` | types a keystroke as the keymap writes it (`space`, `cmd-k`, `2`) |
//! | `assert WORDS` / `refute WORDS` | a probe or a toast says WORDS (or none does); a miss is logged as `FAIL` |
//! | `watch IDS MS` | for MS, writes each change in what the probes whose id starts with one of IDS (comma separated) say, or where they are |
//! | `mark LABEL` | writes LABEL with the time, to line frames up with |
//! | `probes` | writes every probe shown, with its place |
//! | `quit` | quits the app |

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::{
    App, AppContext as _, AsyncApp, Bounds, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, point, px,
};

/// The log, once a script runs.
struct Log {
    file: std::fs::File,
    t0: Instant,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

thread_local! {
    /// The probes of the frame drawn last, and the number of that frame.
    static PROBES: RefCell<(u64, HashMap<String, (u64, Bounds<Pixels>, String)>)> = RefCell::new((0, HashMap::new()));
}

/// Whether a script is playing: then nothing leaves the app.
pub(crate) fn active() -> bool {
    LOG.get().is_some()
}

/// Writes a line to the log.
pub(crate) fn note(kind: &str, text: &str) {
    if let Some(l) = LOG.get()
        && let Ok(mut l) = l.lock()
    {
        let ms = l.t0.elapsed().as_millis();
        let _ = writeln!(l.file, "{ms:>7} {kind} {text}");
    }
}

/// A new frame is being drawn: probes from older ones no longer count.
pub(crate) fn frame() {
    if active() {
        PROBES.with(|p| p.borrow_mut().0 += 1);
    }
}

/// Records where a probe was drawn this frame.
pub(crate) fn record(id: &str, bounds: Bounds<Pixels>, text: &str) {
    if !active() {
        return;
    }
    PROBES.with(|p| {
        let mut p = p.borrow_mut();
        let f = p.0;
        p.1.insert(id.to_owned(), (f, bounds, text.to_owned()));
    });
}

/// The probes of the last frame: id, place, words.
fn shown() -> Vec<(String, Bounds<Pixels>, String)> {
    PROBES.with(|p| {
        let p = p.borrow();
        let mut v: Vec<_> =
            p.1.iter()
                .filter(|(_, (f, _, _))| *f == p.0)
                .map(|(id, (_, b, t))| (id.clone(), *b, t.clone()))
                .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    })
}

/// The first probe whose id is `words`, else whose id or words contain them.
fn find(words: &str) -> Option<(String, Bounds<Pixels>, String)> {
    let all = shown();
    all.iter()
        .find(|(id, _, _)| id == words)
        .or_else(|| all.iter().find(|(_, _, t)| t == words))
        .or_else(|| {
            all.iter()
                .find(|(id, _, t)| id.contains(words) || t.contains(words))
        })
        .cloned()
}

/// The toasts said so far, for `assert`.
fn toasts() -> &'static Mutex<Vec<String>> {
    static T: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    T.get_or_init(Mutex::default)
}

/// A toast was said.
pub(crate) fn toast(text: &str) {
    if active() {
        note("toast", text);
        if let Ok(mut t) = toasts().lock() {
            t.push(text.to_owned());
        }
    }
}

/// Starts the script `DOZE_APP_SCRIPT` names, if one does.
pub(crate) fn start(cx: &mut App) {
    let Some(script) = std::env::var_os("DOZE_APP_SCRIPT").map(PathBuf::from) else {
        return;
    };
    let steps = match std::fs::read_to_string(&script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("doze-app: script {}: {e}", script.display());
            return;
        }
    };
    let log = std::env::var_os("DOZE_APP_SCRIPT_LOG")
        .map_or_else(|| script.with_extension("log"), PathBuf::from);
    let Ok(file) = std::fs::File::create(&log) else {
        eprintln!("doze-app: script log {}: cannot write", log.display());
        return;
    };
    let _ = LOG.set(Mutex::new(Log {
        file,
        t0: Instant::now(),
    }));
    note("start", &script.display().to_string());
    cx.spawn(async move |cx: &mut AsyncApp| {
        for (n, line) in steps.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            note("step", &format!("{}: {line}", n + 1));
            if !play(line, cx).await {
                break;
            }
        }
        note("end", "");
    })
    .detach();
}

/// Plays one step; `false` ends the script.
async fn play(line: &str, cx: &mut AsyncApp) -> bool {
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    match verb {
        "wait" => sleep(cx, rest.parse().unwrap_or(0)).await,
        "wait-for" => wait_for(rest, cx).await,
        "watch" => watch(rest, cx).await,
        "click" | "dclick" | "hover" => match find(rest) {
            Some((id, b, _)) => {
                let at = b.center();
                note(
                    "at",
                    &format!("{id} {:.0},{:.0}", f32::from(at.x), f32::from(at.y)),
                );
                pointer(cx, at, verb).await;
            }
            None => note("FAIL", &format!("no probe names {rest:?}")),
        },
        "click-at" | "move" => {
            let mut n = rest
                .split_whitespace()
                .filter_map(|x| x.parse::<f32>().ok());
            let at = point(px(n.next().unwrap_or(0.)), px(n.next().unwrap_or(0.)));
            pointer(cx, at, if verb == "move" { "hover" } else { "click" }).await;
        }
        "key" => key(cx, rest),
        "assert" | "refute" => check(rest, verb == "assert"),
        "mark" => note(
            "mark",
            &format!(
                "{rest} @{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis())
            ),
        ),
        "probes" => {
            for (id, b, t) in shown() {
                let (o, s) = (b.origin, b.size);
                note(
                    "probe",
                    &format!(
                        "{id} @{:.0},{:.0} {:.0}x{:.0} {t:?}",
                        f32::from(o.x),
                        f32::from(o.y),
                        f32::from(s.width),
                        f32::from(s.height)
                    ),
                );
            }
        }
        "quit" => {
            note("end", "quit");
            cx.update(|cx| cx.quit());
            return false;
        }
        _ => note("FAIL", &format!("no step called {verb:?}")),
    }
    true
}

async fn sleep(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

/// `watch IDS MS`: for MS, writes each change of the words of the probes whose id starts with any of IDS (comma
/// separated), as it is drawn.
async fn watch(rest: &str, cx: &mut AsyncApp) {
    let (ids, ms) = rest.rsplit_once(' ').unwrap_or((rest, "3000"));
    let ids: Vec<&str> = ids.split(',').collect();
    let end = Instant::now() + Duration::from_millis(ms.parse().unwrap_or(3000));
    let mut last: HashMap<String, String> = HashMap::new();
    while Instant::now() < end {
        for (id, b, words) in shown() {
            let seen = format!(
                "{words:?} @{:.0},{:.0}",
                f32::from(b.origin.x),
                f32::from(b.origin.y)
            );
            if ids.iter().any(|p| id.starts_with(p)) && last.get(&id) != Some(&seen) {
                note("seen", &format!("{id} {seen}"));
                last.insert(id, seen);
            }
        }
        sleep(cx, 16).await;
    }
}

async fn wait_for(rest: &str, cx: &mut AsyncApp) {
    let (words, limit) = match rest.rsplit_once(' ') {
        Some((w, ms)) if ms.parse::<u64>().is_ok() => (w, ms.parse().unwrap_or(10_000)),
        _ => (rest, 10_000),
    };
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(limit) {
        if find(words).is_some() || said(words) {
            note(
                "found",
                &format!("{words} after {} ms", t.elapsed().as_millis()),
            );
            return;
        }
        sleep(cx, 50).await;
    }
    note("FAIL", &format!("{words:?} did not show in {limit} ms"));
}

fn said(words: &str) -> bool {
    toasts()
        .lock()
        .map(|t| t.iter().any(|x| x.contains(words)))
        .unwrap_or(false)
}

fn check(words: &str, want: bool) {
    let seen = find(words).is_some() || said(words);
    let kind = if seen == want { "ok" } else { "FAIL" };
    let what = if want { "shows" } else { "does not show" };
    note(kind, &format!("{what} {words:?}"));
}

/// Points at `at`, then presses there (once for `click`, twice for `dclick`).
async fn pointer(cx: &mut AsyncApp, at: Point<Pixels>, how: &str) {
    send(
        cx,
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
    );
    if how == "hover" {
        return;
    }
    let times = if how == "dclick" { 2 } else { 1 };
    for n in 1..=times {
        sleep(cx, 30).await;
        send(
            cx,
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: n,
                first_mouse: false,
            }),
        );
        sleep(cx, 30).await;
        send(
            cx,
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: n,
            }),
        );
    }
}

fn send(cx: &mut AsyncApp, ev: PlatformInput) {
    cx.update(|cx| {
        if let Some(h) = cx.try_global::<crate::app::State>().and_then(|s| s.window) {
            let _ = cx.update_window(h.into(), |_, window, cx| {
                window.dispatch_event(ev, cx);
            });
        }
    });
}

fn key(cx: &mut AsyncApp, keys: &str) {
    let Ok(k) = Keystroke::parse_with_os(keys, gpui::OperatingSystem::Mac) else {
        note("FAIL", &format!("not a keystroke: {keys:?}"));
        return;
    };
    cx.update(|cx| {
        if let Some(h) = cx.try_global::<crate::app::State>().and_then(|s| s.window) {
            let _ = cx.update_window(h.into(), |_, window, cx| {
                window.dispatch_keystroke(k, cx);
            });
        }
    });
}
