//! The neutral table model: the IR flattened into shapes, members, constraints, enum sets, operations and errors, each
//! referred to by index. Both the words and the typed keys are written from this.

use crate::ir::{Ir, SBinding, SMember, STarget, Ts, Ty, TypeKind};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub(super) const K_BOOL: u8 = 0;
pub(super) const K_INT: u8 = 1;
pub(super) const K_FLOAT: u8 = 2;
pub(super) const K_STR: u8 = 3;
pub(super) const K_BLOB: u8 = 4;
pub(super) const K_STREAM: u8 = 5;
pub(super) const K_EVENT: u8 = 6;
pub(super) const K_TS: u8 = 7;
pub(super) const K_LIST: u8 = 8;
pub(super) const K_MAP: u8 = 9;
pub(super) const K_STRUCT: u8 = 10;
pub(super) const K_UNION: u8 = 11;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Range {
    Int(Option<i64>, Option<i64>),
    Float(Option<f64>, Option<f64>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Cons {
    pub(super) len_min: Option<u64>,
    pub(super) len_max: Option<u64>,
    pub(super) range: Option<Range>,
    pub(super) pattern: Option<String>,
    pub(super) enum_set: Option<u32>,
}

#[derive(Debug)]
pub(super) struct Shape {
    pub(super) name: String,
    pub(super) first: u32,
    pub(super) count: u32,
    pub(super) union: bool,
    pub(super) xml: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(super) struct Mem {
    pub(super) name: String,
    pub(super) wire: Option<String>,
    pub(super) xml: Option<String>,
    pub(super) bind: u8,
    pub(super) bind_str: Option<String>,
    pub(super) kind: u8,
    pub(super) flags: u8,
    pub(super) ts: u8,
    pub(super) target: u32,
    pub(super) cons: u32,
}

#[derive(Debug)]
pub(super) struct Op {
    pub(super) name: String,
    pub(super) input: Option<u32>,
    pub(super) output: Option<u32>,
    pub(super) errs: Vec<u32>,
    pub(super) http: Option<(String, String, u16)>,
}

#[derive(Debug)]
pub(super) struct Err {
    pub(super) shape: u32,
    pub(super) code: String,
    pub(super) status: u16,
    pub(super) fault: u8,
}

#[derive(Default)]
pub(crate) struct Model {
    pub(super) shapes: Vec<Shape>,
    pub(super) members: Vec<Mem>,
    pub(super) cons: Vec<Cons>,
    pub(super) enums: Vec<Vec<String>>,
    pub(super) ops: Vec<Op>,
    pub(super) errs: Vec<Err>,
    pub(super) shape_ix: HashMap<String, u32>,
    pub(super) cons_ix: HashMap<String, u32>,
    pub(super) enum_ix: HashMap<Vec<String>, u32>,
    pub(super) hand_patterns: Vec<String>,
}

fn bind_kind(b: &SBinding) -> (u8, Option<String>) {
    match b {
        SBinding::Body => (0, None),
        SBinding::Label => (1, None),
        SBinding::Query(s) => (2, Some(s.clone())),
        SBinding::QueryParams => (3, None),
        SBinding::Header(s) => (4, Some(s.clone())),
        SBinding::PrefixHeaders(s) => (5, Some(s.clone())),
        SBinding::Payload => (6, None),
        SBinding::ResponseCode => (7, None),
        SBinding::HostLabel => (8, None),
    }
}

const fn ts_code(t: Option<Ts>) -> u8 {
    match t {
        None => 0,
        Some(Ts::EpochSeconds) => 1,
        Some(Ts::DateTime) => 2,
        Some(Ts::HttpDate) => 3,
    }
}

impl Model {
    pub fn build(ir: &Ir) -> Result<Self> {
        let mut m = Self::default();
        // Placeholders: constraint 0 and enum 0 mean "none".
        m.cons.push(Cons::default());
        m.enums.push(Vec::new());
        // Shapes first, so targets can refer forward.
        for td in ir.types.values() {
            if matches!(td.kind, TypeKind::Struct { .. } | TypeKind::Union(_)) {
                let ix = u32::try_from(m.shapes.len())?;
                m.shape_ix.insert(td.name.clone(), ix);
                m.shapes.push(Shape {
                    name: td.name.clone(),
                    first: 0,
                    count: 0,
                    union: matches!(td.kind, TypeKind::Union(_)),
                    xml: None,
                });
            }
        }
        for td in ir.types.values() {
            let Some(&ix) = m.shape_ix.get(&td.name) else {
                continue;
            };
            match &td.kind {
                TypeKind::Struct {
                    fields,
                    xml_name,
                    error,
                } => {
                    let first = m.reserve(fields.len())?;
                    for (i, f) in fields.iter().enumerate() {
                        // A member with a model default is not required: the caller may leave it out.
                        let mem =
                            m.member(ir, &f.schema, &f.ty, !f.optional && f.default.is_none())?;
                        m.set(first + u32::try_from(i)?, mem);
                    }
                    let sh = &mut m.shapes[ix as usize];
                    sh.first = first;
                    sh.count = u32::try_from(fields.len())?;
                    sh.xml.clone_from(xml_name);
                    if let Some(e) = error {
                        m.errs.push(Err {
                            shape: ix,
                            code: e.query_code.clone(),
                            status: e.http_status,
                            fault: u8::from(matches!(e.fault, crate::ir::FaultKind::Server)),
                        });
                    }
                }
                TypeKind::Union(vars) => {
                    let first = m.reserve(vars.len())?;
                    for (i, v) in vars.iter().enumerate() {
                        let ty = v.ty.clone().unwrap_or(Ty::Prim(crate::ir::Prim::Bool));
                        let mem = m.member(ir, &v.schema, &ty, false)?;
                        m.set(first + u32::try_from(i)?, mem);
                    }
                    let sh = &mut m.shapes[ix as usize];
                    sh.first = first;
                    sh.count = u32::try_from(vars.len())?;
                }
                _ => {}
            }
        }
        for op in &ir.ops {
            let input = op.input.as_ref().and_then(|n| m.shape_ix.get(n).copied());
            let output = op.output.as_ref().and_then(|n| m.shape_ix.get(n).copied());
            let mut errs = Vec::new();
            for e in &op.errors {
                if let Some(&sx) = m.shape_ix.get(e)
                    && let Some(i) = m.errs.iter().position(|x| x.shape == sx)
                {
                    errs.push(u32::try_from(i)?);
                }
            }
            m.ops.push(Op {
                name: op.name.clone(),
                input,
                output,
                errs,
                http: op
                    .http
                    .as_ref()
                    .map(|h| (h.method.clone(), h.uri.clone(), h.code)),
            });
        }
        Ok(m)
    }

    fn reserve(&mut self, n: usize) -> Result<u32> {
        let first = u32::try_from(self.members.len())?;
        self.members.extend(std::iter::repeat_n(Mem::default(), n));
        Ok(first)
    }
    fn set(&mut self, ix: u32, mem: Mem) {
        self.members[ix as usize] = mem;
    }

    /// The constraints a named newtype adds, and the type its children have.
    fn resolve<'t>(&mut self, ir: &'t Ir, ty: &'t Ty) -> (Cons, Option<&'t Ty>, Option<&'t Ty>) {
        match ty {
            Ty::Named(n) => match ir.types.get(n).map(|t| &t.kind) {
                Some(TypeKind::StrNewtype(c) | TypeKind::BlobNewtype(c)) => {
                    (self.cons_of(c, None), None, None)
                }
                Some(TypeKind::IntNewtype(c) | TypeKind::FloatNewtype(c)) => {
                    (self.cons_of(c, None), None, None)
                }
                Some(TypeKind::ListNewtype(inner, c)) => (self.cons_of(c, None), Some(inner), None),
                Some(TypeKind::MapNewtype(k, v, c)) => (self.cons_of(c, None), Some(k), Some(v)),
                Some(TypeKind::Enum(vs)) => {
                    let set: Vec<String> = vs.iter().map(|v| v.wire.clone()).collect();
                    (
                        self.cons_of(&crate::model::Constraints::default(), Some(set)),
                        None,
                        None,
                    )
                }
                _ => (Cons::default(), None, None),
            },
            Ty::Vec(inner) => (Cons::default(), Some(inner), None),
            Ty::Map(k, v) => (Cons::default(), Some(k), Some(v)),
            Ty::Prim(_) => (Cons::default(), None, None),
        }
    }

    fn cons_of(&mut self, c: &crate::model::Constraints, enum_vals: Option<Vec<String>>) -> Cons {
        let range = c.range.as_ref().and_then(|b| {
            let is_float = b.min.as_ref().is_some_and(|n| !n.is_i64() && !n.is_u64())
                || b.max.as_ref().is_some_and(|n| !n.is_i64() && !n.is_u64());
            if b.min.is_none() && b.max.is_none() {
                None
            } else if is_float {
                Some(Range::Float(
                    b.min.as_ref().and_then(serde_json::Number::as_f64),
                    b.max.as_ref().and_then(serde_json::Number::as_f64),
                ))
            } else {
                Some(Range::Int(
                    b.min.as_ref().and_then(serde_json::Number::as_i64),
                    b.max.as_ref().and_then(serde_json::Number::as_i64),
                ))
            }
        });
        let enum_set = enum_vals.map(|v| {
            let next = u32::try_from(self.enums.len()).unwrap_or(0);
            *self.enum_ix.entry(v.clone()).or_insert_with(|| {
                self.enums.push(v);
                next
            })
        });
        Cons {
            len_min: c.length.as_ref().and_then(|b| b.min),
            len_max: c.length.as_ref().and_then(|b| b.max),
            range,
            pattern: c.pattern.clone(),
            enum_set,
        }
    }

    fn intern_cons(&mut self, c: Cons) -> u32 {
        if c == Cons::default() {
            return 0;
        }
        if let Some(p) = &c.pattern
            && regex_lite::Regex::new(p).is_err()
            && !self.hand_patterns.contains(p)
        {
            self.hand_patterns.push(p.clone());
        }
        let key = format!("{c:?}");
        if let Some(&i) = self.cons_ix.get(&key) {
            return i;
        }
        let i = u32::try_from(self.cons.len()).unwrap_or(0);
        self.cons.push(c);
        self.cons_ix.insert(key, i);
        i
    }

    /// One member record (and, for lists and maps, the records of their elements).
    fn member(&mut self, ir: &Ir, s: &SMember, ty: &Ty, required: bool) -> Result<Mem> {
        let (own, child_a, child_b) = self.resolve(ir, ty);
        let cons = self.intern_cons(own);
        let (bind, bind_str) = bind_kind(&s.binding);
        let mut flags = 0;
        if required || s.required {
            flags |= 1;
        }
        if s.flattened {
            flags |= 2;
        }
        if s.xml_attribute {
            flags |= 4;
        }
        let (kind, ts, target) = match &s.target {
            STarget::Bool => (K_BOOL, 0, 0),
            STarget::Int => (K_INT, 0, 0),
            STarget::Float => (K_FLOAT, 0, 0),
            STarget::Str => (K_STR, 0, 0),
            STarget::Blob => (K_BLOB, 0, 0),
            STarget::Stream => (K_STREAM, 0, 0),
            STarget::EventStream => (K_EVENT, 0, 0),
            STarget::Timestamp(t) => (K_TS, ts_code(*t), 0),
            STarget::Struct(n) | STarget::Union(n) => {
                let Some(&ix) = self.shape_ix.get(n) else {
                    bail!("no shape {n}")
                };
                (
                    if matches!(s.target, STarget::Struct(_)) {
                        K_STRUCT
                    } else {
                        K_UNION
                    },
                    0,
                    ix,
                )
            }
            STarget::List(elem) => {
                let inner = child_a.cloned().unwrap_or(Ty::Prim(crate::ir::Prim::Str));
                let e = self.member(ir, elem, &inner, false)?;
                let ix = u32::try_from(self.members.len())?;
                self.members.push(e);
                (K_LIST, 0, ix)
            }
            STarget::Map(k, v) => {
                let kt = child_a.cloned().unwrap_or(Ty::Prim(crate::ir::Prim::Str));
                let vt = child_b.cloned().unwrap_or(Ty::Prim(crate::ir::Prim::Str));
                let km = self.member(ir, k, &kt, false)?;
                let vm = self.member(ir, v, &vt, false)?;
                let ix = u32::try_from(self.members.len())?;
                self.members.push(km);
                self.members.push(vm);
                (K_MAP, 0, ix)
            }
        };
        Ok(Mem {
            name: s.name.clone(),
            wire: (s.wire != s.name).then(|| s.wire.clone()),
            xml: s.xml_name.clone(),
            bind,
            bind_str,
            kind,
            flags,
            ts,
            target,
            cons,
        })
    }
}
