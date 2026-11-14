//! Any bytes: Go's token stream and syntax check never panic, never hang, and every token and
//! diagnostic lies inside the input, with tokens in order and covering it end to end (only spaces and
//! tabs between them).
#![no_main]

use doze_hcl::check::check;
use doze_hcl::lex::{Kind, lex};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let toks = lex(data);
    assert_eq!(toks.last().map(|t| t.kind), Some(Kind::Eof));
    let mut at = if data.starts_with(&[0xef, 0xbb, 0xbf]) { 3 } else { 0 };
    for t in &toks {
        assert!(t.start <= t.end && t.end <= data.len());
        let gap = data.get(at..t.start).unwrap_or_default();
        assert!(gap.iter().all(|b| *b == b' ' || *b == b'\t'), "gap {gap:?}");
        at = t.end;
    }
    for d in check(data, "x.hcl").iter() {
        let r = d.subject.unwrap_or_default();
        assert!(r.start.byte as usize <= data.len() && r.end.byte as usize <= data.len());
    }
});
