//! Numbers to and from decimal text, by Go's algorithms (`math/big`'s `scan`, `roundShortest`,
//! `fmtF`, `fmtE`), so the same input gives the same bits and the same bits print the same digits.

use super::float::{Float, Form};
use num_bigint::BigUint;
use num_traits::{One, Zero};

/// Decimal digits `0.d1d2… × 10^exp`, no trailing zeros (Go's `decimal`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Decimal {
    mant: Vec<u8>,
    exp: i64,
}

impl Decimal {
    /// The exact decimal of `mant × 2^shift`.
    fn exact(mant: &BigUint, shift: i64) -> Decimal {
        if mant.is_zero() {
            return Decimal::default();
        }
        let (int, frac) = if shift >= 0 {
            (mant << u64::try_from(shift).unwrap_or(0), 0)
        } else {
            // m × 2^-s = m × 5^s / 10^s
            let s = u32::try_from(-shift).unwrap_or(u32::MAX);
            (mant * BigUint::from(5u8).pow(s), i64::from(s))
        };
        let digits = int.to_str_radix(10).into_bytes();
        let exp = i64::try_from(digits.len()).unwrap_or(i64::MAX) - frac;
        let mut d = Decimal { mant: digits, exp };
        d.trim();
        d
    }

    fn trim(&mut self) {
        while self.mant.last() == Some(&b'0') {
            self.mant.pop();
        }
        if self.mant.is_empty() {
            self.exp = 0;
        }
    }

    fn at(&self, i: i64) -> u8 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.mant.get(i).copied())
            .unwrap_or(b'0')
    }

    fn len(&self) -> i64 {
        i64::try_from(self.mant.len()).unwrap_or(i64::MAX)
    }

    fn round(&mut self, n: i64) {
        if n < 0 || n >= self.len() {
            return;
        }
        if self.should_round_up(n) {
            self.round_up(n)
        } else {
            self.round_down(n)
        }
    }

    fn should_round_up(&self, n: i64) -> bool {
        let d = self.at(n);
        if d == b'5' && n + 1 == self.len() {
            // exactly halfway: to even
            return n > 0 && (self.at(n - 1) - b'0') & 1 != 0;
        }
        d >= b'5'
    }

    fn round_up(&mut self, n: i64) {
        if n < 0 || n >= self.len() {
            return;
        }
        let mut n = usize::try_from(n).unwrap_or(0);
        while n > 0 && self.mant.get(n - 1) == Some(&b'9') {
            n -= 1;
        }
        if n == 0 {
            self.mant = vec![b'1'];
            self.exp += 1;
            return;
        }
        self.mant.truncate(n);
        if let Some(last) = self.mant.last_mut() {
            *last += 1;
        }
    }

    fn round_down(&mut self, n: i64) {
        if n < 0 || n >= self.len() {
            return;
        }
        self.mant.truncate(usize::try_from(n).unwrap_or(0));
        self.trim();
    }
}

/// Go's `roundShortest`: the fewest digits that read back to `x` at its precision.
fn round_shortest(d: &mut Decimal, x: &Float) {
    let Form::Finite { mant, exp } = &x.form else {
        return;
    };
    if d.mant.is_empty() {
        return;
    }
    // Go keeps the mantissa left-aligned in whole words; `exp` here is the exponent of its last bit.
    let nbits = i64::try_from(mant.bits()).unwrap_or(0);
    let s = nbits - (i64::from(x.prec) + 1);
    let m = if s < 0 {
        mant << u64::try_from(-s).unwrap_or(0)
    } else {
        mant >> u64::try_from(s).unwrap_or(0)
    };
    let e = exp + s;
    let lower = Decimal::exact(&(&m - BigUint::one()), e);
    let upper = Decimal::exact(&(&m + BigUint::one()), e);
    let inclusive = !m.bit(1);
    for i in 0..d.len() {
        let (l, dm, u) = (lower.at(i), d.at(i), upper.at(i));
        let okdown = l != dm || inclusive && i + 1 == lower.len();
        let okup = dm != u
            && (inclusive || dm + 1 < u || i + 1 < upper.len() || i >= upper.len() && dm < b'9');
        match (okdown, okup) {
            (true, true) => return d.round(i + 1),
            (true, false) => return d.round_down(i + 1),
            (false, true) => return d.round_up(i + 1),
            (false, false) => {}
        }
    }
}

fn decimal_of(x: &Float) -> Decimal {
    match &x.form {
        Form::Finite { mant, exp } => Decimal::exact(mant, *exp),
        _ => Decimal::default(),
    }
}

/// `x.Text('f', -1)`: plain notation, the shortest digits that read back.
pub(crate) fn text_f(x: &Float) -> String {
    text(x, b'f', -1)
}

/// `x.Text('g', 10)` (big.Float's `String`), used where cty orders sets of structured values.
pub(crate) fn text_g10(x: &Float) -> String {
    text(x, b'g', 10)
}

/// Go's `x.Text(fmt, prec)` for `e`, `E`, `f`, `g` and `G`; a negative `prec` asks for the shortest
/// digits that read back.
pub(crate) fn text(x: &Float, fmt: u8, prec: i64) -> String {
    let mut out = String::new();
    if x.neg {
        out.push('-');
    }
    if x.is_inf() {
        if !x.neg {
            out.push('+');
        }
        out.push_str("Inf");
        return out;
    }
    let mut d = decimal_of(x);
    let shortest = prec < 0;
    let prec = if shortest {
        round_shortest(&mut d, x);
        match fmt {
            b'e' | b'E' => d.len() - 1,
            b'f' => (d.len() - d.exp).max(0),
            _ => d.len(),
        }
    } else {
        match fmt {
            b'e' | b'E' => {
                d.round(1 + prec);
                prec
            }
            b'f' => {
                d.round(d.exp + prec);
                prec
            }
            _ => {
                let p = prec.max(1);
                d.round(p);
                p
            }
        }
    };
    match fmt {
        b'e' | b'E' => fmt_e(&mut out, char::from(fmt), prec, &d),
        b'f' => fmt_f(&mut out, prec, &d),
        _ => fmt_g(&mut out, fmt, prec, shortest, &d),
    }
    out
}

fn fmt_g(out: &mut String, fmt: u8, mut prec: i64, shortest: bool, d: &Decimal) {
    let mut eprec = prec;
    if eprec > d.len() && d.len() >= d.exp {
        eprec = d.len();
    }
    if shortest {
        eprec = 6;
    }
    let exp = d.exp - 1;
    if exp < -4 || exp >= eprec {
        prec = prec.min(d.len());
        let e = if fmt == b'G' { 'E' } else { 'e' };
        fmt_e(out, e, prec - 1, d);
    } else {
        if prec > d.exp {
            prec = d.len();
        }
        fmt_f(out, (prec - d.exp).max(0), d);
    }
}

fn fmt_f(out: &mut String, prec: i64, d: &Decimal) {
    if d.exp > 0 {
        for i in 0..d.exp {
            out.push(char::from(d.at(i)));
        }
    } else {
        out.push('0');
    }
    if prec > 0 {
        out.push('.');
        for i in 0..prec {
            out.push(char::from(d.at(d.exp + i)));
        }
    }
}

fn fmt_e(out: &mut String, e: char, prec: i64, d: &Decimal) {
    out.push(char::from(if d.mant.is_empty() { b'0' } else { d.at(0) }));
    if prec > 0 {
        out.push('.');
        let mut i = 1;
        let m = d.len().min(prec + 1);
        while i < m {
            out.push(char::from(d.at(i)));
            i += 1;
        }
        while i <= prec {
            out.push('0');
            i += 1;
        }
    }
    out.push(e);
    let exp = if d.mant.is_empty() { 0 } else { d.exp - 1 };
    out.push(if exp < 0 { '-' } else { '+' });
    let a = exp.unsigned_abs();
    if a < 10 {
        out.push('0');
    }
    out.push_str(&a.to_string());
}

/// For tests: the digit count of the exact decimal, to keep printing bounded.
#[cfg(test)]
pub(crate) fn exact_digits(x: &Float) -> usize {
    decimal_of(x).mant.len()
}
