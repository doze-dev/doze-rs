//! `doze-app bench --map` (a `capture` build): the Map under load, as the logs bench measures the logs. It opens the
//! stack's Map, then draws frames offscreen in full (layout, paint, the GPU's render and reading the picture back:
//! an upper bound, a frame on screen does not read back), each after one step of what a person does:
//!
//! - `pan`: 120 steps of 37 px across and down, at the fit;
//! - `zoom`: 120 steps in and out about the middle, from the fit to 1:1 and back (every level of detail);
//! - `hover`: the pointer's card changed every frame, 120 cards, at the fit and at 1:1;
//! - `lines`: the pan again with every line drawn (`L`).
//!
//! It prints `PHASE_ms_median`, `_p99`, `_max` and `_over_16ms`, and the layout's own time (`layout_ms`, the best of
//! five, release).

use std::time::{Duration, Instant};

use gpui::{AsyncApp, WindowHandle};

use super::MapView;
use crate::overview::View;
use crate::shell::Root;

fn report(name: &str, mut took: Vec<f64>) {
    took.sort_by(f64::total_cmp);
    let at = |q: usize| took.get(took.len() * q / 100).copied().unwrap_or(0.);
    println!("{name}_frames {}", took.len());
    println!("{name}_ms_median {:.1}", at(50));
    println!("{name}_ms_p99 {:.1}", at(99));
    println!("{name}_ms_max {:.1}", took.last().copied().unwrap_or(0.));
    println!(
        "{name}_over_16ms {}",
        took.iter().filter(|t| **t > 16.0).count()
    );
}

/// Draws `n` frames, each after `step(k)` on the Map, and times each.
fn frames(
    cx: &mut AsyncApp,
    h: WindowHandle<Root>,
    n: usize,
    step: &dyn Fn(&mut MapView, usize),
) -> Vec<f64> {
    let mut took = Vec::with_capacity(n);
    for k in 0..n {
        let ms = h.update(cx, |root, window, cx| {
            root.map.update(cx, |m, cx| {
                step(m, k);
                cx.notify();
            });
            let t = Instant::now();
            let _ = window.render_to_image();
            t.elapsed().as_secs_f64() * 1000.
        });
        took.extend(ms);
    }
    took
}

/// The map's size, and its layout's time: the best of five.
fn layout_time(m: &MapView) {
    let ins = m.ins.clone();
    println!(
        "cards {} calls {}",
        m.layout.cards.len(),
        m.layout.edges.len()
    );
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let _ = doze_map::layout(&ins, &doze_map::Options::default());
        best = best.min(t.elapsed().as_secs_f64() * 1000.);
    }
    println!("layout_ms {best:.2}");
}

/// The whole run, in a window `root` already shows the stack in.
pub(crate) async fn run(cx: &mut AsyncApp, h: WindowHandle<Root>) {
    let _ = h.update(cx, |root, _, cx| {
        root.overview.update(cx, |o, cx| o.set_view(View::Map, cx));
    });
    for _ in 0..100 {
        let ready = h.update(cx, |root, window, cx| {
            let _ = window.render_to_image();
            let m = root.map.read(cx);
            m.cam.fitted && !m.layout.cards.is_empty()
        });
        if ready.unwrap_or(false) {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    let _ = h.update(cx, |root, _, cx| layout_time(root.map.read(cx)));
    let pan = move |m: &mut MapView, k: usize| {
        if k == 0 {
            m.refit();
            m.cam.zoom_centre(1.6, m.layout.bounds);
        }
        let d = if (k / 30).is_multiple_of(2) {
            37.0
        } else {
            -37.0
        };
        m.cam.pan_by(d, d * 0.6, m.layout.bounds);
    };
    report("pan", frames(cx, h, 120, &pan));
    let zoom = move |m: &mut MapView, k: usize| {
        if k == 0 {
            m.refit();
        }
        let f = if (k / 30).is_multiple_of(2) {
            1.06
        } else {
            1.0 / 1.06
        };
        m.cam.zoom_centre(f, m.layout.bounds);
    };
    report("zoom", frames(cx, h, 120, &zoom));
    let hover = move |m: &mut MapView, k: usize| {
        if k == 0 {
            m.refit();
        }
        if k == 60 {
            m.cam.zoom_centre(1.0 / m.cam.zoom, m.layout.bounds);
        }
        let n = m.layout.cards.len().max(1);
        m.hover = Some(doze_map::CardId(u32::try_from(k * 7 % n).unwrap_or(0)));
    };
    report("hover", frames(cx, h, 120, &hover));
    let lines = move |m: &mut MapView, k: usize| {
        if k == 0 {
            m.hover = None;
            m.all_lines = true;
        }
        pan(m, k);
    };
    report("lines", frames(cx, h, 120, &lines));
}
