#![no_main]
//! Any text, read as psql's output, without a panic; every row has the width asked for; and rows of fields without the
//! separator or a line break, written as psql writes them, read back as themselves.

#[path = "../../src/rows.rs"]
#[allow(dead_code)]
mod rows;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    let n = s.len() % 4 + 1;
    assert!(rows::split(s, n).iter().all(|r| r.len() == n));
    let fields: Vec<String> = s
        .split(|c| c == rows::SEP || c == '\n' || c == '\r')
        .map(str::to_owned)
        .collect();
    let want: Vec<Vec<String>> = fields
        .chunks_exact(2)
        .map(<[String]>::to_vec)
        .collect();
    let text: String = want.iter().map(|r| r.join(&rows::SEP.to_string()) + "\n").collect();
    assert_eq!(rows::split(&text, 2), want);
});
