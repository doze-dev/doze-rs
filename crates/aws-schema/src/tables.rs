//! The schema tables a model crate generates, read in place.
//!
//! The tables are one word array and one string pool, with no pointer in either, so the linker records no
//! relocations and the data sits in read-only pages. A string is `offset (20 bits) | length (12 bits)` into the pool.

pub const K_BOOL: u8 = 0;
pub const K_INT: u8 = 1;
pub const K_FLOAT: u8 = 2;
pub const K_STR: u8 = 3;
pub const K_BLOB: u8 = 4;
pub const K_STREAM: u8 = 5;
pub const K_EVENT: u8 = 6;
pub const K_TS: u8 = 7;
pub const K_LIST: u8 = 8;
pub const K_MAP: u8 = 9;
pub const K_STRUCT: u8 = 10;
pub const K_UNION: u8 = 11;

pub const F_REQUIRED: u8 = 1;

#[derive(Clone, Copy, Debug)]
pub struct ShapeRef<'t> {
    pub name: &'t str,
    pub first: u32,
    pub count: u32,
    pub union: bool,
    pub xml: Option<&'t str>,
}

#[derive(Clone, Copy, Debug)]
pub struct MemberRef<'t> {
    pub name: &'t str,
    /// The key JSON uses (the member name unless `@jsonName`).
    pub wire: &'t str,
    pub xml: Option<&'t str>,
    pub bind: u8,
    pub bind_str: Option<&'t str>,
    pub kind: u8,
    pub flags: u8,
    pub ts: u8,
    /// Struct and union: shape index. List: element member index. Map: key member index (the value follows it).
    pub target: u32,
    pub cons: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum RangeRef {
    Int(Option<i64>, Option<i64>),
    Float(Option<f64>, Option<f64>),
}

#[derive(Clone, Copy, Debug)]
pub struct ConsRef<'t> {
    pub len_min: Option<u64>,
    pub len_max: Option<u64>,
    pub range: Option<RangeRef>,
    pub pattern: Option<&'t str>,
    pub enum_set: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct OpRef<'t> {
    pub name: &'t str,
    pub input: Option<u32>,
    pub output: Option<u32>,
    /// The HTTP method of the `@http` binding; empty for a protocol that has none.
    pub method: &'t str,
    /// The URI template of the `@http` binding (`/{Bucket}/{Key+}`); empty when there is none.
    pub uri: &'t str,
    /// The success status of the `@http` binding; 0 when there is none.
    pub code: u16,
    /// First of this operation's errors in the operation-error list, and how many there are.
    pub errors: (u32, u32),
}

/// A modelled error structure.
#[derive(Clone, Copy, Debug)]
pub struct ErrRef<'t> {
    /// The shape index of the error structure.
    pub shape: u32,
    /// The code a client sees (the `awsQueryError` code, else the shape name).
    pub code: &'t str,
    /// The HTTP status.
    pub status: u16,
    /// A server fault (5xx class) rather than a client one.
    pub server: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub shapes: u32,
    pub members: u32,
    pub cons: u32,
    pub extras: u32,
    pub enum_index: u32,
    pub enum_data: u32,
    pub errs: u32,
    pub ops: u32,
    pub op_errs: u32,
    pub n_shapes: u32,
    pub n_ops: u32,
}

#[derive(Debug)]
pub struct Tables {
    w: &'static [u32],
    pool: &'static str,
    l: Layout,
}

impl Tables {
    #[must_use]
    pub const fn new(w: &'static [u32], pool: &'static str, l: Layout) -> Self {
        Self { w, pool, l }
    }

    // WHY: the words are generated and the generator's self-check walks every index, so an out-of-range read is a generator bug; a
    // zero is the "none" value of every field, which is what the walk would report. A panic here would be a crash on a bad table
    // instead of a refused request.
    fn word(&self, i: u32) -> u32 {
        self.w.get(i as usize).copied().unwrap_or(0)
    }

    fn string(&self, r: u32) -> &str {
        if r == 0 {
            return "";
        }
        let (off, len) = ((r >> 12) as usize, (r & 0xfff) as usize);
        self.pool.get(off..off + len).unwrap_or("")
    }

    fn opt_string(&self, r: u32) -> Option<&str> {
        (r != 0).then(|| self.string(r))
    }
}

impl Tables {
    #[must_use]
    pub fn n_shapes(&self) -> u32 {
        self.l.n_shapes
    }
    #[must_use]
    pub fn n_members(&self) -> u32 {
        (self.l.cons - self.l.members) / 4
    }
    /// How many enum sets there are (set 0 is "none").
    #[must_use]
    pub fn n_enum_sets(&self) -> u32 {
        self.l.enum_data - self.l.enum_index
    }
    #[must_use]
    pub fn n_cons(&self) -> u32 {
        (self.l.extras - self.l.cons) / 9
    }
    #[must_use]
    pub fn n_ops(&self) -> u32 {
        self.l.n_ops
    }
    #[must_use]
    pub fn shape(&self, i: u32) -> Option<ShapeRef<'_>> {
        if i >= self.l.n_shapes {
            return None;
        }
        let b = self.l.shapes + i * 4;
        let cu = self.word(b + 2);
        Some(ShapeRef {
            name: self.string(self.word(b)),
            first: self.word(b + 1),
            count: cu & 0x7fff_ffff,
            union: cu >> 31 != 0,
            xml: self.opt_string(self.word(b + 3)),
        })
    }
    #[must_use]
    pub fn member(&self, i: u32) -> Option<MemberRef<'_>> {
        let b = self.l.members + i * 4;
        if b + 3 >= self.l.cons {
            return None;
        }
        let w1 = self.word(b + 1);
        let name = self.string(self.word(b));
        let extra = self.word(b + 3);
        let (wire, xml, bind_str) = if extra == 0 {
            (name, None, None)
        } else {
            let e = self.l.extras + extra * 3;
            let wire = self.opt_string(self.word(e)).unwrap_or(name);
            (
                wire,
                self.opt_string(self.word(e + 1)),
                self.opt_string(self.word(e + 2)),
            )
        };
        Some(MemberRef {
            name,
            wire,
            xml,
            bind: ((w1 >> 4) & 0xf) as u8,
            bind_str,
            kind: (w1 & 0xf) as u8,
            flags: ((w1 >> 8) & 0x7) as u8,
            ts: ((w1 >> 11) & 0x3) as u8,
            target: self.word(b + 2),
            cons: w1 >> 16,
        })
    }
    #[must_use]
    pub fn cons(&self, i: u32) -> Option<ConsRef<'_>> {
        let b = self.l.cons + i * 9;
        if b + 8 >= self.l.extras {
            return None;
        }
        let f = self.word(b);
        let u64of = |lo: u32, hi: u32| u64::from(self.word(lo)) | (u64::from(self.word(hi)) << 32);
        let (rmin, rmax) = (u64of(b + 3, b + 4), u64of(b + 5, b + 6));
        let (has_min, has_max) = (f & 64 != 0, f & 128 != 0);
        let range = (f & 4 != 0).then(|| {
            if f & 8 != 0 {
                RangeRef::Float(
                    has_min.then(|| f64::from_bits(rmin)),
                    has_max.then(|| f64::from_bits(rmax)),
                )
            } else {
                RangeRef::Int(
                    has_min.then(|| rmin.cast_signed()),
                    has_max.then(|| rmax.cast_signed()),
                )
            }
        });
        Some(ConsRef {
            len_min: (f & 1 != 0).then(|| u64::from(self.word(b + 1))),
            len_max: (f & 2 != 0).then(|| u64::from(self.word(b + 2))),
            range,
            pattern: if f & 16 != 0 {
                self.opt_string(self.word(b + 7))
            } else {
                None
            },
            enum_set: self.word(b + 8),
        })
    }
    #[must_use]
    pub fn enum_has(&self, set: u32, s: &str) -> bool {
        if set == 0 {
            return false;
        }
        let start = self.l.enum_data + self.word(self.l.enum_index + set);
        let n = self.word(start);
        (0..n).any(|k| self.string(self.word(start + 1 + k)) == s)
    }
    /// The index of the operation called `name`. The generator writes operations in name order, so this is a binary search.
    #[must_use]
    pub fn find_op(&self, name: &str) -> Option<u32> {
        let (mut lo, mut hi) = (0, self.l.n_ops);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.op(mid)?.name.cmp(name) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    #[must_use]
    pub fn op(&self, i: u32) -> Option<OpRef<'_>> {
        if i >= self.l.n_ops {
            return None;
        }
        let b = self.l.ops + i * 8;
        let opt = |x: u32| if x == u32::MAX { None } else { Some(x) };
        Some(OpRef {
            name: self.string(self.word(b)),
            input: opt(self.word(b + 1)),
            output: opt(self.word(b + 2)),
            method: self.string(self.word(b + 5)),
            uri: self.string(self.word(b + 6)),
            code: u16::try_from(self.word(b + 7)).unwrap_or(0),
            errors: (self.word(b + 3), self.word(b + 4)),
        })
    }
}

impl Tables {
    /// How many error structures the model defines.
    #[must_use]
    pub fn n_errs(&self) -> u32 {
        (self.l.ops - self.l.errs) / 4
    }

    /// Error `i` of the model's error list.
    #[must_use]
    pub fn err(&self, i: u32) -> Option<ErrRef<'_>> {
        if i >= self.n_errs() {
            return None;
        }
        let b = self.l.errs + i * 4;
        Some(ErrRef {
            shape: self.word(b),
            code: self.string(self.word(b + 1)),
            status: u16::try_from(self.word(b + 2)).unwrap_or(0),
            server: self.word(b + 3) != 0,
        })
    }

    /// The errors operation `op` can answer, in model order.
    pub fn op_errors(&self, op: &OpRef<'_>) -> impl Iterator<Item = ErrRef<'_>> {
        let (start, n) = op.errors;
        (0..n).filter_map(move |k| self.err(self.word(self.l.op_errs + start + k)))
    }

    /// The strings of enum set `set`, in model order.
    #[must_use]
    pub fn enum_values(&self, set: u32) -> Vec<&str> {
        if set == 0 {
            return Vec::new();
        }
        let start = self.l.enum_data + self.word(self.l.enum_index + set);
        (0..self.word(start))
            .map(|k| self.string(self.word(start + 1 + k)))
            .collect()
    }
}
