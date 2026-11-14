//! Hostile sources parse in linear time (catalogue S6): hcl-edit backtracked for seconds on
//! unclosed nesting, so the Go-shaped check runs first and refuses those before hcl-edit sees them;
//! balanced deep nesting stays within hcl-edit's own limits (S5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use doze_hcl::syntax::{Source, parse_body};
use std::time::{Duration, Instant};

fn timed(text: &str) -> Duration {
    let start = Instant::now();
    let _ = parse_body(&Source::new(text));
    start.elapsed()
}

#[test]
fn unclosed_and_deep_nesting_parse_quickly() {
    let cases = [
        format!("x = {}", "[{".repeat(650)),
        format!("x = {}", "{[".repeat(650)),
        format!("x = {}", "([".repeat(650)),
        format!("x = {}", "[".repeat(4000)),
        format!("x = {}", "(".repeat(4000)),
        format!("x = {}", "\"${".repeat(1000)),
        format!("x = {}", "{a = ".repeat(1000)),
        format!("x = {}", "a ? b : ".repeat(1000)),
        format!("x = {}", "f(".repeat(2000)),
        format!("x = {}1{}", "[{a = ".repeat(30), "}]".repeat(30)),
        format!("x = {}1{}", "[".repeat(60), "]".repeat(60)),
        format!("x = {}1{}", "[".repeat(5000), "]".repeat(5000)),
        format!("{}\n", "a {\n".repeat(3000)),
    ];
    // Generous for a loaded CI runner; the slow path took over a second on 1.3 KB.
    let limit = Duration::from_millis(300);
    for text in &cases {
        let t = timed(text);
        assert!(
            t < limit,
            "{} bytes took {t:?}: {:?}…",
            text.len(),
            text.get(..40)
        );
    }
}

#[test]
fn a_huge_object_literal_is_refused_quickly() {
    let text = format!(
        "x = {{{}}}\n",
        (0..30_000)
            .map(|i| format!("a{i} = 1"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let start = Instant::now();
    let r = parse_body(&Source::new(&text));
    assert!(
        r.is_err(),
        "more than 10,000 attributes are refused (catalogue K4)"
    );
    assert!(start.elapsed() < Duration::from_millis(500));
}
