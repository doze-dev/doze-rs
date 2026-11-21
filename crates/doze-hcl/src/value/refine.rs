//! cty's refinements: what is already known about an unknown value. An unknown can be known not to
//! be null, a string known to start with a prefix, a number known to lie in a range, a collection
//! known to have a length in a range. cty uses them to decide `==`, comparisons and splats early,
//! so an expression over unknowns can still have a known result.

use super::Value;
use crate::number::Number;
use crate::types::Type;
use std::cmp::Ordering;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refinement {
    pub not_null: bool,
    /// A known prefix of a string ("" for none).
    pub prefix: String,
    /// Number bounds, each with whether it is inclusive.
    pub lower: Option<(Number, bool)>,
    pub upper: Option<(Number, bool)>,
    /// Collection length bounds (lower, upper); `None` is (0, unbounded).
    pub len: Option<(usize, Option<usize>)>,
}

impl Refinement {
    pub fn not_null() -> Refinement {
        Refinement {
            not_null: true,
            ..Refinement::default()
        }
    }

    /// Adds what `more` knows: the longer prefix, the tighter bounds.
    pub fn merge(&mut self, more: &Refinement) {
        self.not_null |= more.not_null;
        if more.prefix.len() > self.prefix.len() {
            self.prefix.clone_from(&more.prefix);
        }
        self.lower = tighter(self.lower.take(), more.lower.clone(), Ordering::Greater);
        self.upper = tighter(self.upper.take(), more.upper.clone(), Ordering::Less);
        self.len = match (self.len, more.len) {
            (None, l) | (l, None) => l,
            (Some((a, ah)), Some((b, bh))) => {
                let hi = match (ah, bh) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (x, y) => x.or(y),
                };
                Some((a.max(b), hi))
            }
        };
    }
}

/// Of two number bounds, the one further in direction `dir`; on a tie the exclusive one.
fn tighter(
    a: Option<(Number, bool)>,
    b: Option<(Number, bool)>,
    dir: Ordering,
) -> Option<(Number, bool)> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => match b.0.cmp_value(&a.0) {
            o if o == dir => Some(b),
            Ordering::Equal => Some((a.0, a.1 && b.1)),
            _ => Some(a),
        },
    }
}

/// cty's `NewValue` for a collection whose length is known exactly: an empty collection, a list of
/// that many unknowns, or a set of one unknown.
fn exact_collection(t: &Type, n: usize) -> Option<Value> {
    let unknowns = |e: &Type| vec![Value::unknown(e.clone()); n];
    match t {
        Type::List(e) => Some(Value::List((**e).clone(), unknowns(e))),
        Type::Set(e) if n <= 1 => Some(Value::Set((**e).clone(), unknowns(e))),
        Type::Map(e) if n == 0 => {
            Some(Value::Map((**e).clone(), std::collections::BTreeMap::new()))
        }
        _ => None,
    }
}

impl Value {
    /// An unknown of type `t`, with nothing more known.
    pub const fn unknown(t: Type) -> Value {
        Value::Unknown(t, None)
    }

    /// An unknown known not to be null (cty's `RefineNotNull`). The dynamic unknown stays as it is.
    pub fn unknown_not_null(t: Type) -> Value {
        Value::refined(t, Refinement::not_null())
    }

    /// An unknown with what is known about it. A number range that is a single inclusive point is
    /// that number, as in cty; an empty refinement is a plain unknown; the dynamic unknown takes none.
    pub fn refined(t: Type, r: Refinement) -> Value {
        if t == Type::Dynamic || r == Refinement::default() {
            return Value::unknown(t);
        }
        if let (true, Some((lo, true)), Some((hi, true))) = (r.not_null, &r.lower, &r.upper)
            && lo.same(hi)
        {
            return Value::Number(lo.clone());
        }
        if let (true, Some((lo, Some(hi)))) = (r.not_null, r.len)
            && lo == hi
            && let Some(v) = exact_collection(&t, lo)
        {
            return v;
        }
        Value::Unknown(t, Some(Box::new(r)))
    }

    /// cty's `RefineWith`: an unknown (not of the dynamic type) gains what `more` says; anything
    /// else is returned as it is.
    pub fn refine_with(self, more: &Refinement) -> Value {
        let Value::Unknown(t, r) = self else {
            return self;
        };
        if t == Type::Dynamic {
            return Value::Unknown(t, r);
        }
        let mut r = r.map(|r| *r).unwrap_or_default();
        r.merge(more);
        Value::refined(t, r)
    }

    /// What is known about this value: for a known value, exactly it; for an unknown, its refinement.
    pub fn refinement(&self) -> Refinement {
        match self {
            Value::Unknown(_, Some(r)) => (**r).clone(),
            Value::Unknown(_, None) | Value::Null(_) => Refinement::default(),
            Value::Number(n) => Refinement {
                not_null: true,
                lower: Some((n.clone(), true)),
                upper: Some((n.clone(), true)),
                ..Refinement::default()
            },
            Value::String(s) => Refinement {
                not_null: true,
                prefix: s.clone(),
                ..Refinement::default()
            },
            v => {
                let len = v.len().map(|n| (n, Some(n)));
                Refinement {
                    not_null: true,
                    len,
                    ..Refinement::default()
                }
            }
        }
    }

    /// Known not to be null (cty's `Range().DefinitelyNotNull()`).
    pub fn definitely_not_null(&self) -> bool {
        match self {
            Value::Null(_) => false,
            Value::Unknown(_, r) => r.as_ref().is_some_and(|r| r.not_null),
            _ => true,
        }
    }

    /// Could be null (cty's `Range().CouldBeNull()`).
    pub fn could_be_null(&self) -> bool {
        !self.definitely_not_null()
    }

    /// Whether this unknown's range rules `v` out (cty's `Range().Includes(v)` being false).
    pub(crate) fn excludes(&self, v: &Value) -> bool {
        let Value::Unknown(ty, r) = self else {
            return false;
        };
        let r = r.as_deref().cloned().unwrap_or_default();
        if v.is_null() {
            return r.not_null;
        }
        if !v.ty().conforms_to(ty) {
            return true;
        }
        if let Value::String(s) = v {
            return !s.starts_with(&r.prefix);
        }
        if let Some(n) = v.as_number() {
            let below = r
                .lower
                .as_ref()
                .is_some_and(|(lo, inc)| outside(n.cmp_value(lo), Ordering::Less, *inc));
            let above = r
                .upper
                .as_ref()
                .is_some_and(|(hi, inc)| outside(n.cmp_value(hi), Ordering::Greater, *inc));
            return below || above;
        }
        match (v.len(), r.len) {
            (Some(n), Some((lo, hi))) => n < lo || hi.is_some_and(|hi| n > hi),
            _ => false,
        }
    }
}

/// Whether `ord` (value against a bound) falls on the far side of an inclusive or exclusive bound.
fn outside(ord: Ordering, far: Ordering, inclusive: bool) -> bool {
    ord == far || (ord == Ordering::Equal && !inclusive)
}

/// The prefix cty keeps from a known prefix (`ctystrings.SafeKnownPrefix`): it drops a last grapheme
/// that a later character could still change, unless it is punctuation that always ends one.
pub(crate) fn safe_prefix(prefix: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_segmentation::UnicodeSegmentation;
    let p: String = prefix.nfc().collect();
    let Some((start, last)) = p.grapheme_indices(true).next_back() else {
        return p;
    };
    let ends = matches!(
        last,
        "-" | "_"
            | ":"
            | ";"
            | "/"
            | "\\"
            | ","
            | "."
            | "("
            | ")"
            | "{"
            | "}"
            | "["
            | "]"
            | "|"
            | "?"
            | "!"
            | "~"
            | " "
            | "\t"
            | "@"
            | "#"
            | "$"
            | "%"
            | "^"
            | "&"
            | "*"
            | "+"
            | "\""
            | "'"
    );
    if ends {
        p
    } else {
        p.get(..start).unwrap_or("").to_owned()
    }
}
