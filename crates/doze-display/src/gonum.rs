//! Numbers read and written the way the Go surfaces do, so a cell or a scale label reads the same in the CLI and the
//! app: fixed-point figures (`+Inf` and `NaN` included), Go's `max`/`min` (a NaN wins), and `strconv.ParseFloat`'s
//! grammar, which takes hex floats and digit underscores and refuses a figure too large to hold.

/// `v` with `prec` decimals, as Go's `%.Nf` writes it.
pub(crate) fn fixed(v: f64, prec: usize) -> String {
    if v.is_nan() {
        "NaN".to_owned()
    } else if v.is_infinite() {
        if v > 0.0 { "+Inf" } else { "-Inf" }.to_owned()
    } else {
        format!("{v:.prec$}")
    }
}

/// Go's builtin `max` for floats: a NaN wins, and `+0` beats `-0`.
pub(crate) fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// Go's builtin `min` for floats: a NaN wins, and `-0` beats `+0`.
pub(crate) fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// The lower case of `c` by Unicode's simple mapping, as Go's `unicode.ToLower` (and so `strings.ToLower`) takes
/// it. Rust's full mapping differs only for `İ`, which it lowers to two characters.
pub(crate) fn lower(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => 'i', // WHY: U+0130 is the only character whose full lowercase is longer; its simple one is 'i'
    }
}

/// `s` lowered character by character, as Go's `strings.ToLower` does (no final-sigma rule).
pub(crate) fn lowercase(s: &str) -> String {
    s.chars().map(lower).collect()
}

/// The upper case of `c` by Unicode's simple mapping, as Go's `unicode.ToUpper`. Where the full mapping is longer
/// than one character, the simple one is the character itself, except for the Greek letters with a subscript iota,
/// which take their title-case form.
pub(crate) fn upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => match u32::from(c) {
            n @ (0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7) => {
                char::from_u32(n + 8).unwrap_or(c)
            }
            0x1FB3 => '\u{1FBC}',
            0x1FC3 => '\u{1FCC}',
            0x1FF3 => '\u{1FFC}',
            _ => c,
        },
    }
}

// fuzz:parser fuzz_number
/// `s` as Go's `strconv.ParseFloat(s, 64)` reads it, or `None` where Go returns an error: a syntax it does not take,
/// or a figure beyond the largest float.
#[must_use]
pub fn parse_float(s: &str) -> Option<f64> {
    if let Some(v) = special(s) {
        return Some(v);
    }
    let lit = crate::goparse::read_float(s)?;
    let v = if lit.hex {
        crate::goparse::hex_value(&lit)
    } else {
        let digits: String = s.chars().filter(|&c| c != '_').collect();
        digits.parse::<f64>().ok()?
    };
    v.is_finite().then_some(v)
}

/// `inf`, `infinity` (either with a sign) and `nan`, in any case.
fn special(s: &str) -> Option<f64> {
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'+') => (1.0, s.get(1..)?),
        Some(b'-') => (-1.0, s.get(1..)?),
        _ => {
            if s.eq_ignore_ascii_case("nan") {
                return Some(f64::NAN);
            }
            (1.0, s)
        }
    };
    (rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity"))
        .then_some(sign * f64::INFINITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The forms Go takes and Rust's own parser does not (and the reverse), and an overflow, which Go refuses.
    #[test]
    fn parse_float_takes_gos_grammar() {
        for (s, want) in [
            ("1_000.5", Some(1000.5)),
            ("0x1p4", Some(16.0)),
            ("0x_1.8p1", Some(3.0)),
            ("-Infinity", Some(f64::NEG_INFINITY)),
            ("1e400", None),
            ("+nan", None),
            ("1__0", None),
            ("_1", None),
            ("0x1", None),
            ("infx", None),
            ("", None),
            (".", None),
            ("1.e5", Some(1e5)),
        ] {
            assert_eq!(parse_float(s), want, "{s}");
        }
        assert!(parse_float("NaN").is_some_and(f64::is_nan));
    }
}
