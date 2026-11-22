//! What the bench reads off an idle window: the frames it draws, its footprint (as `footprint` reads it, Activity
//! Monitor's Memory figure) while idle and once it has settled, and the `--check` that fails a run over its lines.

use std::time::{Duration, Instant};

use gpui::AsyncApp;

use super::Watch;

/// The footprint once the window has drawn nothing for 15 s (the driver lets its ~150 MB go about ten seconds
/// after the last frame), waiting at most a minute for such a spell: `settled_footprint_mb MB`, and how long after the
/// last frame it was read.
pub(super) async fn settled_footprint(cx: &mut AsyncApp, mut last: Option<Instant>) -> Option<f64> {
    const SETTLE: Duration = Duration::from_secs(15);
    let give_up = Instant::now() + Duration::from_secs(60);
    let mut frames = crate::app::frames_drawn();
    while last.is_some_and(|l| l.elapsed() < SETTLE) && Instant::now() < give_up {
        cx.background_executor()
            .timer(Duration::from_millis(250))
            .await;
        let n = crate::app::frames_drawn();
        if n != frames {
            (frames, last) = (n, Some(Instant::now()));
        }
    }
    let fp = cx.background_executor().spawn(async { footprint() }).await;
    if let Some(mb) = fp {
        let after = last.map_or(0, |l| l.elapsed().as_secs());
        println!("settled_footprint_mb {mb:.0} ({after} s after the last frame)");
    }
    fp
}

/// Idles for `secs`, counting the frames drawn after the first three seconds (the page settling: its first batch, a
/// fade in): `idle_frames N`, and `idle_frame_ms T` for each of the first twenty, from the window's open. An idle
/// window draws none; one glanced at, a frame when a word to the minute changes. `footprint_at_s S MB` at each of
/// `samples` seconds after `opened`.
/// Returns the frames, and when the last was seen.
pub(super) async fn idle_frames(
    cx: &mut AsyncApp,
    secs: u64,
    opened: Instant,
    samples: &[u64],
) -> (u64, Option<Instant>) {
    let t = Instant::now();
    let settle = Duration::from_secs(3).min(Duration::from_secs(secs));
    cx.background_executor().timer(settle).await;
    let start = crate::app::frames_drawn();
    let (mut last, mut shown, mut at, mut seen) = (start, 0, 0, None);
    let mut samples: Vec<u64> = samples.to_vec();
    samples.sort_unstable();
    while t.elapsed() < Duration::from_secs(secs) {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        while samples
            .first()
            .is_some_and(|s| opened.elapsed() >= Duration::from_secs(*s))
        {
            let s = samples.remove(0);
            let fp = cx.background_executor().spawn(async { footprint() }).await;
            if let Some(mb) = fp {
                println!("footprint_at_s {s} {mb:.0}");
            }
        }
        let now = crate::app::frames_drawn();
        if now != last {
            at = t.elapsed().as_millis();
            seen = Some(Instant::now());
            if shown < 20 {
                println!("idle_frame_ms {at}");
                shown += 1;
            }
        }
        last = now;
    }
    println!("idle_frames {}", last - start);
    println!("last_idle_frame_ms {at}");
    (last - start, seen)
}

/// `--check`: a window left alone draws nothing, and stays under its budget. A failure ends the run with exit 1 (the
/// platform's quit would exit 0).
pub(super) fn checked(drawn: u64, (fp, settled): (Option<f64>, Option<f64>), w: &Watch) {
    let mut bad = Vec::new();
    // Hidden, nothing; glanced at (on screen, untouched), a frame when a word to the minute changes: `per_minute` for
    // each minute (or part of one) counted; in use, what moves.
    let counted = w.idle.saturating_sub(3);
    let allowed = if w.hidden {
        0
    } else {
        w.per_minute * counted.div_ceil(60).max(1)
    };
    if drawn > allowed && (w.hidden || !w.in_use) {
        let how = if w.hidden { "hidden" } else { "glanced at" };
        bad.push(format!(
            "{drawn} frames drawn by a window {how} in {counted} s ({allowed} allowed)"
        ));
    }
    if let (Some(limit), Some(mb)) = (w.budget, fp)
        && mb > limit
    {
        bad.push(format!(
            "idle footprint {mb:.0} MB over the {limit:.0} MB budget"
        ));
    }
    // Settled: 15 s after the last frame, the driver's redraw floor (~150 MB, held ~10 s after a frame) is gone.
    if let (Some(limit), Some(mb)) = (w.settled, settled)
        && mb > limit
    {
        bad.push(format!(
            "idle footprint {mb:.0} MB 15 s after the last frame, over the settled line of {limit:.0} MB"
        ));
    }
    if bad.is_empty() {
        println!("check ok");
        return;
    }
    for b in &bad {
        eprintln!("doze-app bench: {b}");
    }
    w.end(1);
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
