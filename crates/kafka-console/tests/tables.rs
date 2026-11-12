//! Tables: the sticky header and the middle ellipsis (`docs/port/console/polish.md`, P1). The CSS is what makes them work, so
//! the rules are checked in the stylesheet the console ships.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

const CSS: &str = include_str!("../assets/console.css");

/// The rule that is last for a selector, so that what overrides the kit is what is checked.
fn last_rule<'a>(selector: &str) -> &'a str {
    let at = CSS
        .rfind(&format!("\n{selector}{{"))
        .unwrap_or_else(|| panic!("no rule for {selector}"));
    let rest = &CSS[at..];
    &rest[..rest.find('}').unwrap() + 1]
}

#[test]
fn a_table_header_sticks_under_the_top_bar_and_the_table_is_not_its_scroll_container() {
    let sticky = last_rule(".t thead th");
    assert!(
        sticky.contains("position:sticky") && sticky.contains("z-index:2"),
        "{sticky}"
    );
    // The top bar is 52 px: the header sticks right under it, no more and no less.
    assert!(CSS.contains(".topbar{height:52px"), "the bar's height");
    assert!(sticky.contains("top:52px"), "{sticky}");
    // overflow:hidden on the table (the kit's way to round its corners) would make the table the scroll container.
    let table = last_rule("table.t");
    assert!(table.contains("overflow:visible"), "{table}");
    assert!(
        table.contains("border-collapse:separate"),
        "a stuck header carries its borders only when they are separate: {table}"
    );
    // The corners are drawn by the cells instead.
    for corner in [
        ".t thead th:first-child{border-top-left-radius:13px}",
        ".t thead th:last-child{border-top-right-radius:13px}",
        ".t tbody tr:last-child td:first-child{border-bottom-left-radius:13px}",
        ".t tbody tr:last-child td:last-child{border-bottom-right-radius:13px}",
    ] {
        assert!(CSS.contains(corner), "{corner}");
    }
    // A table inside an overlay scrolls with the overlay: its header does not stick to the top bar's line.
    assert!(CSS.contains(".ovl .t thead th{position:static}"));
    // Nothing else between the table and the page scrolls: no ancestor rule of the card or the main area clips.
    for ancestor in [".main{", ".card{", ".root{"] {
        for rule in CSS.split(ancestor).skip(1) {
            let body = &rule[..rule.find('}').unwrap_or(rule.len())];
            assert!(
                !body.contains("overflow:hidden") && !body.contains("overflow:auto"),
                "{ancestor}{body}"
            );
        }
    }
}

#[test]
fn a_long_key_gives_way_in_the_middle_and_keeps_its_end() {
    let a = last_rule(".mid-a");
    assert!(
        a.contains("overflow:hidden")
            && a.contains("text-overflow:ellipsis")
            && a.contains("white-space:nowrap"),
        "{a}"
    );
    let b = last_rule(".mid-b");
    assert!(
        b.contains("flex:none") && b.contains("white-space:nowrap"),
        "the end never shrinks: {b}"
    );
    let outer = last_rule(".mid");
    assert!(
        outer.contains("display:inline-flex") && outer.contains("max-width:100%"),
        "{outer}"
    );
    assert!(
        last_rule(".t td.mid-cell").contains("max-width:"),
        "a cell must be bounded for the ellipsis to apply"
    );
}

#[test]
fn a_choice_that_takes_a_value_shows_its_field_only_while_it_is_chosen_with_no_script() {
    assert!(CSS.contains("\n.opt-row .opt-in{display:none}"));
    assert!(CSS.contains("\n.opt-row:has(input[type=radio]:checked) .opt-in{display:block}"));
}
