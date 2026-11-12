use super::*;
use crate::model::{WireQuery, WireWindow};

fn view(state: LiveState) -> LiveView {
    LiveView {
        state,
        rows: vec![],
        matched: 0,
        window: WireWindow {
            rows: 10,
            capacity: 2000,
            oldest_ms: Some(1_000_000_000_000),
            requests: 10,
        },
        query: WireQuery::default(),
        refused: vec![],
        refused_total: 0,
        denied_total: 0,
        clients: 0,
        last_minute: 14,
        busiest: Some(("clickstream".into(), 412)),
        seen: 10,
        newest_id: 9,
        now_ms: 1_000_000_000_000 + (3 * 3600 + 2 * 60) * 1000,
        listen: "127.0.0.1:9092".into(),
        started_ms: Some(1_000_000_000_000),
        older: None,
        held: None,
        paused_at_ms: None,
        facets: crate::model::Facets::default(),
    }
}

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.clone(),
        })
        .collect()
}

#[test]
fn the_steady_sentence_says_how_far_back_the_ring_reaches_and_the_last_minute() {
    let t = text(&live_sentence(&view(LiveState::Steady)));
    assert!(
        t.starts_with("Everything your apps send, newest first."),
        "{t}"
    );
    assert!(
        t.contains("Right now they reach back to 01:46, 3 h 2 min."),
        "{t}"
    );
    assert!(t.ends_with("14 requests in the last minute."), "{t}");
}

#[test]
fn the_quiet_and_busy_and_paused_sentences() {
    let q = text(&live_sentence(&view(LiveState::Quiet)));
    assert!(
        q.contains("Nothing has reached the broker since it started at 01:46."),
        "{q}"
    );
    assert!(q.contains("127.0.0.1:9092"));
    let b = text(&live_sentence(&view(LiveState::Busy)));
    assert!(
        b.starts_with("A busy minute: 14 requests, 412 of them to clickstream."),
        "{b}"
    );
    let mut p = view(LiveState::Steady);
    p.paused_at_ms = Some(1_000_000_000_000 + 23_600);
    assert_eq!(
        text(&live_sentence(&p)),
        "Paused at 01:47:03.6. Everything below is as it was then."
    );
}

#[test]
fn an_empty_window_reaches_nowhere_and_one_request_is_singular() {
    assert_eq!(reach(&WireWindow::default(), 0), None);
    assert_eq!(last_minute(1), "1 request in the last minute.");
    assert_eq!(last_minute(0), "0 requests in the last minute.");
}

#[test]
fn the_pager_line_and_the_load_line() {
    let mut v = view(LiveState::Steady);
    v.matched = 40;
    assert_eq!(
        pager_line(&v),
        "Showing 0 of 40 matching rows · back to 01:46 · idle polls hidden"
    );
    assert_eq!(
        load_line(7, 571),
        "Repeats of the same call fold into one row. 7 rows stand for 571 requests."
    );
}
