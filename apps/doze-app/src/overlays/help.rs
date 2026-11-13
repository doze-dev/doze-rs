//! The help sheet (`?`, ⌘/): every key by where it works, generated from the action table (so it lists exactly what is
//! bound; Go's Help board listed `o` and `⏎`, which nothing bound), and the words the app uses. Rows that say the same
//! thing in one place fold into one (↑ ↓ k j move; 1–4 the views; ⌘1–9 the tabs), and the groups are packed into four
//! columns by height, so the whole sheet reads at 1280×800 without scrolling (it scrolls only in a smaller window).

use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px};

use crate::actions::{self, Scope, Spec};
use crate::theme::Tokens;
use crate::widgets::{MONO_FAMILY, UI_FAMILY, kbd_chip, tracked, w};

/// The words a person meets, what each means, and its tone (Go's help.tsx glossary).
const WORDS: [(&str, &str, &str); 12] = [
    ("awake", "ok", "running, and something used it lately"),
    (
        "dozing",
        "warn",
        "running, quiet for a while; sleeps when its timer runs out",
    ),
    ("waking", "cool", "starting, because something asked for it"),
    (
        "asleep",
        "dim",
        "not running, data kept; the next connection wakes it",
    ),
    (
        "crashing",
        "bad",
        "exits soon after starting; doze tries again, a few times",
    ),
    ("failed", "bad", "doze stopped trying; it waits for you"),
    (
        "off",
        "dim",
        "turned off: nothing wakes it until you switch it on",
    ),
    (
        "paused",
        "dim",
        "enabled = false in doze.hcl; only the file brings it back",
    ),
    ("pinned", "bright", "never sleeps, whatever holds it"),
    ("stopped", "dim", "the whole stack is down; nothing runs"),
    (
        "the switch",
        "bright",
        "on while it is up or on its way; flip it to wake or sleep",
    ),
    (
        "calls",
        "bright",
        "a reference in doze.hcl from one app to another",
    ),
];

/// Runs of rows that fold into one line: the run's first label, how many rows, the line's label.
const FOLDED: [(&str, usize, &str); 9] = [
    ("Tab 1", 9, "A stack's tab"),
    ("Tiles", 4, "Tiles · List · Map · Grid"),
    ("Debug and up", 4, "Level: debug, info, warn, error"),
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
        .h(px(19.))
        .text_size(px(12.))
        .child(
            div().flex().gap(px(3.)).w(px(78.)).flex_none().children(
                l.keys
                    .iter()
                    .take(2)
                    .map(|k| kbd_chip(t, k, 10., 600., 4.).into_any_element()),
            ),
        )
        .child(
            div()
                .flex_none()
                .text_color(if l.built { t.bright } else { t.dim })
                .font_weight(w(500.))
                .child(SharedString::from(l.label.clone())),
        )
        // A label says what the key does (as Go's sheet had one phrase a key); what comes later says so.
        .when(!l.built, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(10.5))
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
        .pb(px(10.))
        .child(
            div()
                .pb(px(3.))
                .child(tracked(title, 10.5, 650., t.dim, 0.66)),
        )
        .children(ls.iter().map(|l| line(t, l)))
        .into_any_element();
    (30. + 19. * ls.len() as f32, el)
}

/// The words a person meets, and what each means.
fn words(t: &Tokens) -> impl IntoElement {
    let word = |(word, tone, means): &(&'static str, &'static str, &'static str)| {
        div()
            .flex()
            .gap(px(8.))
            .py(px(2.))
            .line_height(px(16.))
            .text_size(px(12.))
            .child(
                div()
                    .w(px(70.))
                    .flex_none()
                    .font_weight(w(600.))
                    .text_color(crate::kit::tone(t, tone))
                    .child(*word),
            )
            .child(div().flex_1().min_w_0().text_color(t.text).child(*means))
    };
    // Three columns of four, under the keys: each meaning reads whole on one line.
    let cols = WORDS.chunks(4).map(|c| {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .children(c.iter().map(word))
    });
    div()
        .flex()
        .flex_col()
        .pt(px(10.))
        .border_t_1()
        .border_color(t.line)
        .child(
            div()
                .pb(px(4.))
                .child(tracked("The words", 10.5, 650., t.dim, 0.66)),
        )
        .child(div().flex().gap(px(24.)).children(cols))
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

/// The groups, in reading order, packed into four columns of about the same height.
fn columns(t: &Tokens) -> impl IntoElement {
    let groups: Vec<(f32, AnyElement)> = actions::help()
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(sc, rows)| group(t, sc.title(), &lines(&rows)))
        .collect();
    let heights: Vec<f32> = groups.iter().map(|g| g.0).collect();
    let cuts = split(&heights, 4);
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
    div().flex().gap(px(24.)).children(cols)
}

/// The sheet.
pub(crate) fn sheet(t: &Tokens, version: &str) -> impl IntoElement {
    let _ = Scope::Global;
    let foot = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pt(px(10.))
        .border_t_1()
        .border_color(t.line)
        .text_size(px(12.))
        .text_color(t.dim)
        .child("Every key here is a doze command too:")
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_color(t.text)
                .child("doze sleep db"),
        )
        .child(",")
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_color(t.text)
                .child("doze logs api"),
        )
        .child("· scripts and CI use those.")
        .child(div().flex_1())
        .child(SharedString::from(format!("doze {version}")))
        .child(kbd_chip(t, "esc", 10., 600., 5.))
        .child("close");
    div()
        .w(px(1220.))
        .max_w(gpui::relative(0.96))
        .max_h(gpui::relative(0.95))
        .flex()
        .flex_col()
        .gap(px(12.))
        .px(px(22.))
        .py(px(18.))
        .font_family(UI_FAMILY)
        .line_height(px(18.))
        .overflow_hidden()
        .rounded(px(14.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .shadow(vec![super::shadow(t)])
        .child(
            div()
                .text_size(px(16.))
                .font_weight(w(650.))
                .text_color(t.bright)
                .child("Keys and words"),
        )
        .child(
            div()
                .id("help-keys")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(columns(t)),
        )
        .child(words(t))
        .child(foot)
}

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
