use std::collections::BTreeSet;

use super::*;

const SNAPSHOT: &str = include_str!("../../design/boards.json");

/// The `data-op` names of one board in the committed snapshot: the keys of its `"ops": {...}` object.
fn ops_of(id: &str) -> Vec<String> {
    let at = SNAPSHOT
        .find(&format!("\"{id}\": {{"))
        .unwrap_or_else(|| panic!("{id} is not in design/boards.json"));
    let rest = &SNAPSHOT[at..];
    let ops = rest.find("\"ops\": {").expect("a board has ops") + "\"ops\": {".len();
    let end = rest[ops..].find('}').unwrap();
    rest[ops..ops + end]
        .split(',')
        .filter_map(|pair| {
            let key = pair.split(':').next()?.trim().trim_matches('"');
            (!key.is_empty()).then(|| key.to_owned())
        })
        .collect()
}

#[test]
fn actions_match_boards() {
    let drawn: BTreeSet<String> = WRITE_BOARDS
        .iter()
        .chain(&GROUP_WRITE_BOARDS)
        .flat_map(|id| ops_of(id))
        .collect();
    let listed: BTreeSet<String> = Action::ALL.iter().map(|a| a.op().to_owned()).collect();
    let missing: Vec<_> = drawn.difference(&listed).collect();
    let unused: Vec<_> = listed.difference(&drawn).collect();
    assert!(
        missing.is_empty(),
        "a board draws a write that actions.rs does not list: {missing:?}"
    );
    assert!(
        unused.is_empty(),
        "actions.rs lists a write that no board draws: {unused:?}"
    );
}

#[test]
fn an_action_is_found_by_its_name_and_nothing_else() {
    for a in Action::ALL {
        assert_eq!(Action::named(a.op()), Some(a));
    }
    for bad in [
        "",
        "produce",
        "Produce ",
        "Fetch",
        "Metadata",
        "CreateTopics\n",
    ] {
        assert_eq!(Action::named(bad), None, "{bad:?}");
    }
    let names: BTreeSet<_> = Action::ALL.iter().map(|a| a.op()).collect();
    assert_eq!(names.len(), Action::ALL.len(), "two actions share a name");
}
