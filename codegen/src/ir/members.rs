//! Structures, unions and the wire metadata of their members.

use anyhow::{Context, Result, bail};

use super::build::{Builder, ts_of};
use super::shapes::MemberCtx;
use super::{Field, SBinding, SMember, STarget, Ts, TypeKind, UnionVariant};
use crate::model::{Aggregate, MemberDef, Shape, TraitsExt, local_name};
use crate::names::type_name;

impl Builder<'_> {
    pub(super) fn structure(&mut self, id: &str, name: &str, s: &Aggregate) -> Result<()> {
        if self.types.contains_key(name) || !self.in_progress.insert(name.to_owned()) {
            return Ok(());
        }
        if !s.mixins.is_empty() {
            bail!("{id}: mixins are not supported");
        }
        let mut fields = Vec::new();
        for (member, def) in &s.members {
            fields.push(self.field(id, name, member, def)?);
        }
        let error = self.error_def(id, &s.traits)?;
        self.in_progress.remove(name);
        let xml_name = s.traits.str_trait("xmlName").map(str::to_owned);
        self.insert(
            name.to_owned(),
            id,
            TypeKind::Struct {
                fields,
                error,
                xml_name,
            },
        )
    }

    fn field(&mut self, id: &str, parent: &str, member: &str, def: &MemberDef) -> Result<Field> {
        let ctx = MemberCtx {
            parent,
            name: member,
            traits: &def.traits,
        };
        let ty = self
            .ty_of(&def.target, Some(&ctx))
            .with_context(|| format!("{id}${member}"))?;
        let target_shape = self.shape(&def.target)?;
        let default = def
            .traits
            .get_trait("default")
            .or_else(|| target_shape.traits().get_trait("default"))
            .filter(|v| !v.is_null())
            .cloned();
        let optional = !(def.traits.has("required") || default.is_some());
        let wire = self.wire_name(member, def);
        let ts = ts_of(&def.traits).or_else(|| ts_of(target_shape.traits()));
        let schema = self.smember(member, &wire, def, ts)?;
        Ok(Field {
            ty,
            optional,
            default,
            schema,
        })
    }

    pub(super) fn union(&mut self, id: &str, name: &str, u: &Aggregate) -> Result<()> {
        if self.types.contains_key(name) || !self.in_progress.insert(name.to_owned()) {
            return Ok(());
        }
        let mut variants = Vec::new();
        for (member, def) in &u.members {
            let ctx = MemberCtx {
                parent: name,
                name: member,
                traits: &def.traits,
            };
            let ty = if def.target == "smithy.api#Unit" {
                None
            } else {
                Some(
                    self.ty_of(&def.target, Some(&ctx))
                        .with_context(|| format!("{id}${member}"))?,
                )
            };
            let wire = self.wire_name(member, def);
            let target_shape = self.shape(&def.target).ok();
            let ts = ts_of(&def.traits).or_else(|| target_shape.and_then(|s| ts_of(s.traits())));
            let schema = self.smember(member, &wire, def, ts)?;
            variants.push(UnionVariant { ty, schema });
        }
        self.in_progress.remove(name);
        self.insert(name.to_owned(), id, TypeKind::Union(variants))
    }

    /// The key a member has in a JSON body: `@jsonName` for restJson1, else its name.
    fn wire_name(&self, member: &str, def: &MemberDef) -> String {
        if self.json_wire {
            def.traits
                .str_trait("jsonName")
                .unwrap_or(member)
                .to_owned()
        } else {
            member.to_owned()
        }
    }

    /// Wire metadata for a member (structure/union member, list element, map key or value).
    fn smember(
        &mut self,
        name: &str,
        wire: &str,
        def: &MemberDef,
        ts: Option<Ts>,
    ) -> Result<SMember> {
        let t = &def.traits;
        let binding = self.binding(t);
        let target = self.starget(&def.target, ts)?;
        Ok(SMember {
            name: name.to_owned(),
            wire: wire.to_owned(),
            xml_name: t.str_trait("xmlName").map(str::to_owned),
            xml_attribute: t.has("xmlAttribute"),
            flattened: t.has("xmlFlattened"),
            required: t.has("required")
                && matches!(binding, SBinding::Query(_) | SBinding::Header(_)),
            binding,
            target,
        })
    }

    fn binding(&self, t: &crate::model::Traits) -> SBinding {
        if !self.rest {
            SBinding::Body
        } else if t.has("httpLabel") {
            SBinding::Label
        } else if let Some(q) = t.str_trait("httpQuery") {
            SBinding::Query(q.to_owned())
        } else if t.has("httpQueryParams") {
            SBinding::QueryParams
        } else if let Some(h) = t.str_trait("httpHeader") {
            SBinding::Header(h.to_owned())
        } else if let Some(p) = t.str_trait("httpPrefixHeaders") {
            SBinding::PrefixHeaders(p.to_owned())
        } else if t.has("httpPayload") {
            SBinding::Payload
        } else if t.has("httpResponseCode") {
            SBinding::ResponseCode
        } else if t.has("hostLabel") {
            SBinding::HostLabel
        } else {
            SBinding::Body
        }
    }

    fn starget(&mut self, id: &str, ts: Option<Ts>) -> Result<STarget> {
        let shape = self.shape(id)?;
        Ok(match shape {
            Shape::Boolean(_) => STarget::Bool,
            Shape::Byte(_) | Shape::Short(_) | Shape::Integer(_) | Shape::Long(_) => STarget::Int,
            Shape::Float(_) | Shape::Double(_) => STarget::Float,
            Shape::String(_) | Shape::Enum(_) => STarget::Str,
            Shape::Blob(s) if s.traits.has("streaming") => STarget::Stream,
            Shape::Blob(_) => STarget::Blob,
            Shape::Timestamp(s) => STarget::Timestamp(ts.or_else(|| ts_of(&s.traits))),
            Shape::List(l) => {
                let m = &l.member;
                STarget::List(Box::new(self.smember(
                    "member",
                    "member",
                    m,
                    ts_of(&m.traits),
                )?))
            }
            Shape::Map(m) => {
                let (k, v) = (&m.key, &m.value);
                STarget::Map(
                    Box::new(self.smember("key", "key", k, ts_of(&k.traits))?),
                    Box::new(self.smember("value", "value", v, ts_of(&v.traits))?),
                )
            }
            Shape::Structure(_) => STarget::Struct(type_name(local_name(id))),
            Shape::Union(u) if u.traits.has("streaming") => STarget::EventStream,
            Shape::Union(_) => STarget::Union(type_name(local_name(id))),
            other => bail!("unsupported schema target {id}: {other:?}"),
        })
    }
}
