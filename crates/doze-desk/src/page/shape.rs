//! How a resolved table and a panel's counters are shaped for the page: each column sized for what it holds, a
//! table's rows ordered and filtered as its view says, and the counters as figures (an `of` column folded into the
//! figure before it). The resolver ([`super::resolve`]) calls these once it has the rows.

use super::Ink;
use super::console::{Col, Node, Row, Stat, TableView};
use doze_control as wire;
use doze_display as words;

/// The widest a column that does not grow is sized for, in characters: past it a cell is cut short.
const WIDEST: usize = 40;
/// A column of words this wide or wider takes the room the others leave.
const LONG: usize = 20;

/// Sizes each column for what it holds, its title's width and its widest cell's (up to [`WIDEST`]), and lets the
/// widest column of long words (`LONG`) take what is left: a table of short columns does not spread them across the
/// page with a gulf before the last, and a long key is not cut short while room is wasted.
pub(super) fn size_columns(cols: &mut [Col], rows: &[Row]) {
    for (i, c) in cols.iter_mut().enumerate() {
        let widest = rows
            .iter()
            .filter_map(|r| r.cells.get(i))
            .map(|c| c.text.chars().count())
            .max()
            .unwrap_or(0);
        // The title is drawn in capitals with a sort mark beside it: a little wider than its letters.
        c.chars = widest.max(c.title.chars().count() + 2).max(3);
        c.grow = false;
    }
    // Only a column of long words grows; a table of short columns keeps them together, the room left after the last.
    let grow = cols
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.right && c.chars >= LONG)
        .max_by_key(|(i, c)| (c.chars, std::cmp::Reverse(*i)))
        .map(|(i, _)| i);
    if let Some(c) = grow.and_then(|g| cols.get_mut(g)) {
        c.grow = true;
    }
    for c in cols.iter_mut().filter(|c| !c.grow) {
        c.chars = c.chars.min(WIDEST);
    }
}

/// The rows a table shows for `view`: those holding its words, in its column's order (figures by what they amount
/// to, words in natural order, a blank last), the module's order among equals.
pub(super) fn shown(
    out: &mut Node,
    cols: &[(&wire::ConsoleColumn, &words::Column)],
    raw: &[words::ConsoleRow],
    view: &TableView,
) {
    let words = view.filter.trim().to_lowercase();
    if !words.is_empty() {
        out.rows.retain(|r| {
            r.cells
                .iter()
                .any(|c| c.text.to_lowercase().contains(&words))
        });
    }
    out.filter.clone_from(&view.filter);
    let Some((i, down, (_, column))) = view.sort.and_then(|(i, d)| Some((i, d, cols.get(i)?)))
    else {
        return;
    };
    if let Some(c) = out.cols.get_mut(i) {
        c.sort = Some(down);
    }
    let key = &column.key;
    // What a cell amounts to: the module's own figure where it sent one (a size in bytes), else what is written.
    let amount = |r: &Row| -> (Option<f64>, String) {
        let rawv = raw.get(r.at).map(|x| x.cell(key)).unwrap_or_default();
        let text = r.cells.get(i).map(|c| c.text.as_str()).unwrap_or_default();
        let n = words::magnitude(rawv).or_else(|| words::magnitude(text));
        (n, text.to_lowercase())
    };
    out.rows.sort_by(|a, b| {
        let (x, y) = (amount(a), amount(b));
        let blank = |v: &(Option<f64>, String)| v.0.is_none() && (v.1.is_empty() || v.1 == "—");
        let order = match (blank(&x), blank(&y)) {
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            _ => match (x.0, y.0) {
                (Some(p), Some(q)) => p.total_cmp(&q),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => words::natural_cmp(&x.1, &y.1),
            },
        };
        if down { order.reverse() } else { order }
    });
}

/// A panel's counters as figures: an `of` column is the limit of the figure before it (`4 of 100`, with how full);
/// a figure not read yet says why rather than leave a bare dash.
pub(super) fn stats(cols: &[words::Column], row: &words::ConsoleRow) -> Vec<Stat> {
    let mut out: Vec<Stat> = Vec::new();
    let mut before: Option<&words::Column> = None;
    for c in cols {
        let (t, tone) = words::cell(c, row);
        if c.role == words::Role::Of
            && let (Some(prev), Some(last)) = (before, out.last_mut())
            && !t.is_empty()
        {
            last.of = t.into_owned();
            let (used, of) = (
                words::magnitude(row.cell(&prev.key)),
                words::magnitude(row.cell(&c.key)),
            );
            if let (Some(u), Some(o)) = (used, of)
                && o > 0.0
            {
                #[allow(clippy::cast_possible_truncation)] // WHY: a meter's fill, 0 to 1
                let fill = (u / o).clamp(0.0, 1.0) as f32;
                last.fill = Some(fill);
            }
            before = None;
            continue;
        }
        let (value, ink, note) = if t.is_empty() || t == "—" {
            let why = if c.role == words::Role::Rate {
                "from the next read"
            } else {
                "not reported"
            };
            ("—".to_owned(), Ink::Dim, why.to_owned())
        } else {
            (t.into_owned(), tone.into(), String::new())
        };
        out.push(Stat {
            label: c.title.clone(),
            value,
            ink,
            of: String::new(),
            fill: None,
            note,
        });
        before = Some(c);
    }
    out
}
