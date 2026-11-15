#![no_main]
//! Any text, read as a Go duration and as a Go time, without a panic; a duration that parses is written back in a
//! form that parses to the same value, and so is a time.

use doze_control::{Timestamp, format_go_duration, parse_go_duration};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Some(d) = parse_go_duration(&text) {
        assert_eq!(parse_go_duration(&format_go_duration(d)), Some(d));
    }
    let t: Timestamp = serde_json::from_value(serde_json::Value::String(text.into_owned())).expect("any string is a time");
    if let Some(n) = t.unix_nanos() {
        assert_eq!(Timestamp::from_unix_nanos(n).unix_nanos(), Some(n));
    }
});
