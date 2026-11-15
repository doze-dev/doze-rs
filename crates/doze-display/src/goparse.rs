//! The grammar of Go's `strconv.ParseFloat` (its `readFloat`, `underscoreOK` and `atofHex`), for
//! [`crate::gonum::parse_float`]: which strings are numbers, and the value of a hex one. Decimal values are left to
//! Rust's parser, which rounds as Go's does.

/// A float literal Go accepts, read but not yet rounded.
#[derive(Debug)]
pub(crate) struct Lit {
    pub(crate) hex: bool,
    neg: bool,
    mantissa: u64,
    exp: i64,
    trunc: bool,
}

/// Reads the whole of `s` as a Go float literal; `None` when it is not one.
pub(crate) fn read_float(s: &str) -> Option<Lit> {
    let b = s.as_bytes();
    let mut i = 0;
    let neg = match b.first()? {
        b'+' => {
            i += 1;
            false
        }
        b'-' => {
            i += 1;
            true
        }
        _ => false,
    };
    let hex = b.get(i) == Some(&b'0')
        && b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b'x'))
        && i + 2 < b.len();
    if hex {
        i += 2;
    }
    let digits = mantissa_digits(b, &mut i, hex)?;
    let mut dp = digits.dp;
    let mut nd_mant = digits.nd_mant;
    if hex {
        dp *= 4;
        nd_mant *= 4;
    }
    let exp_char = if hex { b'p' } else { b'e' };
    let mut underscores = digits.underscores;
    if b.get(i).is_some_and(|c| c.to_ascii_lowercase() == exp_char) {
        let (e, in_exp) = exponent(b, &mut i)?;
        dp += e;
        underscores |= in_exp;
    } else if hex {
        return None;
    }
    if i != b.len() || (underscores && !underscore_ok(s)) {
        return None;
    }
    let exp = if digits.mantissa == 0 {
        0
    } else {
        dp - nd_mant
    };
    Some(Lit {
        hex,
        neg,
        mantissa: digits.mantissa,
        exp,
        trunc: digits.trunc,
    })
}

struct Digits {
    mantissa: u64,
    trunc: bool,
    underscores: bool,
    dp: i64,
    nd_mant: i64,
}

fn mantissa_digits(b: &[u8], i: &mut usize, hex: bool) -> Option<Digits> {
    let (base, max_digits) = if hex { (16, 16) } else { (10, 19) };
    let mut d = Digits {
        mantissa: 0,
        trunc: false,
        underscores: false,
        dp: 0,
        nd_mant: 0,
    };
    let (mut saw_dot, mut saw_digits, mut nd) = (false, false, 0_i64);
    while let Some(&c) = b.get(*i) {
        let value = match c {
            b'_' => {
                d.underscores = true;
                *i += 1;
                continue;
            }
            b'.' if !saw_dot => {
                saw_dot = true;
                d.dp = nd;
                *i += 1;
                continue;
            }
            b'0'..=b'9' => u64::from(c - b'0'),
            b'a'..=b'f' | b'A'..=b'F' if hex => u64::from(c.to_ascii_lowercase() - b'a' + 10),
            _ => break,
        };
        saw_digits = true;
        *i += 1;
        if value == 0 && nd == 0 && c == b'0' {
            d.dp -= 1; // a leading zero
            continue;
        }
        nd += 1;
        if d.nd_mant < max_digits {
            d.mantissa = d.mantissa * base + value;
            d.nd_mant += 1;
        } else if c != b'0' {
            d.trunc = true;
        }
    }
    if !saw_dot {
        d.dp = nd;
    }
    saw_digits.then_some(d)
}

/// The exponent after `e` or `p`, signed (Go stops counting at 10,000), and whether it had underscores.
fn exponent(b: &[u8], i: &mut usize) -> Option<(i64, bool)> {
    *i += 1;
    let sign = match b.get(*i)? {
        b'+' => {
            *i += 1;
            1
        }
        b'-' => {
            *i += 1;
            -1
        }
        _ => 1,
    };
    if !b.get(*i)?.is_ascii_digit() {
        return None;
    }
    let (mut e, mut underscores) = (0_i64, false);
    while let Some(&c) = b.get(*i) {
        match c {
            b'_' => underscores = true,
            b'0'..=b'9' if e < 10_000 => e = e * 10 + i64::from(c - b'0'),
            b'0'..=b'9' => {}
            _ => break,
        }
        *i += 1;
    }
    Some((sign * e, underscores))
}

/// Underscores only between digits (a base prefix counts as one), never first or last.
fn underscore_ok(s: &str) -> bool {
    let b = s.strip_prefix(['+', '-']).unwrap_or(s).as_bytes();
    let (mut saw, mut i, mut hex) = (b'^', 0, false);
    if b.len() >= 2
        && b.first() == Some(&b'0')
        && b.get(1)
            .is_some_and(|c| matches!(c.to_ascii_lowercase(), b'b' | b'o' | b'x'))
    {
        i = 2;
        saw = b'0';
        hex = b.get(1).is_some_and(|c| c.eq_ignore_ascii_case(&b'x'));
    }
    for &c in b.iter().skip(i) {
        if c.is_ascii_digit() || (hex && c.is_ascii_hexdigit()) {
            saw = b'0';
        } else if c == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else if saw == b'_' {
            return false;
        } else {
            saw = b'!';
        }
    }
    saw != b'_'
}

const MANT_BITS: u32 = 52;
const BIAS: i64 = -1023;
const MAX_EXP: i64 = (1 << 11) + BIAS - 2;
const MIN_EXP: i64 = BIAS + 1;

/// The value of a hex literal, rounded to even as Go's `atofHex` rounds it; infinite when it is too large.
pub(crate) fn hex_value(lit: &Lit) -> f64 {
    let (mut m, mut exp) = (lit.mantissa, lit.exp + i64::from(MANT_BITS));
    while m != 0 && m >> (MANT_BITS + 2) == 0 {
        m <<= 1;
        exp -= 1;
    }
    if lit.trunc {
        m |= 1;
    }
    while m >> (1 + MANT_BITS + 2) != 0 {
        m = m >> 1 | m & 1;
        exp += 1;
    }
    while m > 1 && exp < MIN_EXP - 2 {
        m = m >> 1 | m & 1;
        exp += 1;
    }
    let mut round = m & 3;
    m >>= 2;
    round |= m & 1;
    exp += 2;
    if round == 3 {
        m += 1;
        if m == 1 << (1 + MANT_BITS) {
            m >>= 1;
            exp += 1;
        }
    }
    if m >> MANT_BITS == 0 {
        exp = BIAS;
    }
    let sign = if lit.neg { -1.0 } else { 1.0 };
    if exp > MAX_EXP {
        return sign * f64::INFINITY;
    }
    let biased = u64::try_from(exp - BIAS).unwrap_or(0) & ((1 << 11) - 1);
    sign * f64::from_bits(m & ((1 << MANT_BITS) - 1) | biased << MANT_BITS)
}
