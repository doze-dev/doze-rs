//! Resolves a shape to a type: a primitive, a named type, or a list or map of those. A shape that carries
//! constraints (or is a structure, union or enum) becomes a named type in the IR.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

use super::build::Builder;
use super::{EnumVariant, Prim, Ty, TypeDef, TypeKind};
use crate::model::{Constraints, MemberDef, Shape, Traits, TraitsExt, local_name};
use crate::names::type_name;

/// Arguments of [`Builder::simple_named`].
struct Simple<'a, 'b> {
    id: &'a str,
    ctx: Option<&'a MemberCtx<'b>>,
    overridden: bool,
    c: Constraints,
}

/// What a member says about itself, passed down so member-level overrides can be honoured.
pub(super) struct MemberCtx<'a> {
    pub(super) parent: &'a str,
    pub(super) name: &'a str,
    pub(super) traits: &'a Traits,
}

impl Builder<'_> {
    /// Resolves a whole shape (no member context).
    pub(super) fn ty_of_shape(&mut self, id: &str) -> Result<Ty> {
        self.ty_of(id, None)
    }

    /// The constraints in force: the shape's own, overridden by the member's. Also whether the member overrode any.
    fn effective(own: &Traits, member: Option<&Traits>) -> Result<(Constraints, bool)> {
        let base = Constraints::from_traits(own)?;
        match member {
            Some(mt) => {
                let m = Constraints::from_traits(mt)?;
                let overridden = !m.is_empty();
                Ok((base.overridden_by(&m), overridden))
            }
            None => Ok((base, false)),
        }
    }

    pub(super) fn ty_of(&mut self, id: &str, ctx: Option<&MemberCtx<'_>>) -> Result<Ty> {
        let shape = self.shape(id)?;
        let mt = ctx.map(|c| c.traits);
        Ok(match shape {
            Shape::Boolean(_) => Ty::Prim(Prim::Bool),
            Shape::Timestamp(_) => Ty::Prim(Prim::Timestamp),
            Shape::Byte(s) | Shape::Short(s) | Shape::Integer(s) | Shape::Long(s) => {
                let (c, over) = Self::effective(&s.traits, mt)?;
                let s = Simple {
                    id,
                    ctx,
                    overridden: over,
                    c,
                };
                self.simple_named(s, TypeKind::IntNewtype, Ty::Prim(Prim::Int))?
            }
            Shape::Float(s) | Shape::Double(s) => {
                let (c, over) = Self::effective(&s.traits, mt)?;
                let s = Simple {
                    id,
                    ctx,
                    overridden: over,
                    c,
                };
                self.simple_named(s, TypeKind::FloatNewtype, Ty::Prim(Prim::Float))?
            }
            Shape::String(s) => {
                let (c, over) = Self::effective(&s.traits, mt)?;
                let s = Simple {
                    id,
                    ctx,
                    overridden: over,
                    c,
                };
                self.simple_named(s, TypeKind::StrNewtype, Ty::Prim(Prim::Str))?
            }
            Shape::Blob(s) => {
                if s.traits.has("streaming") {
                    return Ok(Ty::Prim(Prim::Stream));
                }
                let (c, over) = Self::effective(&s.traits, mt)?;
                let s = Simple {
                    id,
                    ctx,
                    overridden: over,
                    c,
                };
                self.simple_named(s, TypeKind::BlobNewtype, Ty::Prim(Prim::Blob))?
            }
            Shape::Enum(e) => {
                let name = type_name(local_name(id));
                if !self.types.contains_key(&name) {
                    let variants = self.enum_variants(id, &e.members)?;
                    self.insert(name.clone(), id, TypeKind::Enum(variants))?;
                }
                Ty::Named(name)
            }
            Shape::List(l) => {
                let (c, over) = Self::effective(&l.traits, mt)?;
                self.list(id, ctx, over, c, &l.member)?
            }
            Shape::Map(m) => {
                let (c, over) = Self::effective(&m.traits, mt)?;
                self.map(id, ctx, over, c, m)?
            }
            Shape::Structure(s) => {
                let name = type_name(local_name(id));
                self.structure(id, &name, s)?;
                Ty::Named(name)
            }
            Shape::Union(u) => {
                let name = type_name(local_name(id));
                self.union(id, &name, u)?;
                if u.traits.has("streaming") {
                    return Ok(Ty::Prim(Prim::EventStream));
                }
                Ty::Named(name)
            }
            Shape::Document(_)
            | Shape::BigInteger(_)
            | Shape::BigDecimal(_)
            | Shape::IntEnum(_) => {
                bail!("unsupported shape type for {id}")
            }
            Shape::Service(_) | Shape::Operation(_) | Shape::Resource(_) => {
                bail!("{id} is not a data shape")
            }
        })
    }

    fn list(
        &mut self,
        id: &str,
        ctx: Option<&MemberCtx<'_>>,
        overridden: bool,
        c: Constraints,
        member: &MemberDef,
    ) -> Result<Ty> {
        let name = Self::aggregate_name(id, ctx, overridden);
        let elem_ctx = MemberCtx {
            parent: &name,
            name: "member",
            traits: &member.traits,
        };
        let elem = self.ty_of(&member.target, Some(&elem_ctx))?;
        if c.is_empty() {
            return Ok(Ty::Vec(Box::new(elem)));
        }
        self.insert_once(name.clone(), id, TypeKind::ListNewtype(elem, c))?;
        Ok(Ty::Named(name))
    }

    fn map(
        &mut self,
        id: &str,
        ctx: Option<&MemberCtx<'_>>,
        overridden: bool,
        c: Constraints,
        m: &crate::model::MapDef,
    ) -> Result<Ty> {
        let name = Self::aggregate_name(id, ctx, overridden);
        let kctx = MemberCtx {
            parent: &name,
            name: "key",
            traits: &m.key.traits,
        };
        let key = self.ty_of(&m.key.target, Some(&kctx))?;
        let vctx = MemberCtx {
            parent: &name,
            name: "value",
            traits: &m.value.traits,
        };
        let value = self.ty_of(&m.value.target, Some(&vctx))?;
        if c.is_empty() {
            return Ok(Ty::Map(Box::new(key), Box::new(value)));
        }
        self.insert_once(name.clone(), id, TypeKind::MapNewtype(key, value, c))?;
        Ok(Ty::Named(name))
    }

    /// Name of a list/map newtype: the shape name, or `<Parent><Member>` when the member
    /// overrides the shape's constraints.
    fn aggregate_name(id: &str, ctx: Option<&MemberCtx<'_>>, overridden: bool) -> String {
        match ctx {
            Some(c) if overridden => format!("{}{}", c.parent, type_name(c.name)),
            _ => type_name(local_name(id)),
        }
    }

    fn simple_named(
        &mut self,
        s: Simple<'_, '_>,
        kind: impl FnOnce(Constraints) -> TypeKind,
        plain: Ty,
    ) -> Result<Ty> {
        let Simple {
            id,
            ctx,
            overridden,
            c,
        } = s;
        if c.is_empty() {
            return Ok(plain);
        }
        let name = Self::aggregate_name(id, ctx, overridden);
        self.insert_once(name.clone(), id, kind(c))?;
        Ok(Ty::Named(name))
    }

    fn insert_once(&mut self, name: String, id: &str, kind: TypeKind) -> Result<()> {
        if self.types.contains_key(&name) {
            return Ok(());
        }
        self.insert(name, id, kind)
    }

    pub(super) fn insert(&mut self, name: String, id: &str, kind: TypeKind) -> Result<()> {
        if let Some(prev) = self.by_shape.get(&name)
            && prev != id
        {
            bail!("type name collision: {name} is both {prev} and {id}");
        }
        self.by_shape.insert(name.clone(), id.to_owned());
        self.types.insert(name.clone(), TypeDef { name, kind });
        Ok(())
    }

    fn enum_variants(
        &self,
        id: &str,
        members: &indexmap::IndexMap<String, MemberDef>,
    ) -> Result<Vec<EnumVariant>> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for (name, m) in members {
            let wire = m
                .traits
                .str_trait("enumValue")
                .map_or_else(|| name.clone(), str::to_owned);
            if seen.insert(wire.clone()) {
                out.push(EnumVariant { wire });
            }
        }
        if out.is_empty() {
            bail!("enum {id} has no members");
        }
        Ok(out)
    }
}
