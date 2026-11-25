#![no_main]
//! Any pattern against any name: no panic, and the answer is the one a plain dynamic-programming match gives (the walk with one
//! backtrack point must agree with it on every input, and take no more than pattern x name steps).

use libfuzzer_sys::fuzz_target;

/// The reference: `*` any run, `?` any one character, table of (pattern prefix, name prefix).
fn reference(p: &[char], n: &[char]) -> bool {
    let mut row = vec![false; n.len() + 1];
    row[0] = true;
    for pc in p {
        let mut next = vec![false; n.len() + 1];
        if *pc == '*' {
            let mut seen = false;
            for i in 0..=n.len() {
                seen |= row[i];
                next[i] = seen;
            }
        } else {
            for i in 1..=n.len() {
                next[i] = row[i - 1] && (*pc == '?' || *pc == n[i - 1]);
            }
        }
        row = next;
    }
    row[n.len()]
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The first line is the pattern, the rest the name; both are kept short enough for the reference to be quick.
    let (pattern, name) = text.split_once('\n').unwrap_or((text, ""));
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().take(64).collect(), name.chars().take(64).collect());
    let (ps, ns): (String, String) = (p.iter().collect(), n.iter().collect());
    assert_eq!(aws_console::lab::glob::fits(&ps, &ns), reference(&p, &n), "{ps:?} {ns:?}");
    let _ = aws_console::lab::glob::last_name(&ns);
});
