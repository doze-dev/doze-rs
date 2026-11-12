//! The text cases: cleaning, levels, the palette's matching, and the short sentences.

use std::time::Duration;

use doze_display as d;
use doze_display::LogLevel;
use serde_json::{Value, json};

use crate::{HOME, bits};

fn text(v: &Value) -> &str {
    v.as_str().expect("a string")
}

fn duration(v: &Value) -> Duration {
    // Go's durations may be negative; every function here reads a negative one as zero.
    Duration::from_nanos(u64::try_from(v.as_i64().expect("nanoseconds")).unwrap_or(0))
}

pub(crate) fn say(case: &str, input: &Value) -> Value {
    match case {
        "clean" => clean(input),
        "level" => {
            let line = text(input);
            let (rank, tag) = d::rank(line);
            let rank = LogLevel::ALL
                .iter()
                .position(|l| *l == rank)
                .expect("a level");
            json!({"level": d::level(line).map_or("", LogLevel::word), "rank": rank, "tag": tag.unwrap_or("")})
        }
        "fuzzy" => {
            let at = d::fuzzy_match(text(&input[0]), text(&input[1]));
            let score = d::fuzzy_score(at.as_deref().unwrap_or_default());
            json!({"ok": at.is_some(), "at": at.unwrap_or_default(), "score": score})
        }
        "short_run" => json!(d::short_run(text(input))),
        "tilde" => json!(d::tilde(text(input), Some(HOME))),
        "humanise" => json!(d::humanise(text(input))),
        "action_key" => json!(
            d::action_key(text(input))
                .map(String::from)
                .unwrap_or_default()
        ),
        "notices" => {
            let n = usize::try_from(input[0].as_u64().expect("a count")).expect("fits");
            json!([
                d::running_note(n).unwrap_or_default(),
                d::task_started(text(&input[1]))
            ])
        }
        "parse_float" => {
            let v = d::parse_float(text(input));
            json!([v.is_some(), bits(v.unwrap_or(0.0))])
        }
        "banner" => {
            let flag = |i: usize| input[i].as_bool().expect("a flag");
            let said = d::banner(flag(0), flag(1), duration(&input[2]), duration(&input[3]));
            json!([said.is_none(), said.unwrap_or_default()])
        }
        other => panic!("not a text case: {other}"),
    }
}

fn clean(input: &Value) -> Value {
    let raw = hex(text(input));
    let mut out = json!({"clean": d::clean_bytes(&raw)});
    // Go's FirstLine and CleanLines take text; the oracle asks them only of input that is.
    if let Ok(s) = std::str::from_utf8(&raw) {
        out["first_line"] = json!(d::first_line(s));
        out["lines"] = json!(d::clean_lines(s));
    }
    out
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}
