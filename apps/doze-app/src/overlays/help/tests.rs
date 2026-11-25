//! The sheet keeps every key and word it had: its inventory, read through the rail's sections, equals the one frozen
//! from the sheet before the redesign (`before.tsv`: scope, key, row, from the action table the old sheet drew).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use super::groups;
use super::model::{self, GROUP_PAST, RAIL, Section};
use crate::actions::Scope;

/// `scope \t key \t row` for every key the rail's sections show, and `Words \t \t word` for every word.
fn inventory() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for s in RAIL {
        match s {
            Section::Words => {
                for (w, _, _) in model::words() {
                    out.insert(format!("Words\t\t{w}"));
                }
            }
            Section::Keys(sc) => {
                for g in model::grouped(sc) {
                    for l in &g.lines {
                        // A line stands for its rows; each row's keys are among the line's.
                        // (Two rows may share a label, as ⌘K's two Step outs do: both stand on the line.)
                        for row in &l.rows {
                            let (_, specs) = crate::actions::help()
                                .into_iter()
                                .find(|(x, _)| *x == sc)
                                .unwrap();
                            for spec in specs.iter().filter(|r| r.label == *row) {
                                for k in spec.keys {
                                    assert!(l.raw.contains(k), "{sc:?} {row}: {k} not on its line");
                                    out.insert(format!("{sc:?}\t{k}\t{row}"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn every_key_and_word_from_before_is_still_on_the_sheet() {
    let before: BTreeSet<String> = include_str!("before.tsv")
        .lines()
        .map(str::to_owned)
        .collect();
    let now = inventory();
    let lost: Vec<_> = before.difference(&now).collect();
    let new: Vec<_> = now.difference(&before).collect();
    assert!(lost.is_empty(), "gone from the sheet: {lost:?}");
    assert!(new.is_empty(), "on the sheet but not before: {new:?}");
    assert_eq!(before.len(), 160);
}

#[test]
fn every_line_of_a_long_context_is_in_exactly_one_named_group() {
    for s in RAIL {
        let Section::Keys(sc) = s else { continue };
        let all = model::scope_lines(sc);
        let gs = model::grouped(sc);
        let shown: usize = gs.iter().map(|g| g.lines.len()).sum();
        assert_eq!(shown, all.len(), "{sc:?}: a line shown twice or not at all");
        if all.len() > GROUP_PAST {
            assert!(gs.len() > 1, "{sc:?} has {} lines in one list", all.len());
            assert!(
                gs.iter().all(|g| g.title.is_some_and(|t| t != "More")),
                "{sc:?}: a line no group names: {:?}",
                gs.iter().find(|g| g.title == Some("More"))
            );
        } else {
            assert!(gs.len() == 1 && gs[0].title.is_none(), "{sc:?} is short");
        }
    }
    // A group that names a line no context has is a typo.
    for (sc, named) in groups::GROUPS {
        let labels: Vec<String> = model::scope_lines(*sc)
            .into_iter()
            .map(|l| l.label)
            .collect();
        for (_, ls) in *named {
            for l in *ls {
                assert!(labels.iter().any(|x| x == l), "{sc:?}: no line {l}");
            }
        }
    }
}

#[test]
fn runs_fold_and_digits_span() {
    let global = model::scope_lines(Scope::Global);
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
    let map = model::scope_lines(Scope::Map);
    assert!(
        map.iter()
            .any(|l| l.label == "The nearest card" && l.keys == ["↑↓←→", "hjkl"])
    );
    // Activity's ⏎ and / are lines of their own (the old sheet folded them into Move and showed two chips of four).
    let act = model::scope_lines(Scope::Activity);
    let labels: Vec<&str> = act.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Shorter span",
            "Longer span",
            "Move",
            "That service alone",
            "Find a service"
        ]
    );
}

#[test]
fn the_filter_finds_a_key_by_its_words_in_another_context() {
    // "neighbour" is the Map's H; nothing else says it.
    let f = model::find("Neighbour");
    assert_eq!(f.keys.len(), 1);
    let (sc, ls) = &f.keys[0];
    assert_eq!(*sc, Scope::Map);
    assert_eq!(ls[0].label, "Neighbourhood");
    assert_eq!(ls[0].keys, ["H"]);
    assert_eq!(f.count(Section::Keys(Scope::Map)), 1);
    // By what it does past its label: "the minimap" is m's.
    let f = model::find("minimap");
    assert_eq!(f.keys[0].1[0].label, "Overview on or off");
    // By a key, as typed or as its chip writes it.
    let f = model::find("cmd-k");
    assert!(
        f.keys
            .iter()
            .any(|(sc, ls)| *sc == Scope::Global && ls[0].label == "Search or do…")
    );
    assert!(model::find("⌘K").count(Section::Keys(Scope::Global)) >= 1);
    // And the words, by the word or by what it means.
    let f = model::find("timer");
    assert_eq!(f.words.len(), 1);
    assert_eq!(f.words[0].0, "dozing");
    assert!(model::find("zzzz-nothing").is_empty());
    assert!(model::find("  ").is_empty());
}

#[test]
fn the_rail_steps_and_stops_at_its_ends() {
    assert_eq!(
        Section::Keys(Scope::Global).step(-1),
        Section::Keys(Scope::Global)
    );
    assert_eq!(
        Section::Keys(Scope::Global).step(1),
        Section::Keys(Scope::Library)
    );
    assert_eq!(Section::Words.step(1), Section::Words);
    assert_eq!(Section::Keys(Scope::Bell).step(1), Section::Words);
    // A context the sheet has no keys for opens on Everywhere's.
    assert_eq!(model::opening(Scope::Empty), Section::Keys(Scope::Global));
    assert_eq!(model::opening(Scope::Logs), Section::Keys(Scope::Logs));
}

#[test]
fn a_match_is_marked_where_it_is() {
    assert_eq!(model::hit("Open the stack", "stack"), Some(9..14));
    assert_eq!(model::hit("Zoom in", ""), None);
}
