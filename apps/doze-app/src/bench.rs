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
            let (src, hub, first, idle) = (
                source.clone(),
                source.hub.clone(),
                (source.first.clone(), source.facts.clone()),
                (b.idle, b.closed.unwrap_or(10)),
            );
            let (logs, follow) = (b.logs, b.follow);
            cx.spawn(async move |cx: &mut AsyncApp| {
                let _keep = src.clone();
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if logs {
                    logs_bench(cx, src, follow).await;
                } else {
                    time_open(cx, hub, first, idle).await;
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    ExitCode::SUCCESS
}

/// The window opened as a tray click opens it, timed to its first frame and to its first frame with the stack.
async fn time_open(
    cx: &mut AsyncApp,
    hub: doze_desk::Hub,
    (first, facts): (Option<doze_desk::StackKey>, crate::facts::Mode),
    (idle, closed): (u64, u64),
) {
    let t = Instant::now();
    let opened = cx.update(|cx| {
        cx.set_activation_policy(ActivationPolicy::Regular);
        let bounds = gpui::Bounds::centered(None, gpui::size(gpui::px(1280.), gpui::px(800.)), cx);
        cx.open_window(crate::app::options(bounds, true), |window, cx| {
            window
                .on_next_frame(move |_, _| println!("first_frame_ms {}", t.elapsed().as_millis()));
            cx.new(|cx| Root::new(hub.clone(), first.clone(), facts.clone(), window, cx))
        })
    });
    let Ok(h) = opened else { return };
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
    cx.background_executor()
        .timer(Duration::from_secs(idle))
        .await;
    println!("idle_done");
    let _ = h.update(cx, |_, w, _| w.remove_window());
    hub.set_visible(false);
    cx.background_executor()
        .timer(Duration::from_secs(closed))
        .await;
    println!("closed_done");
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
