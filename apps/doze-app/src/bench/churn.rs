//! `doze-app bench --churn`: the window put through what a person does to it, its memory read after each step. A
//! window kept open for hours is maximised and restored, resized, minimised, covered and hidden; each of those stops
//! and restarts its display link and may resize its drawables. A layer holds at most three drawables (each a whole
//! window of pixels, 22 MB maximised on a 3024 × 1898 display), so the count must not climb with the steps.
//!
//! Each step: the action, two seconds, then (a window on screen) 120 frames drawn back to back, timed against the
//! display's pace (`over_16ms`; `late`, frames over 1.5 × the median gap: ones that missed the pace), then the window
//! left alone until it has drawn nothing for 15 s (the driver's redraw floor gone), then
//! `churn STEP footprint_mb MB drawables N held_mb MB`. Ten quick rounds of all of it end the run (`after_10_rounds`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{AsyncApp, WindowHandle};

use super::footprint;
use super::measure::drawables;
use crate::shell::Root;

/// What a step does to the window.
#[derive(Clone, Copy, Debug)]
enum Act {
    /// Nothing: the window as it opened, after drawing.
    Fresh,
    /// Zoomed (the green button): to the screen's size, or back.
    Zoom,
    /// Dragged through a dozen sizes, then back to 1280 × 800.
    Drag,
    /// Into the Dock.
    Minimise,
    /// The app hidden (⌘H): the window covered.
    Hide,
    /// Out of the Dock, the app unhidden, the window in front, all without activating the app.
    Back,
}

pub(super) async fn run(cx: &mut AsyncApp, h: WindowHandle<Root>) {
    let steps = [
        ("fresh", Act::Fresh),
        ("maximised", Act::Zoom),
        ("restored", Act::Zoom),
        ("dragged", Act::Drag),
        ("minimised", Act::Minimise),
        ("unminimised", Act::Back),
        ("hidden", Act::Hide),
        ("shown", Act::Back),
    ];
    for (name, act) in steps {
        step(cx, h, name, act).await;
    }
    // The same again, quickly, ten times over: what a day of use does. The count read once at the end.
    for _ in 0..10 {
        for act in [
            Act::Zoom,
            Act::Zoom,
            Act::Minimise,
            Act::Back,
            Act::Hide,
            Act::Back,
        ] {
            apply(cx, h, act).await;
            pause(cx, 1500).await;
        }
    }
    step(cx, h, "after_10_rounds", Act::Fresh).await;
    println!("churn_done");
}

async fn pause(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

async fn apply(cx: &mut AsyncApp, h: WindowHandle<Root>, act: Act) {
    match act {
        Act::Fresh => {}
        Act::Zoom => {
            let _ = h.update(cx, |_, w, _| w.zoom_window());
        }
        Act::Minimise => {
            let _ = h.update(cx, |_, w, _| w.minimize_window());
        }
        Act::Hide => cx.update(|cx| cx.hide()),
        Act::Back => {
            doze_os::ui::bring_back();
        }
        Act::Drag => {
            for i in 0..12u16 {
                let (dw, dh) = (f32::from(i % 4) * 90., f32::from(i % 3) * 70.);
                let _ = h.update(cx, |_, w, _| {
                    w.resize(gpui::size(gpui::px(1000. + dw), gpui::px(640. + dh)));
                });
                pause(cx, 120).await;
            }
            let _ = h.update(cx, |_, w, _| {
                w.resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
            });
        }
    }
}

async fn step(cx: &mut AsyncApp, h: WindowHandle<Root>, name: &str, act: Act) {
    apply(cx, h, act).await;
    pause(cx, 2000).await;
    if matches!(act, Act::Back) {
        let (active, key) = doze_os::ui::holds_input();
        if active || key > 0 {
            eprintln!("doze-app bench --churn: bringing the window back took the focus; stopping");
            std::process::exit(3);
        }
    }
    let on_screen = !matches!(act, Act::Minimise | Act::Hide);
    let pace = if on_screen {
        Some(pace(cx, h).await)
    } else {
        None
    };
    settle(cx).await;
    let fp = cx.background_executor().spawn(async { footprint() }).await;
    let n = cx.background_executor().spawn(async { drawables() }).await;
    let mut line = format!(
        "churn {name} footprint_mb {:.0} drawables {}",
        fp.unwrap_or(0.),
        n.map_or_else(|| "?".to_owned(), |n| n.to_string())
    );
    if let Some(p) = pace {
        line.push_str(&format!(
            " frames {} frame_median_ms {:.1} over_16ms {} late {}",
            p.frames, p.median, p.over_16ms, p.late
        ));
    }
    println!("{line}");
}

/// Waits until the window has drawn nothing for 15 s (at most 45 s).
async fn settle(cx: &mut AsyncApp) {
    const QUIET: Duration = Duration::from_secs(15);
    let give_up = Instant::now() + Duration::from_secs(45);
    let (mut frames, mut last) = (crate::app::frames_drawn(), Instant::now());
    while last.elapsed() < QUIET && Instant::now() < give_up {
        pause(cx, 250).await;
        let n = crate::app::frames_drawn();
        if n != frames {
            (frames, last) = (n, Instant::now());
        }
    }
}

/// How frames drawn back to back came.
struct Pace {
    /// Frames drawn within 4 s (of 120 asked for).
    frames: usize,
    /// The median gap between them, ms: the display's pace for a window redrawing (on this Mac's 120 Hz panel, 33 ms
    /// for a frame that asks for the next once it is drawn).
    median: f64,
    /// Gaps over 16 ms.
    over_16ms: usize,
    /// Gaps over 1.5 × the median: frames that missed the pace (a drawable not ready in time).
    late: usize,
}

/// 120 frames asked for back to back (each a full redraw of the page), timed.
async fn pace(cx: &mut AsyncApp, h: WindowHandle<Root>) -> Pace {
    let at: Rc<RefCell<Vec<Instant>>> = Rc::default();
    let (tx, rx) = async_channel::bounded::<()>(1);
    let mine = at.clone();
    let _ = h.update(cx, |_, window, _| {
        window.on_next_frame(move |w, _| next(mine, tx, w));
        window.refresh();
    });
    let give_up = Instant::now() + Duration::from_secs(4);
    while rx.try_recv().is_err() && Instant::now() < give_up {
        pause(cx, 20).await;
    }
    let at = at.borrow();
    let mut gaps: Vec<f64> = at
        .windows(2)
        .filter_map(|w| Some(w.get(1)?.duration_since(*w.first()?).as_secs_f64() * 1000.))
        .collect();
    gaps.sort_by(f64::total_cmp);
    let median = gaps.get(gaps.len() / 2).copied().unwrap_or(0.);
    Pace {
        frames: at.len(),
        median,
        over_16ms: gaps.iter().filter(|g| **g > 16.).count(),
        late: gaps.iter().filter(|g| **g > median * 1.5).count(),
    }
}

fn next(at: Rc<RefCell<Vec<Instant>>>, done: async_channel::Sender<()>, window: &mut gpui::Window) {
    at.borrow_mut().push(Instant::now());
    if at.borrow().len() >= 120 {
        let _ = done.try_send(());
        return;
    }
    window.refresh();
    window.on_next_frame(move |w, _| next(at, done, w));
}
