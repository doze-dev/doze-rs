//! A JSON number as an exact decimal: what the limits of a schema (`minimum`, `maxLength`, `multipleOf`) are compared as.
//! `1`, `1.0` and `1e0` are one value; nothing is rounded through a binary float unless a divisor has more than 36 digits.

use std::cmp::Ordering;

/// `digits` times ten to `exp`, negated when `neg`. `digits` has no leading or trailing zeros; zero has none at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dec {
    neg: bool,
    digits: Vec<u8>,
    exp: i64,
}

/// An exponent past this is clamped: such numbers compare correctly, they are only not exact.
const EXP_BOUND: i64 = 1 << 40;

impl Dec {
    /// Reads a JSON number's text (as the ordered JSON keeps it); anything else is `None`.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (neg, rest) = match text.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, text),
        };
        let (mantissa, exp_text) = match rest.find(['e', 'E']) {
            Some(i) => (rest.get(..i)?, rest.get(i + 1..)?),
            None => (rest, "0"),
        };
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if int.is_empty() || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let exp = parse_exp(exp_text)?;
        let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
        let frac_len = i64::try_from(frac.len()).ok()?;
        let mut exp = exp - frac_len;
        while digits.last() == Some(&0) {
            digits.pop();
            exp += 1;
        }
        let lead = digits.iter().take_while(|d| **d == 0).count();
        digits.drain(..lead);
        if digits.is_empty() {
            return Some(Self { neg: false, digits, exp: 0 });
        }
        Some(Self { neg, digits, exp })
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    /// Whether the value is a whole number.
    pub(crate) fn is_integer(&self) -> bool {
        self.exp >= 0 || self.is_zero()
    }

    pub(crate) fn is_negative(&self) -> bool {
        self.neg
    }

    /// The position of the leading digit: magnitudes compare by it first.
    fn magnitude(&self) -> i64 {
        i64::try_from(self.digits.len()).unwrap_or(i64::MAX) + self.exp
    }

    fn cmp_abs(&self, other: &Self) -> Ordering {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
        self.magnitude()
            .cmp(&other.magnitude())
            .then_with(|| self.digits.cmp(&other.digits))
    }

    /// Whether `self` is a whole multiple of `divisor` (never, when the divisor is zero).
    pub(crate) fn is_multiple_of(&self, divisor: &Self) -> bool {
        if divisor.is_zero() {
            return false;
        }
        if self.is_zero() {
            return true;
        }
        // self/divisor = (S / D) * 10^k; with no trailing zeros in S, a negative k leaves a fraction.
        let k = self.exp - divisor.exp;
        if k < 0 {
            return false;
        }
        match small(&divisor.digits) {
            Some(d) => {
                let mut r = 0_u128;
                for &digit in &self.digits {
                    r = (r * 10 + u128::from(digit)) % d;
                }
                mul_mod(r, pow_mod(10, k.unsigned_abs(), d), d) == 0
            }
            None => {
                let q = self.to_f64() / divisor.to_f64();
                q.is_finite() && q.fract() == 0.0
            }
        }
    }

    fn to_f64(&self) -> f64 {
        let mut text: String = self.digits.iter().map(|d| char::from(b'0' + d)).collect();
        if text.is_empty() {
            return 0.0;
        }
        text.push_str(&format!("e{}", self.exp));
        let v: f64 = text.parse().unwrap_or(f64::NAN);
        if self.neg { -v } else { v }
    }
}

impl PartialOrd for Dec {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Dec {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.neg, other.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.cmp_abs(other),
            (true, true) => other.cmp_abs(self),
        }
    }
}

fn parse_exp(text: &str) -> Option<i64> {
    let (neg, digits) = match text.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: i64 = 0;
    for b in digits.bytes() {
        v = (v * 10 + i64::from(b - b'0')).min(EXP_BOUND);
    }
    Some(if neg { -v } else { v })
}

/// The digits as a number, when it has at most 36 of them (so that ten times a remainder still fits).
fn small(digits: &[u8]) -> Option<u128> {
    (digits.len() <= 36).then(|| digits.iter().fold(0_u128, |a, d| a * 10 + u128::from(*d)))
}

fn mul_mod(a: u128, b: u128, m: u128) -> u128 {
    // a, b < m <= 10^36 < 2^120: doubling stays below 2^121.
    let (mut a, mut b, mut out) = (a % m, b, 0_u128);
    while b > 0 {
        if b & 1 == 1 {
            out = (out + a) % m;
        }
        a = (a * 2) % m;
        b >>= 1;
    }
    out
}

fn pow_mod(base: u128, mut exp: u64, m: u128) -> u128 {
    let (mut base, mut out) = (base % m, 1 % m);
    while exp > 0 {
        if exp & 1 == 1 {
            out = mul_mod(out, base, m);
        }
        base = mul_mod(base, base, m);
        exp >>= 1;
    }
    out
}
