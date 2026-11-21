//! Numbers as Confluent's normalizing printer writes them: commons-lang's `createNumber`, then `DecimalFormat`.

/// How Confluent's normalizing printer writes a number: commons-lang's `NumberUtils.createNumber`, then a
/// `java.text.DecimalFormat` without grouping (at most three decimals, half to even). Text it cannot read (`inf`, `nan`)
/// is written as it is.
pub(super) fn java_format_number(text: &str) -> String {
    format_number(text).unwrap_or_else(|| text.to_owned())
}

pub(super) fn format_number(text: &str) -> Option<String> {
    let (neg, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let sign = if neg { "-" } else { "" };
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        let v = u128::from_str_radix(hex, 16).ok()?;
        return Some(if v == 0 {
            "0".into()
        } else {
            format!("{sign}{v}")
        });
    }
    if !body.contains(['.', 'e', 'E']) {
        if !body.bytes().all(|b| b.is_ascii_digit()) || body.is_empty() {
            return None;
        }
        let digits = if body.len() > 1 && body.starts_with('0') {
            // Integer.decode reads a leading zero as octal.
            u128::from_str_radix(&body[1..], 8).ok()?.to_string()
        } else {
            body.trim_start_matches('0').to_owned()
        };
        let digits = if digits.is_empty() {
            "0".to_owned()
        } else {
            digits
        };
        return Some(if digits == "0" {
            digits
        } else {
            format!("{sign}{digits}")
        });
    }
    let (mant, frac_exp) = match body.split_once('.') {
        Some((m, rest)) => (m, rest),
        None => (body, ""),
    };
    let frac = frac_exp.split(['e', 'E']).next().unwrap_or_default();
    let mant = mant.split(['e', 'E']).next().unwrap_or_default();
    if !mant.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let all_zero = mant.bytes().chain(frac.bytes()).all(|b| b == b'0');
    let f: f32 = text.parse().ok()?;
    let d: f64 = text.parse().ok()?;
    let exact = Exact::parse(text)?;
    if f.is_finite() && !(f == 0.0 && !all_zero) && format!("{f:e}") == format!("{d:e}") {
        return Some(format_double(f64::from(f)));
    }
    if d.is_finite() && !(d == 0.0 && !all_zero) {
        if Exact::parse(&format!("{d:e}")).is_some_and(|x| x == exact) {
            return Some(format_double(d));
        }
        return Some(exact.format3());
    }
    Some(exact.format3())
}

/// `DecimalFormat.format(double)`: the shortest digits of the double, with at most three decimals (rounded as the
/// double's exact value rounds, half to even).
pub(super) fn format_double(x: f64) -> String {
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let short = format!("{:e}", x.abs());
    let Some(e) = Exact::parse(&short) else {
        return format!("{x}");
    };
    let frac_digits = i64::try_from(e.digits.len()).unwrap_or(0) - 1 - e.exp_of_first();
    if frac_digits <= 3 {
        return format!("{sign}{}", e.plain());
    }
    let s = format!("{:.3}", x.abs());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{sign}{s}")
}

/// An exact decimal: digits (no leading or trailing zeros) times ten to `exp` (of the last digit).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Exact {
    neg: bool,
    digits: Vec<u8>,
    exp: i64,
}

impl Exact {
    pub(super) fn parse(text: &str) -> Option<Self> {
        let (neg, body) = match text.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (m, e) = match body.find(['e', 'E']) {
            Some(i) => (&body[..i], body[i + 1..].parse::<i64>().ok()?),
            None => (body, 0),
        };
        let (int, frac) = m.split_once('.').unwrap_or((m, ""));
        if !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
        let mut exp = e.checked_sub(i64::try_from(frac.len()).ok()?)?;
        while digits.last() == Some(&0) {
            digits.pop();
            exp += 1;
        }
        let lead = digits.iter().take_while(|&&d| d == 0).count();
        digits.drain(..lead);
        if digits.is_empty() {
            return Some(Self {
                neg: false,
                digits,
                exp: 0,
            });
        }
        Some(Self { neg, digits, exp })
    }

    /// The power of ten of the first digit.
    pub(super) fn exp_of_first(&self) -> i64 {
        self.exp + i64::try_from(self.digits.len()).unwrap_or(0) - 1
    }

    /// Written out without an exponent (a magnitude, no sign).
    pub(super) fn plain(&self) -> String {
        if self.digits.is_empty() {
            return "0".into();
        }
        let ds: String = self.digits.iter().map(|d| char::from(b'0' + d)).collect();
        if self.exp >= 0 {
            return format!("{ds}{}", "0".repeat(usize::try_from(self.exp).unwrap_or(0)));
        }
        let frac = usize::try_from(-self.exp).unwrap_or(0);
        if ds.len() > frac {
            let cut = ds.len() - frac;
            format!("{}.{}", &ds[..cut], &ds[cut..])
        } else {
            format!("0.{}{ds}", "0".repeat(frac - ds.len()))
        }
    }

    /// `DecimalFormat.format(BigDecimal)`: at most three decimals, half to even.
    pub(super) fn format3(&self) -> String {
        let sign = if self.neg { "-" } else { "" };
        if self.exp >= -3 {
            return format!("{sign}{}", self.plain());
        }
        // Keep the digits down to 10^-3, and decide on the rest.
        let keep = i64::try_from(self.digits.len()).unwrap_or(0) + self.exp + 3;
        let (mut kept, rest): (Vec<u8>, Vec<u8>) = if keep <= 0 {
            (Vec::new(), {
                let mut r = vec![0; usize::try_from(-keep).unwrap_or(0)];
                r.extend(&self.digits);
                r
            })
        } else {
            let k = usize::try_from(keep).unwrap_or(0).min(self.digits.len());
            let (a, b) = self.digits.split_at(k);
            (a.to_vec(), b.to_vec())
        };
        let half = match rest.first() {
            Some(&d) if d > 5 => std::cmp::Ordering::Greater,
            Some(&5) if rest.len() > 1 => std::cmp::Ordering::Greater,
            Some(&5) => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Less,
        };
        let up = match half {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => kept.last().is_some_and(|d| d % 2 == 1),
            std::cmp::Ordering::Less => false,
        };
        if up {
            // Add one to the last kept digit, carrying.
            let mut carry = true;
            for d in kept.iter_mut().rev() {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                kept.insert(0, 1);
            }
        }
        let rounded = Self {
            neg: false,
            digits: kept,
            exp: -3,
        };
        let text = Self::parse(&format!("{}e-3", rounded.plain_digits()))
            .map_or_else(|| "0".into(), |r| r.plain());
        format!("{sign}{text}")
    }

    pub(super) fn plain_digits(&self) -> String {
        if self.digits.is_empty() {
            "0".into()
        } else {
            self.digits.iter().map(|d| char::from(b'0' + d)).collect()
        }
    }
}
