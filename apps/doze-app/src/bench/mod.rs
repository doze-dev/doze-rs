//! `doze-app bench`: the app's own timings, printed as `name value` lines for a script to read (docs/port/app/PLAN.md
//! §6). It starts as a menu-bar app, opens the window a second later as a tray click would, and reports how long the
//! first frame and the first frame with the stack on it took; then it stays idle for `--idle` seconds (for a memory
//! reading from outside: `footprint`, Activity Monitor's figure), closes the window, waits ten seconds and quits.
//! It never touches the doze home: its stack is a fixture.

use std::cell::Cell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{ActivationPolicy, App, AppContext as _, AsyncApp, QuitMode};

mod churn;
mod measure;
pub(crate) mod real;
pub(crate) mod stage;

use measure::drawables;
pub(crate) use measure::footprint;
use measure::{checked, idle_frames, settled_footprint};

use crate::cli::Bench;
use crate::shell::Root;
use crate::theme::Choice;

thread_local! {
    /// The bench's window while it is open: the batches go to it.
    static WINDOW: Cell<Option<gpui::WindowHandle<Root>>> = const { Cell::new(None) };
}

/// The stacks the bench shows: a fixture's, `$DOZE_HOME`'s, or (`--real`) a stack of its own under a daemon of its
/// own, on a throwaway home (bench/real.rs). No fixture and no `--real`: `$DOZE_HOME` when it is set (a fixture
/// served from another process), else an empty folder; never the real home by default.
fn source(b: &Bench) -> Result<(Rc<crate::source::Source>, Option<Rc<real::Real>>), ExitCode> {
    let real = match b.real.map(real::Real::up).transpose() {
        Ok(r) => r.map(Rc::new),
        Err(e) => {
            eprintln!("doze-app bench --real: {e:#}");
            return Err(ExitCode::FAILURE);
        }
    };
    let home = match &real {
        Some(r) => r.home.clone(),
        None => std::env::var_os("DOZE_HOME").map_or_else(std::env::temp_dir, PathBuf::from),
    };
    match crate::source_for(&b.from, home) {
        Ok(s) => Ok((Rc::new(s), real)),
        Err(e) => {
            eprintln!("doze-app bench: {e:#}");
            if let Some(r) = &real {
                r.down();
            }
            Err(ExitCode::FAILURE)
        }
    }
}

/// As a launch takes the hub's batches (`app::take`): the menu bar, and the window while it is open. (Until
/// 2026-11-21 the bench's window heard none, so a live stack never moved it and the bench passed a window that, on a
/// real daemon, drew every second or two.)
fn pump(hub: doze_desk::Hub, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(batch) = hub.updates().recv().await {
            cx.update(|cx| crate::app::take(&hub, &batch, WINDOW.with(Cell::get), cx));
        }
    })
    .detach();
}

pub(crate) fn run(b: Bench, launched: Instant) -> ExitCode {
    let (source, real) = match source(&b) {
        Ok(s) => s,
        Err(code) => return code,
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
            pump(source.hub.clone(), cx);
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
            let watch = Watch::of(&b, real.clone());
            cx.spawn(async move |cx: &mut AsyncApp| {
                let _keep = src.clone();
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if logs {
                    logs_bench(cx, src, follow, (watch.display, watch.on_top)).await;
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
    /// Frames a minute allowed to a window glanced at (`--per-minute`).
    per_minute: u64,
    /// Hides the app once the window is open (`--hidden`), so the window is covered.
    hidden: bool,
    /// Footprint readings, seconds after the window opens (`--at`).
    at: Vec<u64>,
    /// Counts the window as in use when it opens (`--in-use`), as a click on it would: its clocks run for
    /// `shell::attention::FOR`, then it is left alone.
    in_use: bool,
    /// The page the window is left on, and the service it shows.
    screen: stage::Screen,
    select: Option<String>,
    tab: Option<String>,
    /// `--check`: the footprint at the end of the idle time, and at the end of `closed`, at most these.
    settled: Option<f64>,
    closed_budget: Option<f64>,
    /// The window floats over the others (`--on-top`).
    on_top: bool,
    /// The window maximised, resized, minimised and hidden, each brought back (`--churn`, bench/churn.rs).
    churn: bool,
    /// The real stack, taken down before the run ends.
    real: Option<Rc<real::Real>>,
}

impl Watch {
    fn of(b: &Bench, real: Option<Rc<real::Real>>) -> Watch {
        Watch {
            idle: b.idle,
            closed: b.closed.unwrap_or(10),
            map: b.map,
            display: b.display,
            in_use: b.in_use,
            check: b.check,
            budget: b.budget,
            per_minute: b.per_minute,
            hidden: b.hidden,
            at: b.at.clone(),
            screen: b.screen,
            select: b.select.clone(),
            tab: b.tab.clone(),
            settled: b.settled,
            closed_budget: b.closed_budget,
            real,
            on_top: b.on_top,
            churn: b.churn,
        }
    }

    /// Ends the run: the real stack down, then `code` (the platform's quit would exit 0).
    fn end(&self, code: i32) -> ! {
        if let Some(r) = &self.real {
            r.down();
        }
        std::process::exit(code)
    }
}

/// The window opened as a tray click opens it, timed to its first frame and to its first frame with the stack; then
/// left alone on its page and watched, then closed and watched again.
async fn time_open(
    cx: &mut AsyncApp,
    hub: doze_desk::Hub,
    first: (Option<doze_desk::StackKey>, crate::facts::Mode),
    w: Watch,
) {
    let t = Instant::now();
    let Some(h) = open(cx, &hub, first, &w, t).await else {
        return;
    };
    idle(cx, h, &w, t).await;
    WINDOW.with(|w| w.set(None));
    let _ = h.update(cx, |_, w, _| w.remove_window());
    hub.set_visible(false);
    cx.update(crate::app::window_gone);
    closed(cx, &w).await;
    if let Some(r) = &w.real {
        r.down();
    }
}

/// Opens the window, waits for what its page shows, and leaves it on `--screen`.
async fn open(
    cx: &mut AsyncApp,
    hub: &doze_desk::Hub,
    (first, facts): (Option<doze_desk::StackKey>, crate::facts::Mode),
    w: &Watch,
    t: Instant,
) -> Option<gpui::WindowHandle<Root>> {
    let opened = cx.update(|cx| {
        let on = w.display.and_then(|i| cx.displays().get(i).map(|d| d.id()));
        let bounds = gpui::Bounds::centered(on, gpui::size(gpui::px(1280.), gpui::px(800.)), cx);
        cx.open_window(crate::app::options(bounds, true), |window, cx| {
            window
                .on_next_frame(move |_, _| println!("first_frame_ms {}", t.elapsed().as_millis()));
            cx.new(|cx| Root::new(hub.clone(), first.clone(), facts.clone(), window, cx))
        })
    });
    let Ok(h) = opened else { return None };
    WINDOW.with(|w| w.set(Some(h)));
    cx.update(|cx| crate::handsoff::after_open(h.into(), cx));
    if w.on_top {
        println!("on_top {}", doze_os::ui::float_over());
    }
    hub.set_visible(true);
    // A page inside a stack waits for a running stack (a live hub reads its daemons after it starts).
    let wants_stack = !matches!(w.screen, stage::Screen::Opened | stage::Screen::Home);
    loop {
        let has = h.update(cx, |root, _, cx| {
            root.refresh(cx);
            // The page it opens on has what it shows: a stack's page its stack, the Library (no stack named) at once.
            (root.key().is_some() || root.screen == crate::shell::Screen::Library)
                && (!wants_stack
                    || hub
                        .stacks()
                        .iter()
                        .any(|s| s.running && !s.instances.is_empty()))
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
    let _ = h.update(cx, |root, window, cx| {
        stage::go(root, hub, w.screen, w.select.as_deref(), window, cx);
        if let (Some(tab), Some(v)) = (&w.tab, root.inside_instance()) {
            v.update(cx, |v, cx| v.set_tab(tab, window, cx));
        }
        println!("screen {:?}", root.screen);
    });
    Some(h)
}

/// The window left alone (in use first with `--in-use`, hidden with `--hidden`): its frames and footprint, checked.
async fn idle(cx: &mut AsyncApp, h: gpui::WindowHandle<Root>, w: &Watch, t: Instant) {
    if w.in_use {
        // As a click on it: the window counts as key and touched, so its clocks run.
        let _ = h.update(cx, |root, _, cx| {
            crate::shell::attention::key(true, cx.background_executor().now());
            root.wind(cx);
        });
    }
    if w.hidden {
        // As ⌘H: every window of the app off screen, the platform's occlusion state says so (nothing of the person's
        // is touched: the app is not active, and hiding it activates nothing).
        cx.update(|cx| cx.hide());
    }
    if w.map {
        map_bench(cx, h).await;
    }
    let (drawn, last) = idle_frames(cx, if w.map { 0 } else { w.idle }, t, &w.at).await;
    let fp = cx.background_executor().spawn(async { footprint() }).await;
    if let Some(mb) = fp {
        println!("idle_footprint_mb {mb:.0}");
    }
    let settled = match w.settled {
        Some(_) => settled_footprint(cx, last).await,
        None => None,
    };
    let _ = h.update(cx, |_, w, cx| {
        let mode = crate::shell::attention::mode(cx.background_executor().now());
        println!("window_mode {mode:?} visible={}", w.is_visible());
    });
    println!("idle_done");
    if w.check {
        checked(drawn, (fp, settled), w);
    }
    if w.churn {
        churn::run(cx, h).await;
    }
}

/// The window closed, as the menu-bar app goes on: its footprint after `--closed`, checked.
async fn closed(cx: &mut AsyncApp, w: &Watch) {
    cx.background_executor()
        .timer(Duration::from_secs(if w.map { 0 } else { w.closed }))
        .await;
    let closed = cx.background_executor().spawn(async { footprint() }).await;
    if let Some(mb) = closed {
        println!("closed_footprint_mb {mb:.0}");
    }
    if w.churn {
        let n = cx.background_executor().spawn(async { drawables() }).await;
        if let Some(n) = n {
            println!("closed_drawables {n}");
        }
    }
    println!("closed_done");
    if w.check
        && let (Some(limit), Some(mb)) = (w.closed_budget, closed)
        && mb > limit
    {
        eprintln!("doze-app bench: closed footprint {mb:.0} MB over the {limit:.0} MB budget");
        w.end(1);
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

/// The logs viewer under load (`--logs`; a build with fixtures), on the display `--display` names, floating over the
/// others with `--on-top`: its scroll counts frames the display delivers, and a display that hides the window, or a
/// window covered, delivers none.
async fn logs_bench(
    cx: &mut AsyncApp,
    src: Rc<crate::source::Source>,
    follow: u64,
    on: (Option<usize>, bool),
) {
    #[cfg(feature = "fixtures")]
    crate::logbench::run(cx, src, follow, on).await;
    #[cfg(not(feature = "fixtures"))]
    {
        let _ = (cx, src, follow, on);
        eprintln!("doze-app bench --logs needs a build with the `fixtures` feature");
    }
}
