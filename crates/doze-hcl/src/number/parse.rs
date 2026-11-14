//! Decimal text to a number, by Go's `big.Float` scan.

use super::float::Float;
use num_bigint::BigUint;
use num_traits::Zero;

/// Go's `big.ParseFloat(s, 10, prec, ToNearestEven)`: an optional sign, decimal digits with an
/// optional point, an optional `e` (decimal) or `p` (binary) exponent; or `Inf`/`inf` with an
/// optional sign. `None` where Go reports an error.
pub(crate) fn parse(s: &str, prec: u32) -> Option<Float> {
    let (neg, body) = match s.as_bytes().first() {
        Some(b'+') => (false, s.get(1..)?),
        Some(b'-') => (true, s.get(1..)?),
        _ => (false, s),
    };
    if body == "Inf" || body == "inf" {
        return (s.len() == 3 || s.len() == 4).then(|| Float::inf(neg, prec));
    }
    let scan = Scan::of(body)?;
    if scan.digits.is_zero() {
        return Some(Float::zero(neg, prec));
    }
    let exp2 = scan.exp2;
    // Go refuses an exponent outside int32 outright ("exponent overflow").
    let top = exp2.checked_add(i64::try_from(scan.digits.bits()).ok()?)?;
    if top > i64::from(i32::MAX) || top < i64::from(i32::MIN) {
        return None;
    }
    let base = Float::from_parts(
        neg,
        scan.digits,
        exp2,
        if scan.exp5 == 0 { prec } else { u32::MAX },
    );
    if scan.exp5 == 0 {
        return Some(base);
    }
    let p = pow5(scan.exp5.unsigned_abs(), prec + 64);
    let r = if scan.exp5 < 0 {
        base.quo(&p, prec)
    } else {
        base.mul(&p, prec)
    };
    r.ok()
}

/// What `scan` read: the digits as an integer, and the exponents of 2 and 5 that scale it.
struct Scan {
    digits: BigUint,
    exp2: i64,
    exp5: i64,
}

impl Scan {
    fn of(body: &str) -> Option<Scan> {
        let bytes = body.as_bytes();
        let mut i = 0;
        let mut digits = Vec::new();
        let mut frac: i64 = 0;
        let mut seen_point = false;
        while let Some(&c) = bytes.get(i) {
            match c {
                b'0'..=b'9' => {
                    digits.push(c);
                    if seen_point {
                        frac += 1;
                    }
                }
                b'.' if !seen_point => seen_point = true,
                _ => break,
            }
            i += 1;
        }
        if digits.is_empty() {
            return None;
        }
        let (exp, binary) = exponent(bytes.get(i..)?)?;
        let n = BigUint::parse_bytes(&digits, 10)?;
        let (exp5, exp2) = if binary {
            (-frac, exp - frac)
        } else {
            (exp - frac, exp - frac)
        };
        Some(Scan {
            digits: n,
            exp2,
            exp5,
        })
    }
}

/// The exponent part: empty, or `e`/`E`/`p`/`P`, a sign, digits, and nothing after. Returns the
/// exponent and whether it is binary.
fn exponent(rest: &[u8]) -> Option<(i64, bool)> {
    let Some((&c, tail)) = rest.split_first() else {
        return Some((0, false));
    };
    let binary = match c {
        b'e' | b'E' => false,
        b'p' | b'P' => true,
        _ => return None,
    };
    let (neg, tail) = match tail.split_first() {
        Some((b'+', t)) => (false, t),
        Some((b'-', t)) => (true, t),
        _ => (false, tail),
    };
    if tail.is_empty() || !tail.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut v: i64 = 0;
    for &d in tail {
        v = v.checked_mul(10)?.checked_add(i64::from(d - b'0'))?;
    }
    Some((if neg { -v } else { v }, binary))
}

/// 5^n at `prec` bits, as Go's `pow5`: exact from a table up to 5^27, then by squaring, every
/// product rounded.
fn pow5(n: u64, prec: u32) -> Float {
    // Go squares `f` at 64 more bits than the product it accumulates into.
    const TABLE_MAX: u64 = 27;
    let five = |k: u64| BigUint::from(5u8).pow(u32::try_from(k).unwrap_or(0));
    if n <= TABLE_MAX {
        return Float::from_parts(false, five(n), 0, prec);
    }
    let mut z = Float::from_parts(false, five(TABLE_MAX), 0, prec);
    let fprec = prec.saturating_add(64);
    let mut f = Float::from_parts(false, BigUint::from(5u8), 0, fprec);
    let mut n = n - TABLE_MAX;
    while n > 0 {
        if n & 1 != 0 {
            z = z.mul(&f, prec).unwrap_or_else(|_| Float::inf(false, prec));
        }
        f = f.mul(&f, fprec).unwrap_or_else(|_| Float::inf(false, prec));
        n >>= 1;
    }
    z
}
