//! Values given on the command line: durations as Go writes them (`--timeout`, `logs --since`, the published docs use
//! `5m`, `90s`, `1h30m`), `--var name=value`, and the shells completion knows.

use std::time::Duration;

// fuzz:parser fuzz_duration
/// A duration: one or more `<number><unit>`, units `ms`, `s`, `m`, `h` (and `us`, `ns`); fractions allowed (`1.5h`).
/// Zero and anything negative are refused: every flag that takes one needs a positive time.
pub fn duration(s: &str) -> Result<Duration, String> {
    let bad = || format!("`{s}` is not a duration: write it as 90s, 5m or 1h30m");
    let mut rest = s.trim();
    if rest.is_empty() {
        return Err(bad());
    }
    let mut total = 0f64;
    while !rest.is_empty() {
        let num_len = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let (num, tail) = rest.split_at(num_len);
        let unit_len = tail
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(tail.len());
        let (unit, tail) = tail.split_at(unit_len);
        let n: f64 = num.parse().map_err(|_| bad())?;
        let secs = match unit {
            "h" => 3600.0,
            "m" => 60.0,
            "s" => 1.0,
            "ms" => 1e-3,
            "us" | "µs" => 1e-6,
            "ns" => 1e-9,
            _ => return Err(bad()),
        };
        total += n * secs;
        rest = tail;
    }
    // A year is far past any wait doze has; past it is a typo, and f64 to Duration would refuse it anyway.
    if !(total > 0.0 && total < 365.0 * 86400.0) {
        return Err(format!(
            "`{s}` is not a time doze can wait: give a positive duration under a year"
        ));
    }
    Ok(Duration::from_secs_f64(total))
}

/// `name=value`, the name an HCL identifier.
pub fn var(s: &str) -> Result<(String, String), String> {
    let Some((name, value)) = s.split_once('=') else {
        return Err(format!("`{s}` has no `=`: write --var name=value"));
    };
    let ident = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ident {
        return Err(format!(
            "`{name}` is not a variable name: letters, digits, _ and -, starting with a letter"
        ));
    }
    Ok((name.to_string(), value.to_string()))
}

/// The shells `doze completion` writes for (as the Go doze).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_as_the_docs_write_them() {
        assert_eq!(duration("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(duration("1h30m"), Ok(Duration::from_secs(5400)));
        assert_eq!(duration("1.5s"), Ok(Duration::from_millis(1500)));
        assert_eq!(duration("250ms"), Ok(Duration::from_millis(250)));
        for bad in ["", "5", "m", "0s", "-5m", "5x", "1..2s", "99999999h"] {
            assert!(duration(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            var("region=eu-west-1"),
            Ok(("region".into(), "eu-west-1".into()))
        );
        assert!(var("=x").is_err() && var("1a=x").is_err() && var("x").is_err());
    }
}
