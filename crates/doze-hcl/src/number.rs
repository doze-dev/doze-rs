//! cty's number: a binary float that carries its own precision (512 bits for anything parsed from
//! text, 64 for host integers, 53 for host floats), with go-cty's operations on top of it. Printing is
//! the shortest decimal that reads back to the same bits at that precision (`Text('f', -1)`), so
//! `0.1 + 0.2` prints `0.3` and `1 / 3` prints 155 digits, as Go does.

mod decimal;
mod float;
mod parse;

use float::{Float, NaN};
use num_bigint::BigUint;
use std::cmp::Ordering;
use std::fmt;

/// The precision of a number read from text (cty's `ParseNumberVal`).
pub const PARSE_PREC: u32 = 512;

#[derive(Clone, PartialEq, Eq)]
pub struct Number(Float);

/// Why an arithmetic operation has no result (Go's `big.ErrNaN`, reported by cty with these words).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithError {
    OpposingInfinities,
    InfinityMinusItself,
    ZeroTimesInfinity,
    ZeroOverZero,
    ModuloInfinity,
}

impl fmt::Display for ArithError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ArithError::OpposingInfinities => "can't compute sum of opposing infinities",
            ArithError::InfinityMinusItself => "can't subtract infinity from itself",
            ArithError::ZeroTimesInfinity => "can't multiply zero by infinity",
            ArithError::ZeroOverZero => "can't divide zero by zero or infinity by infinity",
            ArithError::ModuloInfinity => "can't use modulo with zero and infinity",
        })
    }
}

impl Number {
    /// cty's `ParseNumberVal`: `None` where it says "a number is required".
    pub fn parse(s: &str) -> Option<Number> {
        parse::parse(s, PARSE_PREC).map(Number)
    }

    /// A host integer (cty's `NumberIntVal`: 64 bits of precision, exact).
    pub fn from_i64(v: i64) -> Number {
        let mant = BigUint::from(v.unsigned_abs());
        Number(Float::from_parts(v < 0, mant, 0, 64))
    }

    /// A host unsigned integer (cty's `NumberUIntVal`).
    pub fn from_u64(v: u64) -> Number {
        Number(Float::from_parts(false, BigUint::from(v), 0, 64))
    }

    pub fn from_usize(v: usize) -> Number {
        Number::from_u64(u64::try_from(v).unwrap_or(u64::MAX))
    }

    /// A host float (cty's `NumberFloatVal`: 53 bits). `None` for NaN, which cty refuses.
    pub fn from_f64(v: f64) -> Option<Number> {
        if v.is_nan() {
            return None;
        }
        let neg = v.is_sign_negative();
        if v.is_infinite() {
            return Some(Number(Float::inf(neg, 53)));
        }
        if v == 0.0 {
            return Some(Number(Float::zero(neg, 53)));
        }
        let bits = v.abs().to_bits();
        let exp_bits = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
        let frac = bits & ((1u64 << 52) - 1);
        let (mant, exp) = if exp_bits == 0 {
            (frac, -1074)
        } else {
            (frac | (1u64 << 52), exp_bits - 1075)
        };
        Some(Number(Float::from_parts(neg, BigUint::from(mant), exp, 53)))
    }

    pub fn is_inf(&self) -> bool {
        self.0.is_inf()
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// -1, 0 or 1.
    pub fn signum(&self) -> i8 {
        self.0.sign()
    }

    pub fn is_negative(&self) -> bool {
        self.0.neg
    }

    fn prec2(&self, other: &Number) -> u32 {
        self.0.prec.max(other.0.prec)
    }

    pub fn add(&self, other: &Number) -> Result<Number, ArithError> {
        let p = self.prec2(other);
        self.0
            .add(&other.0, p)
            .map(Number)
            .map_err(|NaN| ArithError::OpposingInfinities)
    }

    pub fn sub(&self, other: &Number) -> Result<Number, ArithError> {
        let p = self.prec2(other);
        self.0
            .sub(&other.0, p)
            .map(Number)
            .map_err(|NaN| ArithError::InfinityMinusItself)
    }

    /// cty's `Multiply` differs from the other operations: the product is rounded to 512 bits and keeps
    /// every bit it needs, so the product of two 64-bit host integers stays exact.
    pub fn mul(&self, other: &Number) -> Result<Number, ArithError> {
        let r = self
            .0
            .mul(&other.0, PARSE_PREC)
            .map_err(|NaN| ArithError::ZeroTimesInfinity)?;
        let prec = self.prec2(other).max(r.min_prec());
        Ok(Number(r.keep_prec(prec)))
    }

    pub fn div(&self, other: &Number) -> Result<Number, ArithError> {
        let p = self.prec2(other);
        self.0
            .quo(&other.0, p)
            .map(Number)
            .map_err(|NaN| ArithError::ZeroOverZero)
    }

    /// cty's `Modulo`: `x % 0` is `x`; otherwise `x - trunc(x / y) × y`, the last two steps at `x`'s
    /// precision. Any infinity is an error (in Go one panics inside `big.Float`, which cty reports).
    pub fn modulo(&self, other: &Number) -> Result<Number, ArithError> {
        if other.is_zero() {
            return Ok(self.clone());
        }
        let rat = self.div(other).map_err(|_| ArithError::ModuloInfinity)?;
        let Some((neg, int, _)) = rat.0.trunc() else {
            return Err(ArithError::ModuloInfinity);
        };
        // Go goes through a big.Int, whose zero has no sign.
        let neg = neg && int != BigUint::default();
        let p = self.0.prec;
        let work = Float::from_parts(neg, int, 0, p);
        let work = other
            .0
            .mul(&work, p)
            .map_err(|NaN| ArithError::ModuloInfinity)?;
        self.0
            .sub(&work, p)
            .map(Number)
            .map_err(|NaN| ArithError::ModuloInfinity)
    }

    pub fn neg(&self) -> Number {
        Number(self.0.neg())
    }

    pub fn abs(&self) -> Number {
        Number(self.0.abs())
    }

    /// Orders by value (-0 equals 0).
    pub fn cmp_value(&self, other: &Number) -> Ordering {
        self.0.cmp(&other.0)
    }

    /// cty's number equality: integers compare exactly, others by their printed form, so two values
    /// that print alike are equal whatever their precision.
    pub fn same(&self, other: &Number) -> bool {
        if self.signum() != other.signum() {
            return false;
        }
        match (self.0.trunc(), other.0.trunc()) {
            (Some((_, a, true)), Some((_, b, true))) => a == b,
            (Some((_, _, true)), _) | (_, Some((_, _, true))) => false,
            _ => {
                let norm = |s: String| if s == "-0" { "0".to_owned() } else { s };
                norm(self.text()) == norm(other.text())
            }
        }
    }

    /// The value as an exact integer, if it is one.
    pub fn to_i64(&self) -> Option<i64> {
        let (neg, int, exact) = self.0.trunc()?;
        if !exact {
            return None;
        }
        let v = i128::try_from(int).ok()?;
        i64::try_from(if neg { -v } else { v }).ok()
    }

    /// Whether the value is an integer (no fractional part, not infinite).
    pub fn is_integer(&self) -> bool {
        self.0.trunc().is_some_and(|(_, _, exact)| exact)
    }

    /// The integer part toward zero: sign and magnitude. `None` for an infinity.
    pub fn trunc(&self) -> Option<(bool, BigUint)> {
        self.0.trunc().map(|(neg, int, _)| (neg, int))
    }

    /// Go's `Text('f', -1)`: what cty prints and converts to a string.
    pub fn text(&self) -> String {
        decimal::text_f(&self.0)
    }

    /// Go's `big.Float.String()` (`Text('g', 10)`), which cty's set ordering hashes.
    pub fn text_g10(&self) -> String {
        decimal::text_g10(&self.0)
    }

    /// The precision this value keeps, in bits.
    pub fn precision(&self) -> u32 {
        self.0.prec
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Number({}, {} bits)", self.text(), self.0.prec)
    }
}

#[cfg(test)]
mod tests;
