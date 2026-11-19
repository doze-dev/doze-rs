//! Reading a submitted schema: following its references to the schemas they name, then handing the text to its
//! language, with Confluent's sentences when it is refused.

use super::{Registry, SchemaInput, State};
use crate::error::RegistryError;
use crate::format::{ParsedSchema, ResolvedRef, SchemaType};
use crate::json;
use crate::store::Reference;

/// The most distinct schemas one reference graph may name.
pub const MAX_RESOLVED_SCHEMAS: usize = 1000;

/// A schema read for a comparison: its language, what the language made of it, and its references as stored (a
/// request's `-1`, the latest, made the version it named).
pub(super) struct Prepared {
    pub(super) kind: SchemaType,
    pub(super) parsed: Box<dyn ParsedSchema>,
    pub(super) refs: Vec<Reference>,
}

/// Where Confluent refused a schema, which decides how its message is written: while reading the text (`Invalid schema
/// {...} with refs [...] of type T, details: ...`), or while checking what it read (`Invalid schema {...}, details: ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Read,
    Check,
}

/// How a refusal of the language's is written, from its words: Confluent checks a JSON Schema against its draft, and
/// resolves a Protobuf schema's types, after it has read the text.
fn stage(kind: SchemaType, why: &str) -> Stage {
    match kind {
        SchemaType::Json if why.starts_with("Invalid JSON Schema") => Stage::Check,
        SchemaType::Protobuf if !why.starts_with("Could not parse Protobuf") => Stage::Check,
        _ => Stage::Read,
    }
}

/// A reference as Confluent's `SchemaReference.toString` writes it, `null` for what the request left out.
fn reference_text(r: &Reference) -> String {
    let null = |s: &str| {
        if s.is_empty() {
            "null".to_owned()
        } else {
            s.to_owned()
        }
    };
    let version = if r.version == Reference::NO_VERSION {
        "null".to_owned()
    } else {
        r.version.to_string()
    };
    format!(
        "{{name='{}', subject='{}', version={version}}}",
        null(&r.name),
        null(&r.subject)
    )
}

fn references_text(refs: &[Reference]) -> String {
    let all: Vec<String> = refs.iter().map(reference_text).collect();
    format!("[{}]", all.join(", "))
}

/// The refusal of a submitted schema, as Confluent words it: the schema as its `Schema.toString` writes it, then the
/// details.
fn invalid(
    subject: &str,
    input: &SchemaInput,
    kind: SchemaType,
    at: Stage,
    details: &str,
) -> RegistryError {
    let refs = references_text(input.refs());
    let quoted = format!(
        "{{subject={subject},version={},id={},schemaType={},references={refs},metadata={},ruleSet={},schema={},schemaTags=null}}",
        input.version.unwrap_or(0),
        input.id.unwrap_or(-1),
        kind.name(),
        input
            .contract
            .metadata
            .as_ref()
            .map_or_else(|| "null".to_owned(), super::Metadata::java),
        input
            .contract
            .rule_set
            .as_ref()
            .map_or_else(|| "null".to_owned(), super::RuleSet::java),
        input.text(),
    );
    RegistryError::invalid_schema(match at {
        Stage::Read => format!(
            "Invalid schema {quoted} with refs {refs} of type {}, details: {details}",
            kind.name()
        ),
        Stage::Check => format!("Invalid schema {quoted}, details: {details}"),
    })
}

impl State {
    /// A request's own references made the versions they name (`-1` the latest live one; a soft-deleted version may be
    /// named, as Confluent allows), or the details of Confluent's refusal.
    fn concrete(&self, refs: &[Reference]) -> Result<Vec<Reference>, String> {
        refs.iter()
            .map(|r| {
                if r.subject.is_empty() || r.version == Reference::NO_VERSION {
                    return Err(format!("Invalid reference: {}", reference_text(r)));
                }
                let found = if r.version == -1 {
                    self.live(&r.subject).last()
                } else {
                    u32::try_from(r.version)
                        .ok()
                        .and_then(|n| self.find(&r.subject, n))
                };
                found
                    .map(|v| Reference {
                        version: i64::from(v.number),
                        ..r.clone()
                    })
                    .ok_or_else(|| {
                        format!(
                            "No schema reference found for subject \"{}\" and version {}",
                            r.subject, r.version
                        )
                    })
            })
            .collect()
    }

    /// Follows references to the schemas they name, dependencies first. Each (subject, version) is followed once, so a
    /// graph that names one schema from many places (a diamond) costs its size and not its paths. A cycle cannot be
    /// built: a reference names a version that existed before the schema naming it.
    // fuzz:parser registry_api
    pub(super) fn resolve(&self, refs: &[Reference]) -> Result<Vec<ResolvedRef>, String> {
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
    fn refs(&mut self, s: &State, refs: &[Reference], direct: bool) -> Result<(), String> {
        for r in refs {
            let found = u32::try_from(r.version)
                .ok()
                .and_then(|n| s.find(&r.subject, n));
            let Some(v) = found else {
                return Err(format!(
                    "No schema reference found for subject \"{}\" and version {}",
                    r.subject, r.version
                ));
            };
            let Some(rec) = s.schemas.get(&v.rec.id) else {
                continue;
            };
            let node = node_of(r);
            if !self.followed.contains(&node) {
                if self.followed.len() >= MAX_RESOLVED_SCHEMAS {
                    return Err(format!(
                        "references name more than {MAX_RESOLVED_SCHEMAS} schemas"
                    ));
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
    /// Reads a schema submitted to `subject`: its language, what the language made of it, and the text to store.
    /// `normalize` is the request's; the subject's (or the global) `normalize` setting adds to it.
    pub(super) fn prepare(
        &self,
        s: &State,
        subject: &str,
        input: &SchemaInput,
        normalize: bool,
    ) -> Result<(Prepared, String), RegistryError> {
        // The type must be named exactly as Confluent names it (`avro` is no type); none is Avro.
        let kind = match input.schema_type.as_deref() {
            None => SchemaType::Avro,
            Some(t) => SchemaType::LISTED
                .into_iter()
                .find(|k| k.name() == t)
                .ok_or_else(|| RegistryError::invalid_schema(format!("Invalid schema type {t}")))?,
        };
        let format = self.formats.get(kind).ok_or_else(|| {
            RegistryError::invalid_schema(format!(
                "Invalid schema: {} schemas are not read by this build",
                kind.name()
            ))
        })?;
        let text = input.text();
        if text.trim().is_empty() {
            return Err(RegistryError::invalid_schema("Empty schema"));
        }
        if format.bounded_depth() && json::depth(text) > json::MAX_SCHEMA_DEPTH {
            // Said without the schema: echoing megabytes of brackets helps nobody.
            return Err(RegistryError::invalid_schema(format!(
                "Invalid schema: nested more than {} levels deep",
                json::MAX_SCHEMA_DEPTH
            )));
        }
        let read = |why: String| invalid(subject, input, kind, Stage::Read, &why);
        let mut refs = s.concrete(input.refs()).map_err(read)?;
        let resolved = s.resolve(&refs).map_err(read)?;
        let parsed = format
            .parse(text, &resolved)
            .map_err(|why| invalid(subject, input, kind, stage(kind, &why), &why))?;
        let normalize = normalize || s.normalizes(subject);
        if normalize {
            // Every language's `normalize()` sorts its references by subject, then version, and keeps each once.
            refs.sort_by(|a, b| {
                a.subject
                    .encode_utf16()
                    .cmp(b.subject.encode_utf16())
                    .then(a.version.cmp(&b.version))
            });
            let mut seen: Vec<Reference> = Vec::new();
            refs.retain(|r| {
                let first = !seen.contains(r);
                if first {
                    seen.push(r.clone());
                }
                first
            });
        }
        let canonical = parsed.canonical(normalize);
        Ok((Prepared { kind, parsed, refs }, canonical))
    }

    /// The refusal when a stored version cannot be read again, as Confluent words it: that schema quoted as invalid.
    pub(super) fn unreadable(
        &self,
        s: &State,
        subject: &str,
        v: &super::Version,
        why: &str,
    ) -> RegistryError {
        let rec = s.schemas.get(&v.rec.id);
        let kind = rec.map_or(SchemaType::Avro, |r| r.kind);
        let old = SchemaInput {
            schema: rec.map(|r| r.text.clone()),
            schema_type: Some(kind.name().to_owned()),
            references: rec.map(|r| r.refs.clone()),
            id: Some(i64::from(v.rec.id)),
            version: Some(i64::from(v.number)),
            contract: rec.map(|r| r.contract.clone()).unwrap_or_default(),
        };
        invalid(subject, &old, kind, stage(kind, why), why)
    }

    /// Reads a stored schema again, to compare with.
    pub(super) fn parse_stored(&self, s: &State, id: u32) -> Result<Prepared, String> {
        let rec = s.schemas.get(&id).ok_or("no such schema")?;
        let refs = s.resolve(&rec.refs)?;
        let format = self
            .formats
            .get(rec.kind)
            .ok_or("its language is not read by this build")?;
        Ok(Prepared {
            kind: rec.kind,
            parsed: format.parse(&rec.text, &refs)?,
            refs: rec.refs.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_quotes_the_schema_as_confluent_does() {
        let input = SchemaInput {
            schema: Some("x".into()),
            references: Some(vec![Reference {
                name: "n".into(),
                subject: String::new(),
                version: Reference::NO_VERSION,
            }]),
            ..SchemaInput::default()
        };
        // Recorded from cp-schema-registry 7.7.1 (tools/registry-diff).
        assert_eq!(
            invalid("s", &input, SchemaType::Avro, Stage::Read, "D").message,
            "Invalid schema {subject=s,version=0,id=-1,schemaType=AVRO,references=[{name='n', subject='null', version=null}],metadata=null,ruleSet=null,schema=x,schemaTags=null} with refs [{name='n', subject='null', version=null}] of type AVRO, details: D"
        );
        assert_eq!(
            invalid(
                "s",
                &SchemaInput::default(),
                SchemaType::Json,
                Stage::Check,
                "Invalid JSON Schema"
            )
            .message,
            "Invalid schema {subject=s,version=0,id=-1,schemaType=JSON,references=[],metadata=null,ruleSet=null,schema=,schemaTags=null}, details: Invalid JSON Schema"
        );
        assert_eq!(
            stage(SchemaType::Protobuf, "Could not resolve type: X"),
            Stage::Check
        );
        assert_eq!(
            stage(SchemaType::Protobuf, "Could not parse Protobuf - x"),
            Stage::Read
        );
    }
}
