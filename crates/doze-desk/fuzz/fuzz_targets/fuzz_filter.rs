#![no_main]
//! Whatever is typed in the logs' filter box reads without a panic, and matching any line with it neither panics nor
//! returns a highlight that is empty, out of the line or off a character boundary.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (typed, line) = text.split_once('\n').unwrap_or((&text, "a line, GET /x 200"));
    let f = doze_desk::page::filter::parse(typed);
    let kept = f.keeps(line);
    for r in f.spans(line) {
        assert!(r.start < r.end && r.end <= line.len());
        assert!(line.is_char_boundary(r.start) && line.is_char_boundary(r.end));
        assert!(kept);
    }
    if f.error().is_some() {
        assert!(kept, "a refused pattern hides nothing");
    }
});
