//! The packed form of the tables: one `u32` word array and one string pool, nothing in it a pointer.
//! A string reference is `offset (20 bits) | length (12 bits)` into the pool; 0 means none. Strings and constraints
//! are interned, so a repeated name or constraint costs one reference.

use super::model::Model;
use super::model::Range;
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::fmt::Write as _;

// ---- string pool for the packed form: a reference is offset(20 bits) | length(12 bits); 0 is "none" ----

#[derive(Default)]
struct Pool {
    buf: String,
    ix: BTreeMap<String, u32>,
}

impl Pool {
    fn new() -> Self {
        Self {
            buf: String::from("\u{0}"),
            ix: BTreeMap::new(),
        }
    }
    fn r(&mut self, s: &str) -> Result<u32> {
        if s.is_empty() {
            return Ok(0);
        }
        if let Some(&r) = self.ix.get(s) {
            return Ok(r);
        }
        let off = self.buf.len();
        if s.len() >= 1 << 12 || off + s.len() >= 1 << 20 {
            bail!("string pool limit: {} bytes at {off}", s.len());
        }
        self.buf.push_str(s);
        let r = u32::try_from((off << 12) | s.len())?;
        self.ix.insert(s.to_owned(), r);
        Ok(r)
    }
    fn ro(&mut self, s: Option<&str>) -> Result<u32> {
        s.map_or(Ok(0), |s| self.r(s))
    }
}

fn lit(s: &str) -> String {
    format!("{s:?}")
}

impl Model {
    /// The `tables.rs` source.
    pub(crate) fn emit_tables(&self) -> Result<String> {
        let mut pool = Pool::new();
        let mut shapes = Vec::new();
        for s in &self.shapes {
            shapes.extend([
                pool.r(&s.name)?,
                s.first,
                s.count | (u32::from(s.union) << 31),
                pool.ro(s.xml.as_deref())?,
            ]);
        }
        let mut extras: Vec<u32> = vec![0, 0, 0];
        let mut members = Vec::new();
        for m in &self.members {
            let needs = m.wire.is_some() || m.xml.is_some() || m.bind_str.is_some();
            let extra = if needs {
                let ix = u32::try_from(extras.len() / 3)?;
                extras.extend([
                    pool.ro(m.wire.as_deref())?,
                    pool.ro(m.xml.as_deref())?,
                    pool.ro(m.bind_str.as_deref())?,
                ]);
                ix
            } else {
                0
            };
            let w1 = u32::from(m.kind)
                | (u32::from(m.bind) << 4)
                | (u32::from(m.flags) << 8)
                | (u32::from(m.ts) << 11)
                | (m.cons << 16);
            if m.cons >= 1 << 16 {
                bail!("more than 65535 distinct constraints");
            }
            members.extend([pool.r(&m.name)?, w1, m.target, extra]);
        }
        let mut cons = Vec::new();
        for c in &self.cons {
            let mut flags = 0u32;
            let (mut rmin, mut rmax) = (0u64, 0u64);
            if c.len_min.is_some() {
                flags |= 1;
            }
            if c.len_max.is_some() {
                flags |= 2;
            }
            match &c.range {
                Some(Range::Int(a, b)) => {
                    flags |= 4;
                    if a.is_some() {
                        flags |= 64;
                    }
                    if b.is_some() {
                        flags |= 128;
                    }
                    rmin = a.unwrap_or(0).cast_unsigned();
                    rmax = b.unwrap_or(0).cast_unsigned();
                }
                Some(Range::Float(a, b)) => {
                    flags |= 4 | 8;
                    if a.is_some() {
                        flags |= 64;
                    }
                    if b.is_some() {
                        flags |= 128;
                    }
                    rmin = a.unwrap_or(0.0).to_bits();
                    rmax = b.unwrap_or(0.0).to_bits();
                }
                None => {}
            }
            if c.pattern.is_some() {
                flags |= 16;
            }
            let lm = c.len_min.unwrap_or(0);
            let lx = c.len_max.unwrap_or(0);
            cons.extend([
                flags,
                u32::try_from(lm.min(u64::from(u32::MAX)))?,
                u32::try_from(lx.min(u64::from(u32::MAX)))?,
                (rmin & 0xffff_ffff) as u32,
                (rmin >> 32) as u32,
                (rmax & 0xffff_ffff) as u32,
                (rmax >> 32) as u32,
                pool.ro(c.pattern.as_deref())?,
                c.enum_set.unwrap_or(0),
            ]);
        }
        let mut enum_index = Vec::new();
        let mut enum_data = Vec::new();
        for e in &self.enums {
            enum_index.push(u32::try_from(enum_data.len())?);
            enum_data.push(u32::try_from(e.len())?);
            for s in e {
                enum_data.push(pool.r(s)?);
            }
        }
        let mut errs = Vec::new();
        for e in &self.errs {
            errs.extend([
                e.shape,
                pool.r(&e.code)?,
                u32::from(e.status),
                u32::from(e.fault),
            ]);
        }
        let mut ops = Vec::new();
        let mut op_errs: Vec<u32> = Vec::new();
        for op in &self.ops {
            let (m, u, c) = op
                .http
                .as_ref()
                .map_or(("", "", 0), |(m, u, c)| (m.as_str(), u.as_str(), *c));
            let es = u32::try_from(op_errs.len())?;
            op_errs.extend(&op.errs);
            ops.extend([
                pool.r(&op.name)?,
                op.input.unwrap_or(u32::MAX),
                op.output.unwrap_or(u32::MAX),
                es,
                u32::try_from(op.errs.len())?,
                pool.r(m)?,
                pool.r(u)?,
                u32::from(c),
            ]);
        }
        // One word array: sections back to back; offsets in the header.
        let mut words: Vec<u32> = Vec::new();
        let mut offs = Vec::new();
        for sec in [
            &shapes,
            &members,
            &cons,
            &extras,
            &enum_index,
            &enum_data,
            &errs,
            &ops,
            &op_errs,
        ] {
            offs.push(u32::try_from(words.len())?);
            words.extend(sec.iter());
        }
        let mut o = String::from("use aws_schema::tables::{Layout, Tables};\n\n");
        let _ = writeln!(o, "pub static WORDS: [u32; {}] = [", words.len());
        for chunk in words.chunks(16) {
            let _ = writeln!(
                o,
                "{},",
                chunk
                    .iter()
                    .map(|w| format!("{w}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        o.push_str("];\n");
        let _ = writeln!(o, "pub static POOL: &str = {};", lit(&pool.buf));
        let _ = writeln!(
            o,
            "pub static TABLES: Tables = Tables::new(&WORDS, POOL, Layout {{ shapes: {}, members: {}, cons: {}, extras: {}, enum_index: {}, enum_data: {}, errs: {}, ops: {}, op_errs: {}, n_shapes: {}, n_ops: {} }});",
            offs[0],
            offs[1],
            offs[2],
            offs[3],
            offs[4],
            offs[5],
            offs[6],
            offs[7],
            offs[8],
            self.shapes.len(),
            self.ops.len()
        );
        Ok(o)
    }
}
