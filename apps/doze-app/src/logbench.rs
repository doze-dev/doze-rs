//! `doze-app bench --logs`: the logs viewer under load (PLAN.md §6). It opens All logs on a fixture stack, fills the
//! ring with 100,000 lines through the fixture daemon (the same socket, hub and fold a daemon's lines take), then:
//!
//! - scrolls for five seconds, jumping about a thousand rows a frame across the whole list (so no row is ever drawn
//!   twice running), and times every frame: `scroll_frames`, `scroll_frame_max_ms`, `scroll_frame_p99_ms`,
//!   `scroll_frames_over_16ms`;
//! - follows for `--follow` seconds while the daemon prints 200 lines a second, reading the process's footprint (the
//!   figure Activity Monitor shows) every second: `follow_footprint_max_mb`, and ten seconds after the lines stop,
//!   `after_follow_footprint_mb`.
//!
//! The window is on screen, so a frame waits for the display: on a 120 Hz panel frames come every 8.3 ms, and a frame
//! over 16 ms is one the display had to wait a refresh for.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use doze_control::{Frame, LogFrame, Timestamp};
use gpui::{AppContext as _, AsyncApp, ScrollStrategy, WindowHandle};

use crate::shell::Root;
use crate::source::Source;

/// The lines a stack's ring holds.
const LINES: usize = 100_000;
/// How fast the daemon prints while the page follows.
const RATE: u64 = 200;

fn line(i: usize) -> Frame {
    let inst = ["api", "web", "db", "mongo", "worker"]
        .get(i % 5)
        .copied()
        .unwrap_or("api");
    let text = match i % 50 {
        0 => format!("ERROR upstream timed out after 5s request={i}"),
        1..=4 => format!(
            "WARN slow query {}ms SELECT * FROM orders WHERE id = {i}",
            200 + i % 300
        ),
        5..=9 => format!("DEBUG cache miss key=product:{i}"),
        _ => format!("INFO GET /products/{i} 200 {}ms", i % 90),
    };
    Frame {
        topic: "logs".into(),
        log: Some(LogFrame {
            instance: inst.into(),
            line: text,
            stream: "o".into(),
            run: 1,
            time: Timestamp::from_system_time(SystemTime::now()),
        }),
        ..Frame::default()
    }
}

/// The process's footprint in megabytes, as `footprint` reads it.
fn footprint() -> Option<f64> {
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

async fn pause(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

/// Fills the ring: the daemon prints `LINES` lines in bursts, and the hub takes them in.
async fn fill(cx: &mut AsyncApp, src: &Source) {
    let t = Instant::now();
    for chunk in 0..(LINES / 2000) {
        for i in chunk * 2000..(chunk + 1) * 2000 {
            src.push(&line(i));
        }
        pause(cx, 5).await;
    }
    let (hub, key) = (src.hub.clone(), src.first.clone());
    for _ in 0..400 {
        let held = key
            .as_ref()
            .and_then(|k| hub.with_logs(k, |r| r.len()))
            .unwrap_or(0);
        if held >= LINES {
            break;
        }
        pause(cx, 50).await;
    }
    let held = key
        .as_ref()
        .and_then(|k| hub.with_logs(k, |r| (r.len(), r.heap_bytes())));
    if let Some((n, bytes)) = held {
        println!("ring_lines {n}");
        println!("ring_mb {:.1}", bytes as f64 / 1_048_576.);
    }
    println!("fill_ms {}", t.elapsed().as_millis());
}

/// One frame of the scroll: note when it began, jump the list, and ask for the next frame from inside this one, so
/// every frame the display shows is one step.
fn step(
    l: gpui::Entity<crate::logs::LogsView>,
    at: Rc<RefCell<Vec<Instant>>>,
    done: async_channel::Sender<()>,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    let k = {
        let mut at = at.borrow_mut();
        at.push(Instant::now());
        at.len()
    };
    let first = at.borrow().first().copied().unwrap_or_else(Instant::now);
    if first.elapsed() >= Duration::from_secs(5) {
        let _ = done.try_send(());
        return;
    }
    l.update(cx, |l, cx| {
        l.follow = false;
        let n = l.index.len().max(1);
        l.scroll
            .scroll_to_item_strict(k * 997 % n, ScrollStrategy::Top);
        cx.notify();
    });
    window.on_next_frame(move |w, cx| step(l, at, done, w, cx));
}

/// Five seconds of scrolling, each frame timed.
async fn scroll(cx: &mut AsyncApp, h: WindowHandle<Root>) {
    let frames: Rc<RefCell<Vec<Instant>>> = Rc::default();
    let (tx, rx) = async_channel::bounded::<()>(1);
    let at = frames.clone();
    let _ = h.update(cx, |root, window, cx| {
        if let Some(l) = root.inside_logs(cx) {
            window.on_next_frame(move |w, cx| step(l, at, tx, w, cx));
            window.refresh();
        }
    });
    let _ = rx.recv().await;
    let at = frames.borrow();
    let mut gaps: Vec<f64> = at
        .windows(2)
        .filter_map(|w| Some(w.get(1)?.duration_since(*w.first()?).as_secs_f64() * 1000.))
        .collect();
    gaps.sort_by(f64::total_cmp);
    let p99 = gaps.get(gaps.len() * 99 / 100).copied().unwrap_or(0.);
    println!("scroll_frames {}", gaps.len());
    println!(
        "scroll_frame_max_ms {:.1}",
        gaps.last().copied().unwrap_or(0.)
    );
    println!("scroll_frame_p99_ms {p99:.1}");
    println!(
        "scroll_frame_median_ms {:.1}",
        gaps.get(gaps.len() / 2).copied().unwrap_or(0.)
    );
    println!(
        "scroll_frames_over_16ms {}",
        gaps.iter().filter(|g| **g > 16.0).count()
    );
}

/// Following while the daemon prints `RATE` lines a second, the footprint read every second.
async fn follow(cx: &mut AsyncApp, h: WindowHandle<Root>, src: &Source, secs: u64) {
    let _ = h.update(cx, |root, _, cx| {
        if let Some(l) = root.inside_logs(cx) {
            l.update(cx, |l, cx| l.set_follow(true, cx));
        }
    });
    let start = Instant::now();
    let (mut sent, mut peak, mut last_read) = (0u64, 0f64, Instant::now());
    while start.elapsed() < Duration::from_secs(secs) {
        let due = start.elapsed().as_millis() as u64 * RATE / 1000;
        while sent < due {
            src.push(&line(LINES + sent as usize));
            sent += 1;
        }
        if last_read.elapsed() >= Duration::from_secs(1) {
            last_read = Instant::now();
            let fp = cx.background_executor().spawn(async { footprint() }).await;
            peak = peak.max(fp.unwrap_or(0.));
        }
        pause(cx, 5).await;
    }
    println!("follow_lines {sent}");
    println!("follow_footprint_max_mb {peak:.0}");
    pause(cx, 10_000).await;
    let after = cx.background_executor().spawn(async { footprint() }).await;
    println!("after_follow_footprint_mb {:.0}", after.unwrap_or(0.));
}

/// The whole run, in the opened window.
pub(crate) async fn run(cx: &mut AsyncApp, src: Rc<Source>, follow_secs: u64) {
    let (hub, first) = (src.hub.clone(), src.first.clone());
    let opened = cx.update(|cx| {
        cx.set_activation_policy(gpui::ActivationPolicy::Regular);
        let bounds = gpui::Bounds::centered(None, gpui::size(gpui::px(1280.), gpui::px(800.)), cx);
        cx.open_window(crate::app::options(bounds, true), |window, cx| {
            cx.new(|cx| Root::new(hub.clone(), first.clone(), window, cx))
        })
    });
    let Ok(h) = opened else { return };
    hub.set_visible(true);
    let pumped = hub.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(batch) = pumped.updates().recv().await {
            let _ = h.update(cx, |root, _, cx| root.apply(&batch, cx));
        }
    })
    .detach();
    pause(cx, 5000).await;
    if let Some(fp) = footprint() {
        println!("stack_page_footprint_mb {fp:.0}");
    }
    idle_pace(cx, h).await;
    let _ = h.update(cx, |root, window, cx| root.open_logs(window, cx));
    if let Some(fp) = footprint() {
        println!("logs_open_footprint_mb {fp:.0}");
    }
    fill(cx, &src).await;
    pause(cx, 1000).await;
    if let Some(fp) = footprint() {
        println!("full_ring_footprint_mb {fp:.0}");
    }
    scroll(cx, h).await;
    #[cfg(feature = "capture")]
    draw_cost(cx, h).await;
    follow(cx, h, &src, follow_secs).await;
    let _ = h.update(cx, |_, w, _| w.remove_window());
}

/// One idle frame: note when it began and ask for the next, drawing nothing new (the display's own pace).
fn idle_step(
    at: Rc<RefCell<Vec<Instant>>>,
    done: async_channel::Sender<()>,
    window: &mut gpui::Window,
) {
    at.borrow_mut().push(Instant::now());
    if at.borrow().len() > 120 {
        let _ = done.try_send(());
        return;
    }
    window.refresh();
    window.on_next_frame(move |w, _| idle_step(at, done, w));
}

/// The display's pace with nothing to draw: the baseline the scroll's frames compare with.
async fn idle_pace(cx: &mut AsyncApp, h: WindowHandle<Root>) {
    let frames: Rc<RefCell<Vec<Instant>>> = Rc::default();
    let (tx, rx) = async_channel::bounded::<()>(1);
    let at = frames.clone();
    let _ = h.update(cx, |_, window, _| {
        window.on_next_frame(move |w, _| idle_step(at, tx, w));
        window.refresh();
    });
    let _ = rx.recv().await;
    let at = frames.borrow();
    let mut gaps: Vec<f64> = at
        .windows(2)
        .filter_map(|w| Some(w.get(1)?.duration_since(*w.first()?).as_secs_f64() * 1000.))
        .collect();
    gaps.sort_by(f64::total_cmp);
    println!(
        "idle_frame_median_ms {:.1}",
        gaps.get(gaps.len() / 2).copied().unwrap_or(0.)
    );
}

/// What a frame of scrolling costs, apart from the display's pace: each of 300 jumps drawn in full offscreen (layout,
/// paint, the GPU's render and reading the picture back), timed. An upper bound: a frame on screen does not read back.
#[cfg(feature = "capture")]
async fn draw_cost(cx: &mut AsyncApp, h: WindowHandle<Root>) {
    let mut took: Vec<f64> = Vec::new();
    for k in 1..=300usize {
        let ms = h.update(cx, |root, window, cx| {
            if let Some(l) = root.inside_logs(cx) {
                l.update(cx, |l, cx| {
                    let n = l.index.len().max(1);
                    l.follow = false;
                    l.scroll
                        .scroll_to_item_strict(k * 997 % n, ScrollStrategy::Top);
                    cx.notify();
                });
            }
            let t = Instant::now();
            let _ = window.render_to_image();
            t.elapsed().as_secs_f64() * 1000.
        });
        took.extend(ms);
    }
    took.sort_by(f64::total_cmp);
    println!("draw_frames {}", took.len());
    println!(
        "draw_ms_median {:.1}",
        took.get(took.len() / 2).copied().unwrap_or(0.)
    );
    println!(
        "draw_ms_p99 {:.1}",
        took.get(took.len() * 99 / 100).copied().unwrap_or(0.)
    );
    println!("draw_ms_max {:.1}", took.last().copied().unwrap_or(0.));
    println!(
        "draw_frames_over_16ms {}",
        took.iter().filter(|t| **t > 16.0).count()
    );
}
