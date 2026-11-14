//! Writing YAML exactly as Go's `gopkg.in/yaml.v3` `Marshal` does, for the files the Go doze writes and people commit
//! (`doze.lock`): its key order (digits compared as numbers), its 4-space indent, and its choice of plain, single- or
//! double-quoted scalars. Checked against the Go encoder on a corpus (`tests/yaml_oracle.rs`).

use std::cmp::Ordering;

/// yaml.v3's map key order (`sorter.go`): letters before digits after a digit run, digit runs compared as numbers.
pub fn key_order(a: &str, b: &str) -> Ordering {
    let (ar, br): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut digits = false;
    for (i, (x, y)) in ar.iter().zip(br.iter()).enumerate() {
        if x == y {
            digits = x.is_ascii_digit();
            continue;
        }
        let (xl, yl) = (x.is_alphabetic(), y.is_alphabetic());
        if xl && yl {
            return x.cmp(y);
        }
        if xl || yl {
            let a_first = if digits { xl } else { yl };
            return if a_first { Ordering::Less } else { Ordering::Greater };
        }
        return digit_runs(&ar, &br, i);
    }
    ar.len().cmp(&br.len())
}

fn digit_runs(ar: &[char], br: &[char], i: usize) -> Ordering {
    let (mut an, mut bn) = (0i64, 0i64);
    let zero = |c: Option<&char>| c == Some(&'0');
    if zero(ar.get(i)) || zero(br.get(i)) {
        let before = ar.iter().take(i).rev().take_while(|c| c.is_ascii_digit());
        if before.clone().any(|c| *c != '0') {
            (an, bn) = (1, 1);
        }
    }
    let run = |s: &[char], mut n: i64| {
        let mut end = i;
        while let Some(c) = s.get(end).filter(|c| c.is_ascii_digit()) {
            n = n.wrapping_mul(10).wrapping_add(i64::from(u32::from(*c) - u32::from('0')));
            end += 1;
        }
        (n, end)
    };
    let ((an, ai), (bn, bi)) = (run(ar, an), run(br, bn));
    an.cmp(&bn).then(ai.cmp(&bi)).then_with(|| ar.get(i).cmp(&br.get(i)))
}

/// A scalar as yaml.v3 writes a Go string.
pub fn scalar(s: &str) -> String {
    if s.contains('\n') || !resolves_to_string(s) || has_special(s) {
        return double_quoted(s);
    }
    if plain_allowed(s) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "''"))
    }
}

/// Whether the plain text would read back as a string (yaml.v3's `resolve` plus its 1.1 booleans and base-60 floats).
fn resolves_to_string(s: &str) -> bool {
    const WORDS: &[&str] = &[
        "true", "True", "TRUE", "false", "False", "FALSE", "", "~", "null", "Null", "NULL", ".nan", ".NaN", ".NAN",
        ".inf", ".Inf", ".INF", "+.inf", "+.Inf", "+.INF", "-.inf", "-.Inf", "-.INF", "y", "Y", "yes", "Yes",
        "YES", "on", "On", "ON", "n", "N", "no", "No", "NO", "off", "Off", "OFF",
    ];
    if WORDS.contains(&s) || base60(s) {
        return false;
    }
    match s.as_bytes().first() {
        // Go's ParseFloat on text that starts with '.' takes only the decimal forms the yaml float pattern has.
        Some(b'.') => !yaml_float(s),
        Some(b'+' | b'-' | b'0'..=b'9') => !(timestamp(s) || number(&s.replace('_', ""))),
        _ => true,
    }
}

/// An integer in any base Go's `ParseInt(_, 0, 64)`/`ParseUint` take, or a yaml-style float.
fn number(p: &str) -> bool {
    let unsigned = p.strip_prefix(['+', '-']).unwrap_or(p);
    let radix = |digits: &str, base: u32| !digits.is_empty() && digits.chars().all(|c| c.is_digit(base));
    let int = if let Some(h) = unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X")) {
        radix(h, 16)
    } else if let Some(b) = unsigned.strip_prefix("0b").or_else(|| unsigned.strip_prefix("0B")) {
        radix(b, 2)
    } else if let Some(o) = unsigned.strip_prefix("0o").or_else(|| unsigned.strip_prefix("0O")) {
        radix(o, 8)
    } else if unsigned.len() > 1 && unsigned.starts_with('0') {
        radix(unsigned, 8)
    } else {
        radix(unsigned, 10)
    };
    int || yaml_float(p)
}

/// `^[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?$`
fn yaml_float(p: &str) -> bool {
    let s = p.strip_prefix(['+', '-']).unwrap_or(p);
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(at) => (s.get(..at).unwrap_or(""), Some(s.get(at + 1..).unwrap_or(""))),
        None => (s, None),
    };
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    let mant_ok = match mant.split_once('.') {
        Some(("", frac)) => !frac.is_empty() && digits(frac),
        Some((int, frac)) => !int.is_empty() && digits(int) && digits(frac),
        None => !mant.is_empty() && digits(mant),
    };
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mant_ok && exp_ok
}

/// `^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+(?:\.[0-9_]*)?$`
fn base60(s: &str) -> bool {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (main, frac) = body.split_once('.').unwrap_or((body, ""));
    if !frac.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
        return false;
    }
    let mut parts = main.split(':');
    let head = parts.next().unwrap_or("");
    let head_ok = head.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && head.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    let rest: Vec<&str> = parts.collect();
    let sixty = |p: &&str| match p.as_bytes() {
        [d] => d.is_ascii_digit(),
        [t, d] => (b'0'..=b'5').contains(t) && d.is_ascii_digit(),
        _ => false,
    };
    head_ok && !rest.is_empty() && rest.iter().all(sixty)
}

/// Dates and times yaml.v3 reads as timestamps: four digits and '-' first, then one of its formats.
fn timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 5 || !b.iter().take(4).all(u8::is_ascii_digit) || b.get(4) != Some(&b'-') {
        return false;
    }
    let rest = s.get(5..).unwrap_or("");
    let (date, time) = match rest.find(['T', 't', ' ']) {
        Some(at) => {
            let zoned = rest.get(at..=at) != Some(" ");
            (rest.get(..at).unwrap_or(""), Some((rest.get(at + 1..).unwrap_or(""), zoned)))
        }
        None => (rest, None),
    };
    let short = |t: &str, max: u32| (1..=2).contains(&t.len()) && t.parse::<u32>().is_ok_and(|n| n <= max);
    let date_ok = matches!(date.split_once('-'), Some((m, d)) if short(m, 12) && short(d, 31) && m != "0" && d != "0" && m != "00" && d != "00");
    date_ok && time.is_none_or(|(t, zoned)| time_ok(t, zoned))
}

/// `15:4:5`, optional `.fraction`, then (after 'T') a zone `Z` or `±hh:mm`, or (after a space) nothing.
fn time_ok(t: &str, zoned: bool) -> bool {
    let mut parts = t.splitn(3, ':');
    let (h, m, rest) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let short = |x: &str, max: u32| (1..=2).contains(&x.len()) && x.parse::<u32>().is_ok_and(|n| n <= max);
    let sec_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let mut tail = rest.get(sec_end..).unwrap_or("");
    if let Some(frac) = tail.strip_prefix('.') {
        let n = frac.find(|c: char| !c.is_ascii_digit()).unwrap_or(frac.len());
        tail = frac.get(n..).unwrap_or("");
    }
    let zone_ok = if zoned {
        tail == "Z" || matches!(tail.as_bytes(), [b'+' | b'-', a, b, b':', c, d] if [a, b, c, d].iter().all(|x| x.is_ascii_digit()))
    } else {
        tail.is_empty()
    };
    short(h, 23) && short(m, 59) && short(rest.get(..sec_end).unwrap_or(""), 59) && zone_ok
}

/// Characters yaml.v3 will not write plain or single-quoted: controls, and the few non-printables.
fn has_special(s: &str) -> bool {
    s.chars().any(|c| {
        let u = u32::from(c);
        !(c == '\n' || (0x20..=0x7e).contains(&u) || u == 0x85 || (0xa0..=0xd7ff).contains(&u) || (0xe000..=0xfffd).contains(&u) && u != 0xfeff || (0x10000..=0x10ffff).contains(&u))
    })
}

/// Whether the emitter allows a plain scalar here (block context): no indicator first, no ": " or " #", no leading or
/// trailing space, no ':' last.
fn plain_allowed(s: &str) -> bool {
    let b = s.as_bytes();
    let (Some(first), Some(last)) = (b.first(), b.last()) else {
        return false;
    };
    let follows_blank = |i: usize| b.get(i + 1).is_none_or(|c| *c == b' ');
    let bad_first = match first {
        b'#' | b',' | b'[' | b']' | b'{' | b'}' | b'&' | b'*' | b'!' | b'|' | b'>' | b'\'' | b'"' | b'%' | b'@' | b'`' => true,
        b'-' | b'?' | b':' => follows_blank(0),
        _ => false,
    };
    !bad_first
        && *first != b' '
        && *last != b' '
        && !s.contains(": ")
        && !s.contains(" #")
        && *last != b':'
}

fn double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\x0b' => out.push_str("\\v"),
            '\x0c' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\x1b' => out.push_str("\\e"),
            '\u{85}' => out.push_str("\\N"),
            '\u{a0}' => out.push_str("\\_"),
            '\u{2028}' => out.push_str("\\L"),
            '\u{2029}' => out.push_str("\\P"),
            c if has_special(c.encode_utf8(&mut [0; 4])) => {
                let u = u32::from(c);
                if u <= 0xff {
                    out.push_str(&format!("\\x{u:02X}"));
                } else if u <= 0xffff {
                    out.push_str(&format!("\\u{u:04X}"));
                } else {
                    out.push_str(&format!("\\U{u:08X}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
