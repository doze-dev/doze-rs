//! Durations as Go writes them (`500ms`, `1m30s`, `1h0m0s`), which is how the flags of the Go edition take them and
//! how the conformance harness passes them.

use std::time::Duration;

/// Parses a Go duration: a sequence of numbers (with an optional fraction), each followed by a unit
/// (`ns`, `us`, `µs`, `ms`, `s`, `m`, `h`); or `0`.
///
/// # Errors
/// What is wrong with it, in words for the person who typed it.
pub(crate) fn parse(text: &str) -> Result<Duration, String> {
    if text == "0" {
        return Ok(Duration::ZERO);
    }
    let mut rest = text;
    let mut total = Duration::ZERO;
    if rest.is_empty() {
        return Err("a duration is a number and a unit, like 500ms or 1m30s".to_owned());
    }
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .ok_or_else(|| format!("{text:?} has a number with no unit"))?;
        let (number, after) = rest.split_at(digits);
        let number: f64 = number
            .parse()
            .map_err(|_| format!("{text:?} has {number:?} where a number should be"))?;
        let unit_end = after
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(after.len());
        let (unit, next) = after.split_at(unit_end);
        let seconds = match unit {
            "ns" => 1e-9,
            "us" | "µs" | "μs" => 1e-6,
            "ms" => 1e-3,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            other => {
                return Err(format!(
                    "{text:?} has the unit {other:?}, which is none of ns, us, ms, s, m, h"
                ));
            }
        };
        total += Duration::try_from_secs_f64(number * seconds)
            .map_err(|_| format!("{text:?} is too long to be a duration"))?;
        rest = next;
    }
    Ok(total)
}

/// A failed-login delay: a duration of at most [`kafka_server::MOST_FAILED_LOGIN_DELAY`] (SEC-236). A longer wait holds a
/// connection and a task for every guess, so it is refused at start rather than cut quietly.
///
/// # Errors
/// What is wrong with it, in words for the person who typed it.
pub(crate) fn failed_login_delay(text: &str) -> Result<Duration, String> {
    let delay = parse(text)?;
    let most = kafka_server::MOST_FAILED_LOGIN_DELAY;
    if delay > most {
        return Err(format!(
            "{text:?} is longer than the {most:?} a failed login may be made to wait: a longer wait holds a connection \
             for every guess"
        ));
    }
    Ok(delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_durations_as_go_prints_them_and_as_people_write_them() {
        let ok = [
            ("0", Duration::ZERO),
            ("500ms", Duration::from_millis(500)),
            ("3s", Duration::from_secs(3)),
            ("1m0s", Duration::from_secs(60)),
            ("1h0m0s", Duration::from_secs(3600)),
            ("1m30s", Duration::from_secs(90)),
            ("1.5s", Duration::from_millis(1500)),
            ("250us", Duration::from_micros(250)),
            ("0s", Duration::ZERO),
        ];
        for (text, want) in ok {
            assert_eq!(parse(text), Ok(want), "{text}");
        }
        for bad in ["", "5", "s", "5x", "-3s", "1h 5m", "1..5s"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
