#![no_main]
//! Any bytes, split anywhere: the event-stream reader never panics, and what it reads in two pieces is what it reads in one.

use aws_console::resources::deep::frames::{Reader, headers};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    drop(headers(data));
    let whole = Reader::default().push(data);
    let cut = data.first().map_or(0, |b| usize::from(*b) % (data.len() + 1));
    let (a, b) = data.split_at(cut);
    let mut r = Reader::default();
    let split = r.push(a).and_then(|mut got| {
        got.extend(r.push(b)?);
        Ok(got)
    });
    if let (Ok(w), Ok(s)) = (&whole, &split) {
        assert_eq!(w, s);
    }
});
