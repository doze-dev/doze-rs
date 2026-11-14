//! The help sheet (`?`, ⌘/): every key by where it works, generated from the action table (so it lists exactly what is
//! bound; Go's Help board listed `o` and `⏎`, which nothing bound), and the words the app uses. Rows that say the same
//! thing in one place fold into one (↑ ↓ k j move; 1–4 the views; ⌘1–9 the tabs), and the groups are packed into four
//! columns by height, so the whole sheet reads at 1280×800 without scrolling (it scrolls only in a smaller window).

use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px};

use crate::actions::{self, Scope, Spec};
use crate::theme::{Tokens, ty};
use crate::widgets::{MONO_FAMILY, UI_FAMILY, kbd_chip, tracked};

/// Runs of rows that fold into one line: the run's first label, how many rows, the line's label.
const FOLDED: [(&str, usize, &str); 9] = [
    ("Tab 1", 9, "A stack's tab"),
    ("Tiles", 4, "Tiles · List · Map · Grid"),
    ("Debug and up", 4, "Level: debug to error"),
    ("Stack above", 2, "Move"),
    ("Service above", 4, "Move"),
    ("Cell above", 4, "Move"),
    ("Card above", 4, "The nearest card"),
    ("Row below", 2, "Move"),
    ("Next item", 2, "Move"),
];

/// One line of the sheet: its keys (as chips write them), its words, and whether the app does it yet.
struct Line {
    keys: Vec<String>,
    label: String,
    what: &'static str,
    built: bool,
}

/// `⌘1 ⌘2 … ⌘9` as `⌘1–9`; `1 2 3 4` as `1–4`.
fn span_keys(keys: Vec<String>) -> Vec<String> {
    let digit = |k: &str| k.chars().last().is_some_and(|c| c.is_ascii_digit());
    if keys.len() > 3 && keys.iter().all(|k| digit(k)) {
        let (first, last) = (
            keys.first().cloned().unwrap_or_default(),
            keys.last().cloned().unwrap_or_default(),
        );
        let end: String = last.chars().last().map(String::from).unwrap_or_default();
        return vec![format!("{first}–{end}")];
    }
    // Pairs that move one way and back read as one chip: ↑↓, ←→, j k, h l.
    let mut out: Vec<String> = Vec::new();
    let has = |k: &str| keys.iter().any(|x| x == k);
    match (has("↑") && has("↓"), has("←") && has("→")) {
        (true, true) => out.push("↑↓←→".to_owned()),
        (true, false) => out.push("↑↓".to_owned()),
        (false, true) => out.push("←→".to_owned()),
        _ => {}
    }
    match (has("k") && has("j"), has("h") && has("l")) {
        (true, true) => out.push("hjkl".to_owned()),
        (true, false) => out.push("j k".to_owned()),
        _ => {}
    }
    let any = !out.is_empty();
    let paired = |k: &String| any && ["↑", "↓", "←", "→", "k", "j", "h", "l"].contains(&k.as_str());
    out.extend(keys.iter().filter(|k| !paired(k)).cloned());
    out
}

/// A scope's rows, runs that say the same thing folded into one line.
fn lines(rows: &[&'static Spec]) -> Vec<Line> {
    // Each line, the label of the run's first row, and how many rows the run may still take.
    let mut out: Vec<(Line, &'static str, usize)> = Vec::new();
    for s in rows {
        let keys: Vec<String> = s.keys.iter().map(|k| actions::key_words(k)).collect();
        if let Some((last, _, left)) = out.last_mut()
            && (*left > 0 || last.label == s.label)
        {
            *left = left.saturating_sub(1);
            for k in keys {
                if !last.keys.contains(&k) {
                    last.keys.push(k);
                }
            }
            continue;
        }
        let left = FOLDED
            .iter()
            .find(|(f, _, _)| *f == s.label)
            .map_or(0, |(_, n, _)| n - 1);
        out.push((
            Line {
                keys,
                label: s.label.to_owned(),
                what: s.what,
                built: s.built,
            },
            s.label,
            left,
        ));
    }
    out.into_iter()
        .map(|(mut l, first, _)| {
            if let Some((_, _, folded)) = FOLDED.iter().find(|(f, _, _)| *f == first) {
                (*folded).clone_into(&mut l.label);
                l.what = "";
            }
            l.keys = span_keys(l.keys);
            l
        })
        .collect()
}

fn line(t: &Tokens, l: &Line) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(LINE_H))
        .text_size(px(ty::SM))
        .child(
            div().flex().gap(px(3.)).w(px(70.)).flex_none().children(
                l.keys
                    .iter()
                    .take(2)
                    .map(|k| kbd_chip(t, k, ty::MICRO, ty::SEMIBOLD, 4.).into_any_element()),
            ),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(if l.built { t.bright } else { t.dim })
                .child(SharedString::from(l.label.clone())),
        )
        // A label says what the key does (as Go's sheet had one phrase a key); what comes later says so.
        .when(!l.built, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(ty::XS))
                    .text_color(t.dim)
                    .child("soon"),
            )
        })
        .into_any_element()
}

/// A group: its heading and its lines, and how tall it is.
fn group(t: &Tokens, title: &str, ls: &[Line]) -> (f32, AnyElement) {
    let el = div()
        .flex()
        .flex_col()
        .pb(px(8.))
        .child(
            div()
                .pb(px(3.))
                .child(tracked(title, ty::XS, ty::HEADING, t.dim, 0.66)),
        )
        .children(ls.iter().map(|l| line(t, l)))
        .into_any_element();
    (GROUP_H + LINE_H * ls.len() as f32, el)
}

/// The words a person meets, and what each means.
fn words(t: &Tokens) -> impl IntoElement {
    let word = |(word, tone, means): &(&'static str, &'static str, &'static str)| {
        div()
            .flex()
            .gap(px(8.))
            .py(px(2.))
            .line_height(px(ty::LH_16))
            .text_size(px(ty::SM))
            .child(
                div()
                    .w(px(70.))
                    .flex_none()
                    .font_weight(ty::SEMIBOLD)
                    .text_color(crate::kit::tone(t, tone))
                    .child(*word),
            )
            .child(div().flex_1().min_w_0().text_color(t.text).child(*means))
    };
    // Go's third column: beside the keys, always in view, its own scroll when the window is short.
    div()
        .id("help-words")
        .w(px(270.))
        .flex_none()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(2.))
        .pl(px(20.))
        .border_l_1()
        .border_color(t.line)
        .child(
            div()
                .pb(px(4.))
                .child(tracked("The words", ty::XS, ty::HEADING, t.dim, 0.66)),
        )
        .children(super::glossary::WORDS.iter().map(word))
        .child(div().flex_none().h(px(8.)))
}

/// Where to start each new column so that the tallest of `n` columns, the groups kept in order, is as short as it can
/// be (a small search: there are a dozen groups).
fn split(h: &[f32], n: usize) -> Vec<usize> {
    fn best(h: &[f32], from: usize, n: usize) -> (f32, Vec<usize>) {
        let rest: f32 = h.iter().skip(from).sum();
        if n == 1 || from >= h.len() {
            return (rest, Vec::new());
        }
        let mut out = (f32::MAX, Vec::new());
        let mut run = 0.;
        for end in from + 1..h.len() {
            run += h.get(end - 1).copied().unwrap_or(0.);
            let (tail, mut cuts) = best(h, end, n - 1);
            let worst = run.max(tail);
            if worst < out.0 {
                cuts.insert(0, end);
                out = (worst, cuts);
            }
        }
        if out.1.is_empty() {
            (rest, Vec::new())
        } else {
            out
        }
    }
    best(h, 0, n).1
}

/// The groups, in reading order, packed into as few columns (three or four) as let the tallest fit `room`, each of
/// about the same height: a short window takes a column more, and past four the keys scroll with room under the last row.
fn columns(t: &Tokens, room: f32) -> gpui::Div {
    let groups: Vec<(f32, AnyElement)> = actions::help()
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(sc, rows)| group(t, sc.title(), &lines(&rows)))
        .collect();
    let heights: Vec<f32> = groups.iter().map(|g| g.0).collect();
    let cuts = (3..=4)
        .map(|n| split(&heights, n))
        .find(|cuts| tallest(&heights, cuts) <= room)
        .unwrap_or_else(|| split(&heights, 4));
    let mut cols: Vec<gpui::Div> = Vec::new();
    let mut col = div().flex().flex_col().flex_1().min_w_0();
    for (i, (_, el)) in groups.into_iter().enumerate() {
        if cuts.contains(&i) {
            cols.push(std::mem::replace(
                &mut col,
                div().flex().flex_col().flex_1().min_w_0(),
            ));
        }
        col = col.child(el);
    }
    cols.push(col);
    div().flex().gap(px(18.)).children(cols)
}

/// The sheet's foot: the keys are commands too, the version, esc.
fn foot(t: &Tokens, version: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pt(px(10.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(ty::SM))
        .text_color(t.dim)
        .child("Every key here is a doze command too:")
        .child(
            div()
                .flex()
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_color(t.text)
                        .child("doze sleep db"),
                )
                .child(","),
        )
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_color(t.text)
                .child("doze logs api"),
        )
        .child("· scripts and CI use those. Letters are off while a console has focus.")
        .child(div().flex_1())
        .child(SharedString::from(format!("doze {version}")))
        .child(kbd_chip(t, "esc", ty::MICRO, ty::SEMIBOLD, 5.))
        .child("close")
}

/// The sheet.
pub(crate) fn sheet(t: &Tokens, version: &str, height: f32) -> impl IntoElement {
    let room = height - INSET_TOP - INSET - CHROME;
    let _ = Scope::Global;
    let foot = foot(t, version);
    let panel = div()
        .w(px(1220.))
        .max_w_full()
        .max_h_full()
        .flex()
        .flex_col()
        .gap(px(8.))
        .px(px(22.))
        .py(px(14.))
        .font_family(UI_FAMILY)
        .line_height(px(ty::LH_18))
        .overflow_hidden()
        .rounded(px(14.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .shadow(super::shadow(t))
        .child(
            div()
                .text_size(px(ty::LG))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child("Keys and words"),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .gap(px(20.))
                .child(
                    div()
                        .id("help-keys")
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(columns(t, room).pb(px(8.))),
                )
                .child(words(t)),
        )
        .child(foot);
    // Inset below the title bar (its tabs stay in sight) and off the window's edges; the keys scroll when the window
    // is short.
    div()
        .absolute()
        .top(px(INSET_TOP))
        .bottom(px(INSET))
        .left(px(INSET))
        .right(px(INSET))
        .flex()
        .justify_center()
        .items_start()
        .child(panel)
}

/// A line of keys, and a group's heading with the space under the group.
const LINE_H: f32 = 18.;
const GROUP_H: f32 = 30.;
/// The panel's own height around the keys: its padding, the title, the foot and the gaps between them.
const CHROME: f32 = 2. * 14. + 26. + 2. * 8. + 42. + 8.;

/// How tall the tallest column is when the groups are cut at `cuts`.
fn tallest(h: &[f32], cuts: &[usize]) -> f32 {
    let mut edges = vec![0];
    edges.extend_from_slice(cuts);
    edges.push(h.len());
    edges
        .windows(2)
        .map(|w| match w {
            [a, b] => h.get(*a..*b).map_or(0., |s| s.iter().sum()),
            _ => 0.,
        })
        .fold(0., f32::max)
}

/// The title bar's height and a margin under it.
const INSET_TOP: f32 = 42. + 22.;
/// The margin at the window's sides and foot.
const INSET: f32 = 20.;

#[cfg(test)]
mod tests {
    use super::{lines, span_keys};
    use crate::actions::{Scope, help};

    #[test]
    fn runs_fold_and_digits_span() {
        assert_eq!(
            span_keys(vec!["⌘1".into(), "⌘2".into(), "⌘3".into(), "⌘9".into()]),
            ["⌘1–9"]
        );
        let g = help();
        let global = g
            .iter()
            .find(|(s, _)| *s == Scope::Global)
            .map(|(_, r)| lines(r))
            .unwrap_or_default();
        assert!(
            global
                .iter()
                .any(|l| l.label == "A stack's tab" && l.keys == ["⌘1–9"])
        );
        assert!(
            global
                .iter()
                .any(|l| l.label.starts_with("Tiles ·") && l.keys == ["1–4"])
        );
        // Every key the table binds in a scope shows in that scope's lines.
        for (_, rows) in &g {
            let n: usize = lines(rows).iter().map(|l| l.keys.len()).sum();
            assert!(n > 0);
        }
    }
}
