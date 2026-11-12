//! One field of a cron expression: a set of numbers, read from lists, ranges, steps and names.

use crate::Invalid;

/// A set of numbers below 256.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bits([u64; 4]);

impl Bits {
    pub(crate) fn set(&mut self, n: u32) {
        let (word, bit) = ((n / 64) as usize, n % 64);
        if let Some(w) = self.0.get_mut(word) {
            *w |= 1 << bit;
        }
    }

    pub(crate) fn has(self, n: u32) -> bool {
        let (word, bit) = ((n / 64) as usize, n % 64);
        self.0.get(word).is_some_and(|w| ((w >> bit) & 1) == 1)
    }

    /// The least member that is `n` or more and at most `max`.
    pub(crate) fn from(self, n: u32, max: u32) -> Option<u32> {
        (n..=max).find(|k| self.has(*k))
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0 == [0; 4]
    }
}

/// What a field's numbers mean: its range and the names it accepts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Kind {
    pub(crate) min: u32,
    pub(crate) max: u32,
    pub(crate) names: &'static [&'static str],
    /// The number the first name stands for.
    pub(crate) first_name: u32,
    /// Subtracted from a number before it is kept, so a set of years fits the bits.
    pub(crate) offset: u32,
}

pub(crate) const MINUTE: Kind = Kind {
    min: 0,
    max: 59,
    names: &[],
    first_name: 0,
    offset: 0,
};
pub(crate) const HOUR: Kind = Kind {
    min: 0,
    max: 23,
    names: &[],
    first_name: 0,
    offset: 0,
};
pub(crate) const DAY_OF_MONTH: Kind = Kind {
    min: 1,
    max: 31,
    names: &[],
    first_name: 0,
    offset: 0,
};
pub(crate) const MONTH: Kind = Kind {
    min: 1,
    max: 12,
    names: &[
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ],
    first_name: 1,
    offset: 0,
};
pub(crate) const DAY_OF_WEEK: Kind = Kind {
    min: 1,
    max: 7,
    names: &["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"],
    first_name: 1,
    offset: 0,
};
pub(crate) const YEAR: Kind = Kind {
    min: 1970,
    max: 2199,
    names: &[],
    first_name: 0,
    offset: 1970,
};

impl Kind {
    /// A number or a name.
    pub(crate) fn value(&self, text: &str) -> Result<u32, Invalid> {
        if let Some(i) = self.names.iter().position(|n| n.eq_ignore_ascii_case(text)) {
            return Ok(self.first_name + u32::try_from(i).unwrap_or(0));
        }
        let n: u32 = text.parse().map_err(|_| Invalid)?;
        if n < self.min || n > self.max {
            return Err(Invalid);
        }
        Ok(n)
    }
}

/// A plain element (no `?`, `L`, `W` or `#`): `*`, `a`, `a-b`, each with an optional `/step`.
pub(crate) fn plain(kind: &Kind, element: &str, into: &mut Bits) -> Result<(), Invalid> {
    let (range, step) = match element.split_once('/') {
        Some((r, s)) => (r, s.parse::<u32>().map_err(|_| Invalid)?),
        None => (element, 1),
    };
    if step == 0 {
        return Err(Invalid);
    }
    let (lo, hi) = if range == "*" {
        (kind.min, kind.max)
    } else if let Some((a, b)) = range.split_once('-') {
        (kind.value(a)?, kind.value(b)?)
    } else {
        let a = kind.value(range)?;
        // `a/n` runs from a to the end of the range; a bare `a` is just a.
        (a, if element.contains('/') { kind.max } else { a })
    };
    if lo > hi {
        return Err(Invalid);
    }
    let mut n = lo;
    while n <= hi {
        into.set(n - kind.offset);
        n += step;
    }
    Ok(())
}

/// A field made only of plain elements.
pub(crate) fn plain_field(kind: &Kind, text: &str) -> Result<Bits, Invalid> {
    let mut bits = Bits::default();
    for element in text.split(',') {
        plain(kind, element, &mut bits)?;
    }
    if bits.is_empty() {
        return Err(Invalid);
    }
    Ok(bits)
}
