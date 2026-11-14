//! A binary floating-point number with a precision per value, as Go's `big.Float` is: signed zero,
//! signed infinity, no NaN, and every result rounded once, to nearest with ties to even.
//!
//! A finite value is `mant × 2^exp` with `mant` odd (trailing zero bits are moved into `exp`), so two
//! equal values have equal parts.

use num_bigint::BigUint;
use num_traits::{One, Zero};
use std::cmp::Ordering;

/// The largest and smallest binary exponent (Go's `x = 0.mant × 2^exp`, so the top bit's position plus
/// one). Go allows the full `int32` range; doze stops at ±2^20 (about 10^±315,000): beyond that Go
/// itself takes minutes to print a number, and no configuration needs one. Past the limit a result
/// overflows to ±Inf or underflows to ±0, as Go's does at its own limit.
pub(crate) const MAX_EXP: i64 = 1 << 17;
pub(crate) const MIN_EXP: i64 = -(1 << 17);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Form {
    Zero,
    Inf,
    /// `mant × 2^exp`, `mant` odd.
    Finite {
        mant: BigUint,
        exp: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Float {
    pub(crate) neg: bool,
    pub(crate) form: Form,
    /// Bits of mantissa this value keeps; results of an operation take the larger of the operands'.
    pub(crate) prec: u32,
}

/// An operation with no defined result (big.Float panics with `ErrNaN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NaN;

fn bits(m: &BigUint) -> i64 {
    i64::try_from(m.bits()).unwrap_or(i64::MAX)
}

impl Float {
    pub(crate) fn zero(neg: bool, prec: u32) -> Float {
        Float {
            neg,
            form: Form::Zero,
            prec,
        }
    }

    pub(crate) fn inf(neg: bool, prec: u32) -> Float {
        Float {
            neg,
            form: Form::Inf,
            prec,
        }
    }

    /// `mant × 2^exp`, rounded to `prec` bits.
    pub(crate) fn from_parts(neg: bool, mant: BigUint, exp: i64, prec: u32) -> Float {
        round(neg, mant, exp, prec)
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.form == Form::Zero
    }

    /// The bits the mantissa needs (Go's `MinPrec`): 0 for zero and infinities.
    pub(crate) fn min_prec(&self) -> u32 {
        match &self.form {
            Form::Finite { mant, .. } => u32::try_from(mant.bits()).unwrap_or(u32::MAX),
            _ => 0,
        }
    }

    /// The same value with precision `prec`, which must be at least `min_prec` (no rounding).
    pub(crate) fn keep_prec(mut self, prec: u32) -> Float {
        self.prec = prec.max(self.min_prec());
        self
    }

    pub(crate) fn is_inf(&self) -> bool {
        self.form == Form::Inf
    }

    /// Rounds to a different precision (Go's `z.SetPrec(p).Set(x)`).
    pub(crate) fn with_prec(&self, prec: u32) -> Float {
        match &self.form {
            Form::Finite { mant, exp } => round(self.neg, mant.clone(), *exp, prec),
            _ => Float {
                prec,
                ..self.clone()
            },
        }
    }

    pub(crate) fn neg(&self) -> Float {
        Float {
            neg: !self.neg,
            ..self.clone()
        }
    }

    pub(crate) fn abs(&self) -> Float {
        Float {
            neg: false,
            ..self.clone()
        }
    }

    /// -1, 0 or 1; zero of either sign is 0.
    pub(crate) fn sign(&self) -> i8 {
        match (&self.form, self.neg) {
            (Form::Zero, _) => 0,
            (_, true) => -1,
            (_, false) => 1,
        }
    }

    pub(crate) fn add(&self, other: &Float, prec: u32) -> Result<Float, NaN> {
        match (&self.form, &other.form) {
            (Form::Inf, Form::Inf) if self.neg != other.neg => Err(NaN),
            (Form::Inf, _) => Ok(Float::inf(self.neg, prec)),
            (_, Form::Inf) => Ok(Float::inf(other.neg, prec)),
            (Form::Zero, Form::Zero) => Ok(Float::zero(self.neg && other.neg, prec)),
            (Form::Zero, _) => Ok(other.with_prec(prec)),
            (_, Form::Zero) => Ok(self.with_prec(prec)),
            (Form::Finite { mant: a, exp: ea }, Form::Finite { mant: b, exp: eb }) => {
                Ok(add_finite((self.neg, a, *ea), (other.neg, b, *eb), prec))
            }
        }
    }

    pub(crate) fn sub(&self, other: &Float, prec: u32) -> Result<Float, NaN> {
        self.add(&other.neg(), prec)
    }

    pub(crate) fn mul(&self, other: &Float, prec: u32) -> Result<Float, NaN> {
        let neg = self.neg != other.neg;
        match (&self.form, &other.form) {
            (Form::Inf, Form::Zero) | (Form::Zero, Form::Inf) => Err(NaN),
            (Form::Inf, _) | (_, Form::Inf) => Ok(Float::inf(neg, prec)),
            (Form::Zero, _) | (_, Form::Zero) => Ok(Float::zero(neg, prec)),
            (Form::Finite { mant: a, exp: ea }, Form::Finite { mant: b, exp: eb }) => {
                Ok(round(neg, a * b, ea.saturating_add(*eb), prec))
            }
        }
    }

    pub(crate) fn quo(&self, other: &Float, prec: u32) -> Result<Float, NaN> {
        let neg = self.neg != other.neg;
        match (&self.form, &other.form) {
            (Form::Zero, Form::Zero) | (Form::Inf, Form::Inf) => Err(NaN),
            (Form::Zero, _) | (_, Form::Inf) => Ok(Float::zero(neg, prec)),
            (Form::Inf, _) | (_, Form::Zero) => Ok(Float::inf(neg, prec)),
            (Form::Finite { mant: a, exp: ea }, Form::Finite { mant: b, exp: eb }) => {
                Ok(quo_finite(neg, (a, *ea), (b, *eb), prec))
            }
        }
    }

    /// Orders by value; -0 equals +0, and infinities sit at the ends.
    pub(crate) fn cmp(&self, other: &Float) -> Ordering {
        let (sa, sb) = (self.sign(), other.sign());
        if sa != sb {
            return sa.cmp(&sb);
        }
        let mag = self.cmp_abs(other);
        if sa < 0 { mag.reverse() } else { mag }
    }

    fn cmp_abs(&self, other: &Float) -> Ordering {
        match (&self.form, &other.form) {
            (Form::Zero, Form::Zero) | (Form::Inf, Form::Inf) => Ordering::Equal,
            (Form::Zero, _) | (_, Form::Inf) => Ordering::Less,
            (_, Form::Zero) | (Form::Inf, _) => Ordering::Greater,
            (Form::Finite { mant: a, exp: ea }, Form::Finite { mant: b, exp: eb }) => {
                let (ta, tb) = (ea + bits(a), eb + bits(b));
                if ta != tb {
                    return ta.cmp(&tb);
                }
                let lo = (*ea).min(*eb);
                let shift = |m: &BigUint, e: i64| m << u64::try_from(e - lo).unwrap_or(0);
                shift(a, *ea).cmp(&shift(b, *eb))
            }
        }
    }

    /// The integer part, toward zero, and whether the value was already an integer. `None` for an
    /// infinity.
    pub(crate) fn trunc(&self) -> Option<(bool, BigUint, bool)> {
        match &self.form {
            Form::Inf => None,
            Form::Zero => Some((self.neg, BigUint::zero(), true)),
            Form::Finite { mant, exp } if *exp >= 0 => {
                Some((self.neg, mant << u64::try_from(*exp).unwrap_or(0), true))
            }
            Form::Finite { mant, exp } => {
                let shift = u64::try_from(-exp).unwrap_or(u64::MAX);
                // mant is odd, so a negative exponent always leaves a fraction behind.
                Some((self.neg, mant >> shift, false))
            }
        }
    }
}

/// Rounds `±mant × 2^exp` to `prec` bits, ties to even, and normalizes.
fn round(neg: bool, mut mant: BigUint, mut exp: i64, prec: u32) -> Float {
    if mant.is_zero() {
        return Float::zero(neg, prec);
    }
    let n = bits(&mant);
    let p = i64::from(prec.max(1));
    if n > p {
        let shift = u64::try_from(n - p).unwrap_or(0);
        let half = mant.bit(shift - 1);
        let sticky = mant.trailing_zeros().is_some_and(|tz| tz < shift - 1);
        mant >>= shift;
        exp = exp.saturating_add(n - p);
        if half && (sticky || mant.bit(0)) {
            mant += 1u8;
        }
    }
    let tz = mant.trailing_zeros().unwrap_or(0);
    mant >>= tz;
    exp = exp.saturating_add(i64::try_from(tz).unwrap_or(0));
    let top = exp.saturating_add(bits(&mant));
    if top > MAX_EXP {
        return Float::inf(neg, prec);
    }
    if top < MIN_EXP {
        return Float::zero(neg, prec);
    }
    Float {
        neg,
        form: Form::Finite { mant, exp },
        prec,
    }
}

type Part<'a> = (bool, &'a BigUint, i64);

fn add_finite(a: Part<'_>, b: Part<'_>, prec: u32) -> Float {
    // Order by magnitude of the top bit, so `big` is the operand that sets the result's scale.
    let top = |p: &Part<'_>| p.2 + bits(p.1);
    let (big, small) = if top(&a) >= top(&b) { (a, b) } else { (b, a) };
    // An operand far below the rounding position only matters as a sticky bit: stand it in by a
    // single bit three places below the last kept one, so the exact sum stays small.
    let floor = top(&big) - i64::from(prec) - 3;
    let small_mant;
    let small = if top(&small) < floor {
        small_mant = BigUint::one();
        (small.0, &small_mant, floor - 1)
    } else {
        small
    };
    let lo = big.2.min(small.2);
    let scale = |p: &Part<'_>| p.1 << u64::try_from(p.2 - lo).unwrap_or(0);
    let (x, y) = (scale(&big), scale(&small));
    if big.0 == small.0 {
        return round(big.0, x + y, lo, prec);
    }
    match x.cmp(&y) {
        Ordering::Equal => Float::zero(false, prec),
        Ordering::Greater => round(big.0, x - y, lo, prec),
        Ordering::Less => round(small.0, y - x, lo, prec),
    }
}

fn quo_finite(neg: bool, a: (&BigUint, i64), b: (&BigUint, i64), prec: u32) -> Float {
    // Scale the dividend so the quotient has at least prec + 2 bits, then keep the remainder as a
    // sticky bit: rounding then sees exactly what an infinitely precise quotient would give it.
    let want = i64::from(prec) + 2;
    let k = (want + bits(b.0) - bits(a.0)).max(0);
    let num = a.0 << u64::try_from(k).unwrap_or(0);
    let q = &num / b.0;
    let r = num - &q * b.0;
    let exp = a.1 - b.1 - k;
    if r.is_zero() {
        round(neg, q, exp, prec)
    } else {
        round(neg, (q << 1u8) | BigUint::one(), exp - 1, prec)
    }
}
