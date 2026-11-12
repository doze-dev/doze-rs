//! Scalars: numbers, strings against their constraints, blobs. Plus the length rule and base64.

use super::{D, R, Violation};
use crate::arena::Val;
use crate::tables::{ConsRef, MemberRef, RangeRef};

impl D<'_> {
    pub(super) fn number_ahead(&self, mi: u32, expected: &'static str) -> R<()> {
        if matches!(self.peek(), Some(b'-' | b'0'..=b'9')) {
            Ok(())
        } else {
            Err(Violation::Type {
                member: mi,
                expected,
            })
        }
    }

    pub(super) fn int(&mut self, mi: u32, c: Option<&ConsRef<'_>>) -> R<Val> {
        self.number_ahead(mi, "integer")?;
        let n: i64 = self.number()?.parse().map_err(|_| Violation::Type {
            member: mi,
            expected: "integer",
        })?;
        if let Some(r) = c.and_then(|c| c.range) {
            let ok = match r {
                RangeRef::Int(a, b) => a.is_none_or(|a| n >= a) && b.is_none_or(|b| n <= b),
                #[allow(clippy::cast_precision_loss)]
                RangeRef::Float(a, b) => {
                    a.is_none_or(|a| n as f64 >= a) && b.is_none_or(|b| n as f64 <= b)
                }
            };
            if !ok {
                return Err(Violation::Range {
                    member: mi,
                    value: Some(n.to_string()),
                });
            }
        }
        Ok(Val::Int(n))
    }

    pub(super) fn float(&mut self, mi: u32, ts: bool, c: Option<&ConsRef<'_>>) -> R<Val> {
        if ts && self.peek() == Some(b'"') {
            let v = self.string()?;
            return if crate::time::valid(self.text(v)) {
                Ok(v)
            } else {
                Err(Violation::Type {
                    member: mi,
                    expected: "timestamp",
                })
            };
        }
        self.number_ahead(mi, "number")?;
        let f: f64 = self.number()?.parse().map_err(|_| Violation::Type {
            member: mi,
            expected: "number",
        })?;
        if let Some(r) = c.and_then(|c| c.range) {
            let ok = match r {
                RangeRef::Float(a, b) => a.is_none_or(|a| f >= a) && b.is_none_or(|b| f <= b),
                #[allow(clippy::cast_precision_loss)]
                RangeRef::Int(a, b) => {
                    a.is_none_or(|a| f >= a as f64) && b.is_none_or(|b| f <= b as f64)
                }
            };
            if !ok {
                return Err(Violation::Range {
                    member: mi,
                    value: Some(f.to_string()),
                });
            }
        }
        Ok(Val::Float(f))
    }

    pub(super) fn str_value(
        &mut self,
        m: &MemberRef<'_>,
        mi: u32,
        c: Option<&ConsRef<'_>>,
    ) -> R<Val> {
        if self.peek() != Some(b'"') {
            return Err(Violation::Type {
                member: mi,
                expected: "string",
            });
        }
        let v = self.string()?;
        if let Some(c) = c {
            let s = self.text(v);
            if c.len_min.is_some() || c.len_max.is_some() {
                let n = s.chars().count() as u64;
                if c.len_min.is_some_and(|a| n < a) || c.len_max.is_some_and(|b| n > b) {
                    return Err(Violation::Length {
                        member: mi,
                        value: Some(s.to_owned()),
                    });
                }
            }
            if c.enum_set != 0 && !self.t.enum_has(c.enum_set, s) {
                return Err(Violation::Enum {
                    member: mi,
                    value: Some(s.to_owned()),
                });
            }
            if let Some(p) = c.pattern
                && !self.pats.matches(m.cons, p, s)
            {
                return Err(Violation::Pattern {
                    member: mi,
                    value: Some(s.to_owned()),
                });
            }
        }
        Ok(v)
    }

    pub(super) fn blob(&mut self, mi: u32, c: Option<&ConsRef<'_>>) -> R<Val> {
        if self.peek() != Some(b'"') {
            return Err(Violation::Type {
                member: mi,
                expected: "base64 string",
            });
        }
        let Val::Str(o, l) = self.string()? else {
            return Err(Violation::Type {
                member: mi,
                expected: "base64 string",
            });
        };
        let text = self.src.get(o as usize..(o + l) as usize).unwrap_or("");
        let off = u32::try_from(self.ar.bin.len()).unwrap_or(0);
        self.ar.bin.reserve(text.len() / 4 * 3 + 3);
        // WHY: the decoded bytes exist nowhere in the body, so this write is the one copy a blob needs.
        if !unbase64(text, &mut self.ar.bin) {
            return Err(Violation::Type {
                member: mi,
                expected: "base64 string",
            });
        }
        let len = u32::try_from(self.ar.bin.len()).unwrap_or(0) - off;
        if out_of_bounds(c, u64::from(len)) {
            return Err(Violation::Length {
                member: mi,
                value: None,
            });
        }
        Ok(Val::Blob(off, len))
    }
}

/// Is `n` (a count of characters, bytes, elements or entries) outside the inclusive `@length` bounds?
pub(super) fn out_of_bounds(c: Option<&ConsRef<'_>>, n: u64) -> bool {
    c.is_some_and(|c| c.len_min.is_some_and(|a| n < a) || c.len_max.is_some_and(|b| n > b))
}

/// Standard base64, padding optional. Appends to `out`; false if the text is not base64.
fn unbase64(s: &str, out: &mut Vec<u8>) -> bool {
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes().take_while(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return false,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    true
}
