//! `doze-app bench`: the app's own timings, printed as `name value` lines for a script to read (docs/port/app/PLAN.md
//! §6). It starts as a menu-bar app, opens the window a second later as a tray click would, and reports how long the
//! first frame and the first frame with the stack on it took; then it stays idle for `--idle` seconds (for a memory
//! reading from outside: `footprint`, Activity Monitor's figure), closes the window, waits ten seconds and quits.
//! It never touches the doze home: its stack is a fixture.

use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{ActivationPolicy, App, AppContext as _, AsyncApp, QuitMode};

use crate::cli::Bench;
use crate::shell::Root;
use crate::theme::Choice;

pub(crate) fn run(b: Bench, launched: Instant) -> ExitCode {
    // No fixture: `$DOZE_HOME` when it is set (a fixture served from another process), else an empty folder; never the
    // real home by default.
    let home = std::env::var_os("DOZE_HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    let source = match crate::source_for(&b.from, home) {
        Ok(s) => Rc::new(s),
        Err(e) => {
            eprintln!("doze-app bench: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    // The bench's window sits on the owner's screen: it never takes their input (`crate::handsoff`).
    crate::handsoff::set();
    println!("pid {}", std::process::id());
    // The fixture daemon (a test double run in-process) is not part of a real launch: its cost is reported apart.
    println!("fixture_source_ms {}", launched.elapsed().as_millis());
    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            let t = Instant::now();
            println!("platform_ready_ms {}", launched.elapsed().as_millis());
            crate::app::boot_core(cx);
            cx.set_activation_policy(ActivationPolicy::Accessory);
            // The menu-bar item, Needs you and the notifications, as a launch has them, following the hub.
            crate::around::start(cx, &source.hub, &source.facts);
            let updates = source.hub.clone();
            cx.spawn(async move |cx: &mut AsyncApp| {
                while updates.updates().recv().await.is_ok() {
                    cx.update(crate::around::on_batch);
                }
            })
            .detach();
            println!("menu_bar_ready_ms {}", launched.elapsed().as_millis());
            // As a launch does: the window's libraries after the item is up.
            crate::app::boot_ui(cx, Choice::default());
            println!("ui_boot_ms {}", t.elapsed().as_millis());
            let (src, hub, first) = (
                source.clone(),
                source.hub.clone(),
                (source.first.clone(), source.facts.clone()),
            );
            let (logs, follow) = (b.logs, b.follow);
            let watch = Watch {
                idle: b.idle,
                closed: b.closed.unwrap_or(10),
                map: b.map,
                display: b.display,
                in_use: b.in_use,
                check: b.check,
                budget: b.budget,
            };
            cx.spawn(async move |cx: &mut AsyncApp| {
                let _keep = src.clone();
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if logs {
                    logs_bench(cx, src, follow).await;
                } else {
                    time_open(cx, hub, first, watch).await;
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    ExitCode::SUCCESS
}

/// How the window is opened and watched.
struct Watch {
    idle: u64,
    closed: u64,
    map: bool,
    /// Which display, by index (`--display`); the main one when none. A window the system hides (another app full
    /// screen over it, a display covered) is sent no frames, so its reading is not the idle window's: pick one in view.
    display: Option<usize>,
    /// Fails the run (exit 1) when the window, left alone, drew a frame, or its footprint passed `budget` MB
    /// (`--check`, `--budget MB`).
    check: bool,
    budget: Option<f64>,
    /// Counts the window as in use when it opens (`--in-use`), as a click on it would: its clocks run for
    /// `shell::attention::FOR`, then it is left alone.
    in_use: bool,
}

/// The window opened as a tray click opens it, timed to its first frame and to its first frame with the stack.
async fn time_open(
    cx: &mut AsyncApp,
    hub: doze_desk::Hub,
    (first, facts): (Option<doze_desk::StackKey>, crate::facts::Mode),
    w: Watch,
) {
    let t = Instant::now();
    let opened = cx.update(|cx| {
        let on = w.display.and_then(|i| cx.displays().get(i).map(|d| d.id()));
        let bounds = gpui::Bounds::centered(on, gpui::size(gpui::px(1280.), gpui::px(800.)), cx);
        cx.open_window(crate::app::options(bounds, true), |window, cx| {
            window
                .on_next_frame(move |_, _| println!("first_frame_ms {}", t.elapsed().as_millis()));
            cx.new(|cx| Root::new(hub.clone(), first.clone(), facts.clone(), window, cx))
        })
    });
    let Ok(h) = opened else { return };
    cx.update(|cx| crate::handsoff::after_open(h.into(), cx));
    hub.set_visible(true);
    loop {
        let has = h.update(cx, |root, _, cx| {
            root.refresh(cx);
            // The page it opens on has what it shows: a stack's page its stack, the Library (no stack named) at once.
            root.key().is_some() || root.screen == crate::shell::Screen::Library
        });
        if has.unwrap_or(true) || t.elapsed() > Duration::from_secs(10) {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(2))
            .await;
    }
    let _ = h.update(cx, |_, w, _| {
        let t2 = t;
        w.on_next_frame(move |_, _| {
            println!("first_frame_with_stack_ms {}", t2.elapsed().as_millis())
        });
        w.refresh();
    });
    if w.in_use {
        // As a click on it: the window counts as key and touched, so its clocks run.
        let _ = h.update(cx, |root, _, cx| {
            crate::shell::attention::key(true, cx.background_executor().now());
            root.wind(cx);
        });
    }
    if w.map {
        map_bench(cx, h).await;
    }
    let drawn = idle_frames(cx, if w.map { 0 } else { w.idle }).await;
    let fp = cx.background_executor().spawn(async { footprint() }).await;
    if let Some(mb) = fp {
        println!("idle_footprint_mb {mb:.0}");
    }
    println!("idle_done");
    if w.check {
        checked(drawn, fp, &w);
    }
    let _ = h.update(cx, |_, w, _| w.remove_window());
    hub.set_visible(false);
    cx.background_executor()
        .timer(Duration::from_secs(if w.map { 0 } else { w.closed }))
        .await;
    println!("closed_done");
}

/// Idles for `secs`, counting the frames drawn after the first three seconds (the page settling: its first batch, a
/// fade in): `idle_frames N`, and `idle_frame_ms T` for each of the first twenty, from the window's open. An idle
/// window draws none.
async fn idle_frames(cx: &mut AsyncApp, secs: u64) -> u64 {
    let t = Instant::now();
    let settle = Duration::from_secs(3).min(Duration::from_secs(secs));
    cx.background_executor().timer(settle).await;
    let start = crate::app::frames_drawn();
    let (mut last, mut shown, mut at) = (start, 0, 0);
    while t.elapsed() < Duration::from_secs(secs) {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        let now = crate::app::frames_drawn();
        if now != last {
            at = t.elapsed().as_millis();
            if shown < 20 {
                println!("idle_frame_ms {at}");
                shown += 1;
            }
        }
        last = now;
    }
    println!("idle_frames {}", last - start);
    println!("last_idle_frame_ms {at}");
    last - start
}

/// `--check`: a window left alone draws nothing, and stays under its budget. A failure ends the run with exit 1 (the
/// platform's quit would exit 0).
fn checked(drawn: u64, fp: Option<f64>, w: &Watch) {
    let mut bad = Vec::new();
    if drawn > 0 && !w.in_use {
        bad.push(format!(
            "{drawn} frames drawn by a window left alone (0 allowed)"
        ));
    }
    if let (Some(limit), Some(mb)) = (w.budget, fp)
        && mb > limit
    {
        bad.push(format!(
            "idle footprint {mb:.0} MB over the {limit:.0} MB budget"
        ));
    }
    if bad.is_empty() {
        println!("check ok");
        return;
    }
    for b in &bad {
        eprintln!("doze-app bench: {b}");
    }
    std::process::exit(1);
}

/// The process's footprint in megabytes, as `footprint` reads it.
pub(crate) fn footprint() -> Option<f64> {
    let out = std::process::Command::new("footprint")
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let after = text.split("Footprint:").nth(1)?;
    let mut it = after.split_whitespace();
    let n: f64 = it.next()?.parse().ok()?;
    match it.next()? {
        "KB" => Some(n / 1024.),
        "MB" => Some(n),
        "GB" => Some(n * 1024.),
        _ => None,
    }
}

/// The Map under load (`--map`; a build with `capture`, for drawing offscreen).
async fn map_bench(cx: &mut AsyncApp, h: gpui::WindowHandle<Root>) {
    #[cfg(feature = "capture")]
    crate::map::bench::run(cx, h).await;
    #[cfg(not(feature = "capture"))]
    {
        let _ = (cx, h);
        eprintln!("doze-app bench --map needs a build with the `capture` feature");
    }
}

/// The logs viewer under load (`--logs`; a build with fixtures).
async fn logs_bench(cx: &mut AsyncApp, src: Rc<crate::source::Source>, follow: u64) {
    #[cfg(feature = "fixtures")]
    crate::logbench::run(cx, src, follow).await;
    #[cfg(not(feature = "fixtures"))]
    {
        let _ = (cx, src, follow);
        eprintln!("doze-app bench --logs needs a build with the `fixtures` feature");
    }
}
