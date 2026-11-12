//! DynamoDB's number: a decimal of up to 38 significant digits with a decimal exponent between -130 and 125, exact (never a float).
//! Held as sign, digits and a power of ten, normalised so that equal values are equal bytes.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Most significant digits a number may hold.
pub const MAX_DIGITS: usize = 38;
/// Largest power of ten of the leading digit.
const MAX_EXP: i32 = 125;
/// Smallest power of ten of the leading digit.
const MIN_EXP: i32 = -130;

/// Why text is not a number DynamoDB can store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberError {
    /// Not a decimal number at all.
    NotNumeric,
    /// Larger than 9.9999999999999999999999999999999999999E+125.
    Overflow,
    /// Smaller in magnitude than 1E-130.
    Underflow,
    /// More than 38 significant digits.
    TooPrecise,
}

impl NumberError {
    /// AWS's words for it, given the text that was refused. `(memory)` for the precision text; the others are the documented
    /// ones.
    #[must_use]
    pub fn message(self, text: &str) -> String {
        match self {
            Self::NotNumeric => format!("The parameter cannot be converted to a numeric value: {text}"),
            Self::Overflow => "Number overflow. Attempting to store a number with magnitude larger than supported range".to_owned(),
            Self::Underflow => "Number underflow. Attempting to store a number with magnitude smaller than supported range".to_owned(),
            Self::TooPrecise => "Attempting to store more than 38 significant digits in a Number".to_owned(),
        }
    }
}

/// An exact decimal. Value is `digits` (most significant first, no leading or trailing zero) times ten to the `exp`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Number {
    neg: bool,
    digits: Vec<u8>,
    exp: i32,
}

impl Number {
    /// Zero.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            neg: false,
            digits: Vec::new(),
            exp: 0,
        }
    }

    /// An integer.
    #[must_use]
    pub fn from_i64(n: i64) -> Self {
        let text = n.unsigned_abs().to_string();
        let digits = text.bytes().map(|b| b - b'0').collect();
        Self {
            neg: n < 0,
            digits,
            exp: 0,
        }
        .normal()
    }

    /// Reads DynamoDB's number text: optional sign, digits with an optional point, optional exponent.
    ///
    /// # Errors
    /// [`NumberError`] for text that is not a number or is out of DynamoDB's range.
    pub fn parse(text: &str) -> Result<Self, NumberError> {
        let (neg, rest) = match text.as_bytes().first() {
            Some(b'-') => (true, text.get(1..).unwrap_or_default()),
            Some(b'+') => (false, text.get(1..).unwrap_or_default()),
            _ => (false, text),
        };
        let (mantissa, exponent) = match rest.find(['e', 'E']) {
            Some(at) => (
                rest.get(..at).unwrap_or_default(),
                Some(rest.get(at + 1..).unwrap_or_default()),
            ),
            None => (rest, None),
        };
        let (whole, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if (whole.is_empty() && frac.is_empty()) || !all_digits(whole) || !all_digits(frac) {
            return Err(NumberError::NotNumeric);
        }
        let mut exp = match exponent {
            None => 0_i64,
            Some(e) => parse_exponent(e)?,
        };
        exp -= i64::try_from(frac.len()).map_err(|_| NumberError::NotNumeric)?;
        let digits = whole
            .bytes()
            .chain(frac.bytes())
            .map(|b| b - b'0')
            .collect();
        let n = Self {
            neg,
            digits,
            exp: i32::try_from(exp.clamp(-100_000, 100_000)).unwrap_or(0),
        }
        .normal();
        if n.digits.len() > MAX_DIGITS {
            return Err(NumberError::TooPrecise);
        }
        n.check_range()
    }

    fn check_range(self) -> Result<Self, NumberError> {
        if self.is_zero() {
            return Ok(self);
        }
        match self.lead_exp() {
            e if e > MAX_EXP => Err(NumberError::Overflow),
            e if e < MIN_EXP => Err(NumberError::Underflow),
            _ => Ok(self),
        }
    }

    /// Strips leading and trailing zeros; zero has no sign.
    fn normal(mut self) -> Self {
        let lead = self.digits.iter().take_while(|d| **d == 0).count();
        self.digits.drain(..lead);
        while self.digits.last() == Some(&0) {
            self.digits.pop();
            self.exp = self.exp.saturating_add(1);
        }
        if self.digits.is_empty() {
            return Self::zero();
        }
        self
    }

    /// Whether it is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    /// The power of ten of the leading digit.
    fn lead_exp(&self) -> i32 {
        let len = i32::try_from(self.digits.len()).unwrap_or(i32::MAX);
        self.exp.saturating_add(len - 1)
    }

    /// The whole number below or at the value, when it fits an `i64` (the time-to-live attribute is read this way).
    #[must_use]
    pub fn floor_i64(&self) -> Option<i64> {
        let mut whole: i128 = 0;
        let len = i32::try_from(self.digits.len()).ok()?;
        for (at, d) in self.digits.iter().enumerate() {
            let place = self.exp.checked_add(len - 1 - i32::try_from(at).ok()?)?;
            if place < 0 {
                break;
            }
            whole = whole.checked_add(
                i128::from(*d).checked_mul(10_i128.checked_pow(u32::try_from(place).ok()?)?)?,
            )?;
        }
        let fractional = self.exp < 0
            && self
                .digits
                .iter()
                .rev()
                .take(usize::try_from(-self.exp).unwrap_or(usize::MAX))
                .any(|d| *d != 0);
        if self.neg {
            whole = -whole - i128::from(fractional);
        }
        i64::try_from(whole).ok()
    }

    /// The bytes DynamoDB counts for it toward an item's size: about one per two digits, plus one (and one for a negative).
    #[must_use]
    pub fn size(&self) -> usize {
        self.digits.len().div_ceil(2) + 1 + usize::from(self.neg)
    }

    /// `self + other`.
    ///
    /// # Errors
    /// [`NumberError`] when the sum is out of range.
    pub fn add(&self, other: &Self) -> Result<Self, NumberError> {
        self.combine(other, false)
    }

    /// `self - other`.
    ///
    /// # Errors
    /// [`NumberError`] when the difference is out of range.
    pub fn sub(&self, other: &Self) -> Result<Self, NumberError> {
        self.combine(other, true)
    }

    fn combine(&self, other: &Self, subtract: bool) -> Result<Self, NumberError> {
        let other_neg = other.neg != subtract && !other.is_zero();
        let low = self.exp.min(other.exp);
        let a = self.scaled(low);
        let b = other.scaled(low);
        let (neg, mag) = if self.neg == other_neg {
            (self.neg, add_mag(&a, &b))
        } else {
            match cmp_mag(&a, &b) {
                Ordering::Less => (other_neg, sub_mag(&b, &a)),
                _ => (self.neg, sub_mag(&a, &b)),
            }
        };
        let mut n = Self {
            neg,
            digits: mag,
            exp: low,
        }
        .normal();
        n.round_to_precision();
        n.check_range()
    }

    /// The digits as an integer scaled down to the power of ten `low`.
    fn scaled(&self, low: i32) -> Vec<u8> {
        let shift = usize::try_from(self.exp.saturating_sub(low)).unwrap_or(0);
        let mut d = self.digits.clone();
        d.resize(d.len() + shift, 0);
        d
    }

    /// Keeps 38 digits, rounding half up.
    fn round_to_precision(&mut self) {
        if self.digits.len() <= MAX_DIGITS {
            return;
        }
        let drop = self.digits.len() - MAX_DIGITS;
        let up = self.digits.get(MAX_DIGITS).is_some_and(|d| *d >= 5);
        self.digits.truncate(MAX_DIGITS);
        self.exp = self.exp.saturating_add(i32::try_from(drop).unwrap_or(0));
        if up {
            let mut carry = true;
            for d in self.digits.iter_mut().rev() {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                self.digits.insert(0, 1);
                self.digits.truncate(MAX_DIGITS);
                self.exp = self.exp.saturating_add(1);
            }
        }
        *self = std::mem::replace(self, Self::zero()).normal();
    }

    /// Bytes whose byte order is the numeric order, for sort keys: a class byte (negative, zero, positive), the power of ten and
    /// the digits, closed by a byte below any digit. A negative number's bytes are complemented.
    pub fn order_key(&self, out: &mut Vec<u8>) {
        if self.is_zero() {
            out.push(0x80);
            return;
        }
        let biased =
            u16::try_from(i32::from(i16::MAX) + 1 + self.lead_exp().clamp(-30_000, 30_000))
                .unwrap_or(0);
        let mut body = Vec::with_capacity(self.digits.len() + 3);
        body.extend_from_slice(&biased.to_be_bytes());
        body.extend(self.digits.iter().map(|d| d + 1));
        body.push(0);
        if self.neg {
            out.push(0x40);
            out.extend(body.iter().map(|b| !b));
        } else {
            out.push(0xC0);
            out.extend(body);
        }
    }
}

fn parse_exponent(e: &str) -> Result<i64, NumberError> {
    let (neg, digits) = match e.as_bytes().first() {
        Some(b'-') => (true, e.get(1..).unwrap_or_default()),
        Some(b'+') => (false, e.get(1..).unwrap_or_default()),
        _ => (false, e),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(NumberError::NotNumeric);
    }
    let n: i64 = digits.parse().unwrap_or(1_000_000).min(1_000_000);
    Ok(if neg { -n } else { n })
}

fn cmp_mag(a: &[u8], b: &[u8]) -> Ordering {
    let a = trim(a);
    let b = trim(b);
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

fn trim(d: &[u8]) -> &[u8] {
    let lead = d.iter().take_while(|x| **x == 0).count();
    d.get(lead..).unwrap_or_default()
}

fn add_mag(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(a.len().max(b.len()) + 1);
    let mut carry = 0;
    let (mut ia, mut ib) = (a.iter().rev(), b.iter().rev());
    loop {
        let (x, y) = (ia.next(), ib.next());
        if x.is_none() && y.is_none() {
            break;
        }
        let s = x.copied().unwrap_or(0) + y.copied().unwrap_or(0) + carry;
        out.push(s % 10);
        carry = s / 10;
    }
    if carry > 0 {
        out.push(carry);
    }
    out.reverse();
    out
}

/// `a - b` for `a >= b`.
fn sub_mag(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0_i8;
    let mut ib = b.iter().rev();
    for x in a.iter().rev() {
        let mut d = i8::try_from(*x).unwrap_or(0)
            - borrow
            - i8::try_from(ib.next().copied().unwrap_or(0)).unwrap_or(0);
        if d < 0 {
            d += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(u8::try_from(d).unwrap_or(0));
    }
    out.reverse();
    out
}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => {
                return if other.neg {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
            }
            (false, true) => {
                return if self.neg {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            (false, false) => {}
        }
        if self.neg != other.neg {
            return if self.neg {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let magnitude = self
            .lead_exp()
            .cmp(&other.lead_exp())
            .then_with(|| self.digits.cmp(&other.digits));
        if self.neg {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
}

impl fmt::Display for Number {
    /// Plain notation, or scientific outside a modest range. `(memory)`: AWS's choice of notation is not recorded.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        if self.neg {
            f.write_str("-")?;
        }
        let digits: String = self.digits.iter().map(|d| char::from(b'0' + d)).collect();
        let lead = self.lead_exp();
        if !(-6..=37).contains(&lead) {
            let (first, rest) = digits.split_at(1);
            f.write_str(first)?;
            if !rest.is_empty() {
                write!(f, ".{rest}")?;
            }
            return write!(f, "E{}{}", if lead < 0 { "-" } else { "+" }, lead.abs());
        }
        if self.exp >= 0 {
            let zeros = usize::try_from(self.exp).unwrap_or(0);
            return write!(f, "{digits}{}", "0".repeat(zeros));
        }
        let point = usize::try_from(-self.exp).unwrap_or(0);
        if digits.len() > point {
            let (whole, frac) = digits.split_at(digits.len() - point);
            write!(f, "{whole}.{frac}")
        } else {
            write!(f, "0.{}{digits}", "0".repeat(point - digits.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> Number {
        Number::parse(s).unwrap()
    }

    #[test]
    fn text_is_normalised() {
        assert_eq!(n("1.50").to_string(), "1.5");
        assert_eq!(n("0001").to_string(), "1");
        assert_eq!(n("-0").to_string(), "0");
        assert_eq!(n("1e3").to_string(), "1000");
        assert_eq!(n("+.5").to_string(), "0.5");
        assert_eq!(n("1E-7").to_string(), "1E-7");
        assert_eq!(
            n("12345678901234567890").to_string(),
            "12345678901234567890"
        );
    }

    #[test]
    fn bad_text_and_limits() {
        for bad in ["", "abc", "1.2.3", "--1", "1e", ".", "1e+x"] {
            assert_eq!(Number::parse(bad), Err(NumberError::NotNumeric), "{bad}");
        }
        assert_eq!(Number::parse("1E126"), Err(NumberError::Overflow));
        assert_eq!(Number::parse("1E-131"), Err(NumberError::Underflow));
        assert!(Number::parse("1E125").is_ok() && Number::parse("1E-130").is_ok());
        assert_eq!(Number::parse(&"1".repeat(39)), Err(NumberError::TooPrecise));
        assert!(Number::parse(&"9".repeat(38)).is_ok());
    }

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(n("0.1").add(&n("0.2")).unwrap(), n("0.3"));
        assert_eq!(n("1").sub(&n("3")).unwrap(), n("-2"));
        assert_eq!(n("-5").add(&n("5")).unwrap(), Number::zero());
        assert_eq!(n("99").add(&n("1")).unwrap(), n("100"));
        assert_eq!(n("1E125").add(&n("9E125")), Err(NumberError::Overflow));
        assert_eq!(n("2.5").sub(&n("-1.5")).unwrap(), n("4"));
    }

    #[test]
    fn order_is_numeric_in_values_and_in_key_bytes() {
        let list = [
            "-100", "-2.5", "-2", "-0.001", "0", "0.001", "1", "1.5", "2", "10", "100", "1E20",
        ];
        let keys: Vec<Vec<u8>> = list
            .iter()
            .map(|s| {
                let mut k = Vec::new();
                n(s).order_key(&mut k);
                k
            })
            .collect();
        for w in keys.windows(2) {
            assert!(w[0] < w[1]);
        }
        for w in list.windows(2) {
            assert!(n(w[0]) < n(w[1]), "{} < {}", w[0], w[1]);
        }
    }

    #[test]
    fn whole_part_and_size() {
        assert_eq!(n("1700000000").floor_i64(), Some(1_700_000_000));
        assert_eq!(n("1.9").floor_i64(), Some(1));
        assert_eq!(n("-1.5").floor_i64(), Some(-2));
        assert_eq!(n("12").size(), 2);
        assert_eq!(n("-123").size(), 4);
    }
}
