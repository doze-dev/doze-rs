use super::*;
use crate::fixtures_log::log;

fn inventory() -> LogView {
    log("inventory.levels", 0).unwrap()
}

#[test]
fn a_cell_is_as_wide_as_the_offsets_it_spans_and_the_colour_says_what_the_cleaner_did() {
    let r = ruler(&inventory());
    let flex: Vec<i64> = r.cells.iter().map(|c| c.flex).collect();
    assert_eq!(flex, [9812, 9791, 9817, 9775, 9682, 6333, 6090, 604]);
    let cells: Vec<Cell> = r.cells.iter().map(|c| c.cell).collect();
    assert_eq!(
        cells,
        [
            Cell::Clean,
            Cell::Clean,
            Cell::Clean,
            Cell::Clean,
            Cell::Dirty,
            Cell::Dirty,
            Cell::Dirty,
            Cell::Active
        ]
    );
    // The last segment is too narrow for a label (as the board draws it) and the first has its two lines.
    assert_eq!(r.cells[7].label, None);
    assert_eq!(
        r.cells[0].label,
        Some(("1 \u{b7} 3.2 MiB".to_owned(), "kept 41 %".to_owned()))
    );
    assert_eq!(
        r.cells[4].label,
        Some(("5 \u{b7} 8.0 MiB".to_owned(), "dirty".to_owned()))
    );
}

#[test]
fn the_gaps_are_where_the_segment_lost_offsets_and_only_in_cleaned_segments() {
    let v = inventory();
    let r = ruler(&v);
    assert_eq!(r.cells[0].gaps.len(), 7);
    assert!(r.cells[4].gaps.is_empty() && r.cells[7].gaps.is_empty());
    // The hatched share of a cell is the share of its offsets that were removed.
    let hatched: f64 = r.cells[0].gaps.iter().map(|g| g.1).sum();
    let removed = v.segments[0].removed() as f64 / v.segments[0].span() as f64 * 100.0;
    assert!(
        (hatched - removed).abs() < 1.0,
        "{hatched} against {removed}"
    );
    for (at, w) in &r.cells[0].gaps {
        assert!(*at >= 0.0 && at + w <= 100.1, "{at} {w}");
    }
}

#[test]
fn a_log_start_moved_up_hatches_what_is_below_it() {
    let mut v = log("orders", 0).unwrap();
    v.start = 3000;
    let r = ruler(&v);
    let (at, w) = r.cells[0].gaps[0];
    assert!(at.abs() < 0.01, "from the left edge");
    assert!((w - 3000.0 / 5980.0 * 100.0).abs() < 0.1, "{w}");
    assert!(r.key.contains(&Key::Hatching));
}

#[test]
fn the_markers_sit_where_the_offsets_are_and_the_end_is_drawn_at_the_right_edge() {
    let r = ruler(&inventory());
    let names: Vec<&str> = r.marks.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(
        names,
        [
            "log start",
            "checkpoint \u{b7} first dirty offset",
            "last stable offset = high watermark"
        ]
    );
    assert!((r.marks[1].at - 0.63316).abs() < 1e-4, "{}", r.marks[1].at);
    assert_eq!((r.marks[0].at, r.marks[2].at), (0.0, 1.0));
    assert!(r.marks[2].end);
    // With a transaction open the two are said together, as the Retention board writes them.
    let held = ruler(&log("orders", 0).unwrap());
    assert_eq!(
        held.marks.last().unwrap().label,
        "high watermark 16,104 \u{b7} last stable offset 16,098"
    );
}

#[test]
fn an_empty_log_is_one_active_cell_with_its_own_words() {
    let r = ruler(&log("orders.dlq", 0).unwrap());
    assert_eq!(r.cells.len(), 1);
    assert_eq!(r.cells[0].cell, Cell::Active);
    assert_eq!(
        r.cells[0].label,
        Some((
            "1 \u{b7} active segment".to_owned(),
            "0 B, no records".to_owned()
        ))
    );
    assert_eq!(r.marks.len(), 1);
    assert_eq!(
        r.marks[0].label,
        "log start = last stable offset = high watermark"
    );
    assert_eq!(r.key, [Key::ActiveNever]);
}
