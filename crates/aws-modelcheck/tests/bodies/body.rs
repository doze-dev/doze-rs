//! The body generator: valid JSON for a shape, built from the tables alone.

use aws_schema::patterns::Patterns;
use aws_schema::tables::{
    ConsRef, F_REQUIRED, K_BLOB, K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STRUCT, K_TS,
    K_UNION, MemberRef, RangeRef, Tables,
};

/// How deep a generated body nests optional members. Required members always go as deep as they must.
const OPTIONAL_DEPTH: u32 = 2;

pub(crate) struct Gen<'t> {
    pub(crate) t: &'t Tables,
    pub(crate) pats: Patterns,
    /// Patterns no seed satisfied, for the report.
    pub(crate) unmet: std::cell::RefCell<std::collections::BTreeSet<String>>,
}

/// A JSON string literal. (Rust's `{:?}` writes `\u{..}`, which JSON does not have.)
pub(crate) fn json(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(char::from(A[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl Gen<'_> {
    pub(crate) fn cons(&self, m: &MemberRef<'_>) -> Option<ConsRef<'_>> {
        (m.cons != 0).then(|| self.t.cons(m.cons)).flatten()
    }

    /// A string that satisfies the member's length, enum and pattern, or `None` if no candidate does.
    pub(crate) fn string(&self, m: &MemberRef<'_>) -> Option<String> {
        let c = self.cons(m);
        if let Some(c) = &c
            && c.enum_set != 0
        {
            return self
                .t
                .enum_values(c.enum_set)
                .first()
                .map(|s| (*s).to_owned());
        }
        let (min, max) = c.as_ref().map_or((0, u64::MAX), |c| {
            (c.len_min.unwrap_or(0), c.len_max.unwrap_or(u64::MAX))
        });
        let want = usize::try_from(min.max(1).min(max)).ok()?;
        let seeds = [
            "a",
            "A",
            "0",
            "abc",
            "a1",
            "/a",
            "a/b",
            "arn:aws:iam::123456789012:role/r",
            "https://example.com/x",
            "us-east-1",
            "a.b",
            "a-b",
            "a_b",
            "x@example.com",
            "2020-01-01T00:00:00Z",
            "{}",
            "1",
            "k",
            "/a/b",
            "alias/a",
            "a:b",
            "arn:aws:sns:us-east-1:123456789012:t",
            "arn:aws:lambda:us-east-1:123456789012:function:f",
            "s3://b/k",
        ];
        for seed in seeds {
            let mut s = seed.repeat(want.div_ceil(seed.len()).max(1));
            s.truncate(want.max(1).min(s.len()));
            let n = s.chars().count() as u64;
            if n < min || n > max {
                continue;
            }
            let ok = match c.as_ref().and_then(|c| c.pattern) {
                Some(p) => self.pats.matches(m.cons, p, &s),
                None => true,
            };
            if ok {
                return Some(s);
            }
        }
        if let Some(s) = self.sample(m, c.as_ref(), min, max) {
            return Some(s);
        }
        if let Some(p) = c.as_ref().and_then(|c| c.pattern) {
            self.unmet
                .borrow_mut()
                .insert(format!("{p}  (length {min}..{max})"));
        }
        None
    }

    /// A string drawn from the member's pattern, accepted only if it meets the length bounds and the pattern as the
    /// decoder reads it.
    fn sample(
        &self,
        m: &MemberRef<'_>,
        c: Option<&ConsRef<'_>>,
        min: u64,
        max: u64,
    ) -> Option<String> {
        use rand::SeedableRng;
        use rand::distr::Distribution;
        let pattern = c?.pattern?;
        // The sampler rejects anchors; every sample is checked against the real pattern below.
        let unanchored = pattern.replace(['^', '$'], "");
        let gen_ = rand_regex::Regex::compile(&unanchored, 8).ok()?;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        (0..300)
            .map(|_| -> String { gen_.sample(&mut rng) })
            .find(|s| {
                let n = s.chars().count() as u64;
                n >= min && n <= max && self.pats.matches(m.cons, pattern, s)
            })
    }

    /// `n` distinct strings valid for the key member `k`.
    fn keys(&self, k: &MemberRef<'_>, n: usize) -> Option<Vec<String>> {
        let first = self.string(k)?;
        let mut keys = vec![first.clone()];
        let mut extra = 0u32;
        while keys.len() < n {
            extra += 1;
            if extra > 50 {
                return None;
            }
            let candidate = format!("{first}{extra}");
            let c = self.cons(k);
            let fits = c.as_ref().is_none_or(|c| {
                c.len_max
                    .is_none_or(|max| candidate.chars().count() as u64 <= max)
            });
            let matches = c
                .as_ref()
                .and_then(|c| c.pattern)
                .is_none_or(|p| self.pats.matches(k.cons, p, &candidate));
            if fits && matches && c.as_ref().is_none_or(|c| c.enum_set == 0) {
                keys.push(candidate);
            }
        }
        Some(keys)
    }

    fn int(&self, m: &MemberRef<'_>) -> String {
        let lo = match self.cons(m).and_then(|c| c.range) {
            Some(RangeRef::Int(Some(a), _)) => a.max(1),
            Some(RangeRef::Float(Some(a), _)) => a.ceil().max(1.0) as i64,
            _ => 1,
        };
        lo.to_string()
    }

    fn float(&self, m: &MemberRef<'_>) -> String {
        let lo = match self.cons(m).and_then(|c| c.range) {
            Some(RangeRef::Float(Some(a), _)) => a.max(1.0),
            Some(RangeRef::Int(Some(a), _)) => (a as f64).max(1.0),
            _ => 1.0,
        };
        format!("{lo}.5")
    }

    fn blob(&self, m: &MemberRef<'_>) -> Option<String> {
        let c = self.cons(m);
        let (min, max) = c.as_ref().map_or((0, u64::MAX), |c| {
            (c.len_min.unwrap_or(0), c.len_max.unwrap_or(u64::MAX))
        });
        let n = usize::try_from(min.max(1).min(max)).ok()?;
        Some(format!("\"{}\"", base64(&vec![b'a'; n])))
    }

    fn count(&self, m: &MemberRef<'_>) -> usize {
        let min = self.cons(m).and_then(|c| c.len_min).unwrap_or(1).max(1);
        usize::try_from(min).unwrap_or(1).min(3)
    }

    /// The JSON text of a valid value for member `m`, or `None` if the generator cannot make one.
    pub(crate) fn value(&self, m: &MemberRef<'_>, depth: u32) -> Option<String> {
        match m.kind {
            K_BOOL => Some("true".into()),
            K_INT => Some(self.int(m)),
            K_FLOAT => Some(self.float(m)),
            K_TS => Some("1700000000".into()),
            K_STR => self.string(m).map(|s| json(&s)),
            K_BLOB => self.blob(m),
            K_LIST => {
                let elem = self.t.member(m.target)?;
                let one = self.value(&elem, depth + 1)?;
                Some(format!("[{}]", vec![one; self.count(m)].join(",")))
            }
            K_MAP => {
                let (k, v) = (self.t.member(m.target)?, self.t.member(m.target + 1)?);
                let value = self.value(&v, depth + 1)?;
                let keys = self.keys(&k, self.count(m))?;
                let entries: Vec<String> = keys
                    .iter()
                    .map(|key| format!("{}:{value}", json(key)))
                    .collect();
                Some(format!("{{{}}}", entries.join(",")))
            }
            K_STRUCT | K_UNION => self.object(m.target, depth + 1),
            _ => None,
        }
    }

    /// A valid object for shape `shape`: every required member, plus the optional ones near the top.
    pub(crate) fn object(&self, shape: u32, depth: u32) -> Option<String> {
        let sh = self.t.shape(shape)?;
        let mut parts = Vec::new();
        for i in sh.first..sh.first + sh.count {
            let m = self.t.member(i)?;
            let required = m.flags & F_REQUIRED != 0;
            let take = if sh.union {
                parts.is_empty() && self.scalar(&m)
            } else {
                required || depth <= OPTIONAL_DEPTH
            };
            if !take
                || matches!(
                    m.kind,
                    aws_schema::tables::K_STREAM | aws_schema::tables::K_EVENT
                )
            {
                continue;
            }
            match self.value(&m, depth) {
                Some(v) => parts.push(format!("{}:{v}", json(m.wire))),
                None if required => return None,
                None => {}
            }
        }
        if sh.union && parts.is_empty() {
            // No scalar member: take the first one that can be made.
            for i in sh.first..sh.first + sh.count {
                let m = self.t.member(i)?;
                if let Some(v) = self.value(&m, depth) {
                    return Some(format!("{{{}:{v}}}", json(m.wire)));
                }
            }
            return None;
        }
        Some(format!("{{{}}}", parts.join(",")))
    }

    pub(crate) fn scalar(&self, m: &MemberRef<'_>) -> bool {
        matches!(m.kind, K_BOOL | K_INT | K_FLOAT | K_STR | K_TS | K_BLOB)
    }
}
