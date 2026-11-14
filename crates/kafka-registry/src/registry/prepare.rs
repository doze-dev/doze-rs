//! Reading a submitted schema: following its references to the schemas they name, then handing the text to its
//! language.

use super::{Registry, SchemaInput, State};
use crate::error::RegistryError;
use crate::format::{ParsedSchema, ResolvedRef, SchemaType};
use crate::json;
use crate::store::Reference;

/// The most distinct schemas one reference graph may name.
pub const MAX_RESOLVED_SCHEMAS: usize = 1000;

/// A schema read for a comparison: its language and what the language made of it.
pub(super) struct Prepared {
    pub(super) kind: SchemaType,
    pub(super) parsed: Box<dyn ParsedSchema>,
}

impl State {
    /// Follows references to the schemas they name, dependencies first. Each (subject, version) is followed once, so a
    /// graph that names one schema from many places (a diamond) costs its size and not its paths. A cycle cannot be
    /// built: a reference names a version that existed before the schema naming it.
    // fuzz:parser registry_api
    pub(super) fn resolve(&self, refs: &[Reference]) -> Result<Vec<ResolvedRef>, RegistryError> {
        let mut walk = Walk {
            out: Vec::new(),
            followed: Vec::new(),
        };
        walk.refs(self, refs, true)?;
        Ok(walk.out)
    }
}

struct Walk {
    out: Vec<ResolvedRef>,
    followed: Vec<String>,
}

fn node_of(r: &Reference) -> String {
    format!("{}\0{}", r.subject, r.version)
}

impl Walk {
    fn refs(&mut self, s: &State, refs: &[Reference], direct: bool) -> Result<(), RegistryError> {
        for r in refs {
            let found = u32::try_from(r.version)
                .ok()
                .and_then(|n| s.find(&r.subject, n));
            let Some(v) = found.filter(|v| !v.rec.deleted) else {
                return Err(RegistryError::invalid_schema(format!(
                    "Invalid schema: reference \"{}\": subject '{}' version {} was not found",
                    r.name, r.subject, r.version
                )));
            };
            let Some(rec) = s.schemas.get(&v.rec.id) else {
                continue;
            };
            let node = node_of(r);
            if !self.followed.contains(&node) {
                if self.followed.len() >= MAX_RESOLVED_SCHEMAS {
                    return Err(RegistryError::invalid_schema(format!(
                        "Invalid schema: references name more than {MAX_RESOLVED_SCHEMAS} schemas"
                    )));
                }
                self.followed.push(node.clone());
                self.refs(s, &rec.refs, false)?;
            }
            if let Some(seen) = self
                .out
                .iter_mut()
                .find(|o| o.node == node && o.name == r.name)
            {
                seen.direct |= direct;
                continue;
            }
            self.out.push(ResolvedRef {
                name: r.name.clone(),
                subject: r.subject.clone(),
                version: v.number,
                kind: rec.kind,
                text: rec.text.clone(),
                direct,
                node,
                names: rec
                    .refs
                    .iter()
                    .map(|x| (x.name.clone(), node_of(x)))
                    .collect(),
            });
        }
        Ok(())
    }
}

impl Registry {
    /// Reads a submitted schema: its language, what the language made of it, and the text to store.
    pub(super) fn prepare(
        &self,
        s: &State,
        input: &SchemaInput,
        normalize: bool,
    ) -> Result<(Prepared, String), RegistryError> {
        let typ = input.schema_type.as_deref().unwrap_or_default();
        let kind = SchemaType::named(typ)
            .ok_or_else(|| RegistryError::invalid_schema(format!("Invalid schema type {typ}")))?;
        let format = self.formats.get(kind).ok_or_else(|| {
            RegistryError::invalid_schema(format!(
                "Invalid schema: {} schemas are not read by this build",
                kind.name()
            ))
        })?;
        let text = input.text();
        if text.trim().is_empty() {
            return Err(RegistryError::invalid_schema(
                "Invalid schema: schema may not be empty",
            ));
        }
        if format.bounded_depth() && json::depth(text) > json::MAX_SCHEMA_DEPTH {
            // Said without the schema: echoing megabytes of brackets helps nobody.
            return Err(RegistryError::invalid_schema(format!(
                "Invalid schema: nested more than {} levels deep",
                json::MAX_SCHEMA_DEPTH
            )));
        }
        let refs = s.resolve(input.refs())?;
        let parsed = format.parse(text, &refs).map_err(|why| {
            RegistryError::invalid_schema(format!(
                "Invalid schema {}, details: {why}",
                describe(input, kind)
            ))
        })?;
        let canonical = parsed.canonical(normalize);
        Ok((Prepared { kind, parsed }, canonical))
    }

    /// Reads a stored schema again, to compare with.
    pub(super) fn parse_stored(&self, s: &State, id: u32) -> Result<Prepared, String> {
        let rec = s.schemas.get(&id).ok_or("no such schema")?;
        let refs = s.resolve(&rec.refs).map_err(|e| e.message)?;
        let format = self
            .formats
            .get(rec.kind)
            .ok_or("its language is not read by this build")?;
        Ok(Prepared {
            kind: rec.kind,
            parsed: format.parse(&rec.text, &refs)?,
        })
    }
}

/// The schema as Confluent's message quotes it.
fn describe(input: &SchemaInput, kind: SchemaType) -> String {
    let refs = input
        .refs()
        .iter()
        .map(|r| {
            format!(
                "io.confluent.kafka.schemaregistry.client.rest.entities.SchemaReference@0{{name={}, subject={}, version={}}}",
                r.name, r.subject, r.version
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{schema={}, schemaType={}, references=[{refs}]}}",
        input.text(),
        kind.name()
    )
}
