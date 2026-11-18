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

/// A JSON number as Confluent writes it back: Jackson reads a number with a fraction or an exponent as a Java
/// `BigDecimal` and writes its `toString` (`1e2` is `1E+2`, `1.50` stays `1.50`, `-0.0` is `0.0`); an integer
/// is written as it is (`-0` is `0`). Anything else (`true`, `null`) is itself.
#[must_use]
pub fn java_number(raw: &str) -> String {
    let (neg, body) = raw.strip_prefix('-').map_or((false, raw), |b| (true, b));
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return raw.to_owned();
    }
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], body[i + 1..].parse::<i64>().unwrap_or(0)),
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
    let scale = i64::try_from(frac.len()).unwrap_or(0) - exp;
    let len = i64::try_from(coeff.len()).unwrap_or(0);
    let adjusted = -scale + (len - 1);
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
