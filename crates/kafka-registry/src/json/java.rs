//! How Confluent's Java writes what the registry reads: a `HashMap`'s iteration order, and a `BigDecimal`'s text.

/// The order a `java.util.HashMap` built from these names iterates them in: by bucket (`String.hashCode`, spread,
/// masked by a capacity that grows from 16 at three quarters full), and within a bucket in the order they were put in,
/// taken here to be by name.
pub(crate) fn java_hash_order(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    java_hash_inserted(names)
}

/// [`java_hash_order`] for names inserted in the order given.
pub(crate) fn java_hash_inserted(mut names: Vec<String>) -> Vec<String> {
    let mut capacity: u32 = 16;
    while names.len() > (capacity as usize) * 3 / 4 {
        capacity = capacity.saturating_mul(2);
    }
    let bucket = |s: &String| {
        let h = s
            .encode_utf16()
            .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
        let u = h.cast_unsigned();
        (u ^ (u >> 16)) & (capacity - 1)
    };
    names.sort_by_key(bucket);
    names
}

/// What Confluent's Jackson (cp-schema-registry 7.7.1: jackson-databind 2.16, Java 17) makes of a JSON Schema's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaRead {
    /// No fraction and no exponent: an `int`, a `long` or a `BigInteger`, as written.
    Integer,
    /// A `BigDecimal`.
    Decimal,
    /// A `BigDecimal` cannot hold it (its exponent, or its scale, is past a 32-bit `int`), so Jackson keeps the
    /// `double`, and the `double` is zero: stored as `0.0`, or `-0.0` when negative (recorded by tools/registry-diff,
    /// `json-schema-huge-numbers`).
    Zero { neg: bool },
    /// As [`JavaRead::Zero`], but the `double` is infinite: a schema holding one is refused (`Invalid JSON Schema`).
    Infinite { neg: bool },
}

/// How Jackson reads a number's text, by `java.math.BigDecimal(String)`'s rules on Java 17: the exponent's digits,
/// less leading zeros, are at most ten and fit an `int`, and the scale (the fraction's digits less the exponent) fits
/// an `int`; otherwise the number is a `double`. `None` for text that is not a number.
#[must_use]
pub fn java_reads(raw: &str) -> Option<JavaRead> {
    let (neg, body) = raw.strip_prefix('-').map_or((false, raw), |b| (true, b));
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if !body.contains(['.', 'e', 'E']) {
        return Some(JavaRead::Integer);
    }
    let (mantissa, exp_text) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let frac = mantissa.split_once('.').map_or("", |(_, f)| f);
    let exp = match exp_text {
        None => Some(0_i64),
        Some(t) => {
            let (eneg, digits) = match t.strip_prefix('-') {
                Some(d) => (true, d),
                None => (false, t.strip_prefix('+').unwrap_or(t)),
            };
            // BigDecimal skips leading zeros only while more than ten digits are left.
            let mut d = digits;
            while d.len() > 10 && d.starts_with('0') {
                d = &d[1..];
            }
            (d.len() <= 10)
                .then(|| d.parse::<i64>().ok())
                .flatten()
                .map(|v| if eneg { -v } else { v })
                .filter(|&v| i32::try_from(v).is_ok())
        }
    };
    let fits = exp.is_some_and(|e| {
        i64::try_from(frac.len())
            .ok()
            .and_then(|f| f.checked_sub(e))
            .is_some_and(|scale| i32::try_from(scale).is_ok())
    });
    if fits {
        return Some(JavaRead::Decimal);
    }
    let double: f64 = raw.parse().ok()?;
    Some(if double.is_infinite() {
        JavaRead::Infinite { neg }
    } else if double == 0.0 {
        JavaRead::Zero { neg }
    } else {
        // Only a number with some two billion digits gets here.
        JavaRead::Decimal
    })
}

/// A JSON number as Confluent writes it back: Jackson reads a number with a fraction or an exponent as a Java
/// `BigDecimal` and writes its `toString` (`1e2` is `1E+2`, `1.50` stays `1.50`, `-0.0` is `0.0`); an integer
/// is written as it is (`-0` is `0`). One a `BigDecimal` cannot hold is the `double` Jackson keeps instead
/// ([`java_reads`]): `0.0` or `-0.0`, and an infinite one is kept as it was sent (the schema is refused). Anything else
/// (`true`, `null`) is itself.
#[must_use]
pub fn java_number(raw: &str) -> String {
    let (neg, body) = raw.strip_prefix('-').map_or((false, raw), |b| (true, b));
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return raw.to_owned();
    }
    match java_reads(raw) {
        Some(JavaRead::Zero { neg: true }) => return "-0.0".to_owned(),
        Some(JavaRead::Zero { neg: false }) => return "0.0".to_owned(),
        Some(JavaRead::Infinite { .. }) => return raw.to_owned(),
        _ => {}
    }
    // An exponent or scale too large to hold is kept as it was sent: rewriting it would change the number (fuzz
    // json_schema once found `0.5e+000011999153157185615783` stored as `0.5`).
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => match body[i + 1..].parse::<i64>() {
            Ok(e) => (&body[..i], e),
            Err(_) => return raw.to_owned(),
        },
        None => (body, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if frac.is_empty() && !mantissa.contains('.') && !body.contains(['e', 'E']) {
        let digits = int.trim_start_matches('0');
        return if digits.is_empty() {
            "0".to_owned()
        } else {
            raw.to_owned()
        };
    }
    let all: String = format!("{int}{frac}");
    let coeff = all.trim_start_matches('0');
    let coeff = if coeff.is_empty() { "0" } else { coeff };
    let len = i64::try_from(coeff.len()).unwrap_or(0);
    let Some((scale, adjusted)) = i64::try_from(frac.len())
        .ok()
        .and_then(|f| f.checked_sub(exp))
        .and_then(|scale| Some((scale, (len - 1).checked_sub(scale)?)))
    else {
        return raw.to_owned();
    };
    let sign = if neg && coeff != "0" { "-" } else { "" };
    let text = if scale >= 0 && adjusted >= -6 {
        if scale == 0 {
            coeff.to_owned()
        } else if len > scale {
            let cut = usize::try_from(len - scale).unwrap_or(0);
            format!("{}.{}", &coeff[..cut], &coeff[cut..])
        } else {
            let zeros = "0".repeat(usize::try_from(scale - len).unwrap_or(0));
            format!("0.{zeros}{coeff}")
        }
    } else {
        let rest = if coeff.len() > 1 {
            format!(".{}", &coeff[1..])
        } else {
            String::new()
        };
        let e = if adjusted >= 0 {
            format!("+{adjusted}")
        } else {
            adjusted.to_string()
        };
        format!("{}{rest}E{e}", &coeff[..1])
    };
    format!("{sign}{text}")
}
