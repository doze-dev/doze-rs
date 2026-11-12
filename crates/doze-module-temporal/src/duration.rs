//! Durations as people write them in doze.hcl and temporal reads them: `168h`, `7d`, `1h30m`, `90s`, `500ms`. Days
//! are allowed (temporal's CLI accepts them; Go's module did not), and the server reports retention in seconds
//! (`604800s`), so a declared and a reported retention compare as milliseconds.

/// The length of `s` in milliseconds, or `None` if it is not a duration: one or more `<number><unit>` parts with unit
/// `d`, `h`, `m`, `s` or `ms`, whole numbers only, and not so long it overflows.
// fuzz:parser fuzz_duration
pub(crate) fn millis(s: &str) -> Option<u64> {
    let mut rest = s.trim();
    if rest.is_empty() {
        return None;
    }
    let mut total: u64 = 0;
    while !rest.is_empty() {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let n: u64 = rest.get(..digits)?.parse().ok()?;
        rest = rest.get(digits..)?;
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        let unit = rest.get(..letters)?;
        rest = rest.get(letters..)?;
        let scale: u64 = match unit {
            "d" => 86_400_000,
            "h" => 3_600_000,
            "m" => 60_000,
            "s" => 1_000,
            "ms" => 1,
            _ => return None,
        };
        total = total.checked_add(n.checked_mul(scale)?)?;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_forms_people_write_and_the_server_reports() {
        assert_eq!(millis("7d"), Some(604_800_000));
        assert_eq!(millis("168h"), millis("7d"));
        assert_eq!(millis("604800s"), millis("7d"));
        assert_eq!(millis("1h30m"), Some(5_400_000));
        assert_eq!(millis("500ms"), Some(500));
        for bad in [
            "",
            "7",
            "d",
            "7 days",
            "1.5h",
            "-1h",
            "7y",
            "99999999999999999999d",
        ] {
            assert_eq!(millis(bad), None, "{bad:?}");
        }
    }
}
