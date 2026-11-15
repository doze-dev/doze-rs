//! The arithmetic behind the meters and the charts: who holds how much of a total, what a CPU figure is in cores,
//! how a series is squeezed into a width without losing a spike, and what a scale runs from and to. A surface draws
//! bars and blocks from these; the figures are the same on all of them.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::time::Duration;

use crate::format::{ago, mb};
use crate::gonum::{fixed, max, min};

/// How often the daemon samples the meters.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// One instance's part of a total.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Share<'a> {
    /// Whose.
    pub name: &'a str,
    /// How much.
    pub v: f64,
}

/// What a meter draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Meter<'a> {
    /// The shares above the floor, largest first.
    pub shares: Vec<Share<'a>>,
    /// How many were at or under the floor: counted, not drawn, as an instance doing next to nothing has no run to
    /// show.
    pub idle: usize,
    /// The sum of the shares drawn.
    pub sum: f64,
}

/// Sorts `all` into what a meter draws. (Go: `Shares`.)
#[must_use]
pub fn shares(mut all: Vec<Share<'_>>, floor: f64) -> Meter<'_> {
    let before = all.len();
    all.retain(|x| x.v > floor);
    all.sort_by(|a, b| b.v.partial_cmp(&a.v).unwrap_or(Ordering::Equal));
    let sum = all.iter().map(|x| x.v).fold(0.0, |s, v| s + v);
    Meter {
        idle: before - all.len(),
        sum,
        shares: all,
    }
}

/// Divides `inner` cells among the shares: each at least one, the rest in proportion, and the difference taken
/// from or given to the first shares so the bar is exactly full. Nothing when the sum is not positive.
/// (Go: `ShareCells`.)
#[must_use]
pub fn share_cells(shares: &[Share<'_>], sum: f64, inner: usize) -> Vec<usize> {
    if sum <= 0.0 || shares.is_empty() {
        return Vec::new();
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    // WHY: Go's int(x/sum*inner + 0.5), saturating as a float conversion does
    let mut cells: Vec<i64> = shares
        .iter()
        .map(|x| ((x.v / sum * inner as f64 + 0.5) as i64).max(1))
        .collect();
    let mut left = i64::try_from(inner).unwrap_or(i64::MAX)
        - cells.iter().fold(0_i64, |s, &c| s.saturating_add(c));
    for cell in &mut cells {
        if left == 0 {
            break;
        }
        let d = if *cell + left < 1 { 1 - *cell } else { left };
        *cell += d;
        left -= d;
    }
    cells
        .into_iter()
        .map(|c| usize::try_from(c).unwrap_or(1))
        .collect()
}

/// Formats a share of memory: plain megabytes while the total is under a gigabyte, so a key reads as a column of
/// numbers; sizes from there. (Go: `MemUnit`.)
#[must_use]
pub fn mem_unit(total: f64) -> fn(f64) -> String {
    #[allow(clippy::cast_possible_truncation)] // WHY: Go's int64(v), saturating
    fn sized(v: f64) -> String {
        mb(v as i64)
    }
    #[allow(clippy::cast_possible_truncation)]
    fn megabytes(v: f64) -> String {
        ((v as i64).wrapping_add(1 << 19) >> 20).to_string()
    }
    if total >= f64::from(1_u32 << 30) {
        sized
    } else {
        megabytes
    }
}

/// A CPU figure (ps's percent, one core being 100) as cores, which is what it is: 0.23 cores is a fifth of one.
/// (Go: `Cores`.)
#[must_use]
pub fn cores(pct: f64) -> String {
    let c = pct / 100.0;
    if c <= 0.0 {
        "0 cores".to_owned()
    } else if c < 0.005 {
        "<0.01 cores".to_owned()
    } else if c < 0.995 {
        format!("{} cores", fixed(c, 2))
    } else if c < 1.05 {
        "1 core".to_owned()
    } else {
        format!("{} cores", fixed(c, 1))
    }
}

/// [`cores`] without the word, for a key beside a bar. (Go: `CoresUnit`.)
#[must_use]
pub fn cores_unit(pct: f64) -> String {
    let said = cores(pct);
    let bare = said.strip_suffix(" cores").unwrap_or(&said);
    bare.strip_suffix(" core").unwrap_or(bare).to_owned()
}

/// The highest (never under zero) and the mean of what a chart holds. (Go: `Stats`.)
#[must_use]
pub fn stats(vs: &[f64]) -> (f64, f64) {
    let peak = vs.iter().fold(0.0, |p, &v| max(p, v));
    let total = vs.iter().fold(0.0, |s, &v| s + v);
    #[allow(clippy::cast_precision_loss)] // WHY: a count of readings
    let mean = if vs.is_empty() {
        total
    } else {
        total / vs.len() as f64
    };
    (peak, mean)
}

/// The range a chart stands for: zero to a little above the highest reading (no lower than `floor`), or, zoomed, a
/// little either side of the readings, so that a steady series reads as steady and a drift as a drift.
/// (Go: `Scale`.)
#[must_use]
pub fn scale(vs: &[f64], floor: f64, zoom: bool) -> (f64, f64) {
    let Some(&first) = vs.first() else {
        return (0.0, max(floor, 1.0));
    };
    let (lo, hi) = vs
        .iter()
        .fold((first, first), |(lo, hi), &v| (min(lo, v), max(hi, v)));
    if zoom {
        let pad = max((hi - lo) * 0.25, hi * 0.03);
        let pad = if pad == 0.0 { 1.0 } else { pad };
        return (max(lo - pad, 0.0), hi + pad);
    }
    (0.0, max(max(floor, hi * 1.15), 1.0))
}

/// Fits `vs` into `w` columns by merging neighbours by their highest, from the newest back, so the whole history
/// is drawn and a spike is never lost to the width. Shorter than `w`, it is handed back as it is. (Go: `Squeeze`.)
#[must_use]
pub fn squeeze(vs: &[f64], w: usize) -> Cow<'_, [f64]> {
    match columns(vs.len(), w) {
        None => Cow::Borrowed(vs),
        Some(cols) => Cow::Owned(cols.map(|(start, end)| peak(vs, start, end)).collect()),
    }
}

/// [`squeeze`] for readings that carry the time they were taken: a merged column keeps the newest of its times.
/// Times that do not pair with the readings leave both as they are. (Go: `SqueezeTimed`.)
#[must_use]
pub fn squeeze_timed<'a, T: Copy>(
    vs: &'a [f64],
    at: &'a [T],
    w: usize,
) -> (Cow<'a, [f64]>, Cow<'a, [T]>) {
    let Some(cols) = columns(vs.len(), w).filter(|_| at.len() == vs.len()) else {
        return (Cow::Borrowed(vs), Cow::Borrowed(at));
    };
    let (peaks, times) = cols
        .filter_map(|(start, end)| Some((peak(vs, start, end), *at.get(end.checked_sub(1)?)?)))
        .unzip();
    (Cow::Owned(peaks), Cow::Owned(times))
}

/// The `[start, end)` of each merged column, oldest first; `None` when nothing needs merging.
fn columns(n: usize, w: usize) -> Option<impl Iterator<Item = (usize, usize)>> {
    if n <= w || w < 1 {
        return None;
    }
    let per = n.div_ceil(w);
    let cols = n.div_ceil(per);
    Some((0..cols).map(move |i| {
        let end = n - per * (cols - 1 - i);
        (end.saturating_sub(per), end)
    }))
}

fn peak(vs: &[f64], start: usize, end: usize) -> f64 {
    let run = vs.get(start..end).unwrap_or_default();
    run.iter()
        .fold(run.first().copied().unwrap_or(0.0), |m, &v| max(m, v))
}

/// How long `n` readings cover, in the words of a chart's axis: `45s`, `2m 05s`, `3h`. (Go: `SpanWord`.)
#[must_use]
pub fn span_word(n: u64) -> String {
    let secs = n.max(1);
    let d = Duration::from_secs(secs);
    if (60..600).contains(&secs) && !secs.is_multiple_of(60) {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    ago(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Go: TestCoresSaysWhatTheNumberIs.
    #[test]
    fn cores_says_what_the_number_is() {
        for (pct, want) in [
            (0.0, "0 cores"),
            (0.1, "<0.01 cores"),
            (22.6, "0.23 cores"),
            (100.0, "1 core"),
            (250.0, "2.5 cores"),
            (1020.0, "10.2 cores"),
        ] {
            assert_eq!(cores(pct), want, "cores({pct})");
        }
    }

    // Go: TestSqueezeKeepsTheSpikeAndTheNewest. The whole of the history is drawn, whatever the width: readings
    // are merged by their highest, from the newest back, so a spike survives and the right edge is now.
    #[test]
    fn squeeze_keeps_the_spike_and_the_newest() {
        let mut vs = vec![0.0; 240];
        vs[100] = 9.0;
        vs[239] = 5.0;
        let out = squeeze(&vs, 60);
        assert!(out.len() <= 60, "{} columns, want at most 60", out.len());
        assert_eq!(
            out.iter().copied().fold(0.0, f64::max),
            9.0,
            "the spike was lost"
        );
        assert_eq!(
            out.last(),
            Some(&5.0),
            "the newest reading is not at the right edge"
        );
        assert!(matches!(squeeze(&[1.0, 2.0, 3.0], 60), Cow::Borrowed(s) if s.len() == 3));
    }

    // Go: TestShareCellsFillTheBar. The bar is the whole of what is consumed: every share gets at least a cell
    // and the cells add up to the width exactly.
    #[test]
    fn share_cells_fill_the_bar() {
        let all = vec![
            Share {
                name: "api",
                v: 84.0,
            },
            Share {
                name: "web",
                v: 212.0,
            },
            Share {
                name: "quiet",
                v: 0.0,
            },
        ];
        let m = shares(all, 0.0);
        assert_eq!(
            (m.idle, m.shares.len(), m.shares[0].name, m.sum),
            (1, 2, "web", 296.0)
        );
        let cells = share_cells(&m.shares, m.sum, 40);
        assert_eq!(cells.iter().sum::<usize>(), 40);
        assert!(cells[1] >= 1);
    }
}
