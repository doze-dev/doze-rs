//! Avro compatibility as Confluent's registry decides it: Avro's own reader/writer rules
//! (`org.apache.avro.SchemaCompatibility`), each refusal a typed finding with the path in the reader schema where it was
//! found, and Confluent's sentence for each type. Held to Confluent's verdicts and reasons for every pair of the recorded
//! corpus (`tests/data/compat/avro.json`, `avro-reasons.json`).

use std::collections::{HashMap, HashSet};

use super::schema::{NamedKind, Schema, Ty};
use crate::format::{CompatReason, Role};

/// One reason a reader schema cannot read what a writer wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    /// Avro's name for it: `TYPE_MISMATCH`, `NAME_MISMATCH`, ...
    pub(crate) kind: &'static str,
    /// A JSON pointer into the reader schema.
    pub(crate) path: String,
    /// Avro's message: the names, sizes, symbols or types involved.
    pub(crate) info: String,
}

/// Why `reader` cannot read data written with `writer`.
pub(crate) fn incompatibilities(reader: &Schema, writer: &Schema) -> Vec<Finding> {
    let (Some(r), Some(w)) = (&reader.root, &writer.root) else {
        return Vec::new();
    };
    let mut c = Checker {
        reader,
        writer,
        memo: HashMap::new(),
        busy: HashSet::new(),
        path: Vec::new(),
    };
    c.check(r, w)
}

/// Walks a reader and a writer schema together. A pair of named types is compared once (a recursive type stops there),
/// as Avro memoizes them.
struct Checker<'a> {
    reader: &'a Schema,
    writer: &'a Schema,
    memo: HashMap<(usize, usize), Vec<Finding>>,
    busy: HashSet<(usize, usize)>,
    path: Vec<String>,
}

/// What kind of type a type is, the way the rules ask: an error is a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Null,
    Boolean,
    Int,
    Long,
    Float,
    Double,
    Bytes,
    String,
    Array,
    Map,
    Union,
    Record,
    Enum,
    Fixed,
}

impl Kind {
    fn java_name(self) -> &'static str {
        match self {
            Self::Null => "NULL",
            Self::Boolean => "BOOLEAN",
            Self::Int => "INT",
            Self::Long => "LONG",
            Self::Float => "FLOAT",
            Self::Double => "DOUBLE",
            Self::Bytes => "BYTES",
            Self::String => "STRING",
            Self::Array => "ARRAY",
            Self::Map => "MAP",
            Self::Union => "UNION",
            Self::Record => "RECORD",
            Self::Enum => "ENUM",
            Self::Fixed => "FIXED",
        }
    }
}

fn kind(s: &Schema, t: &Ty) -> Kind {
    match t {
        Ty::Null => Kind::Null,
        Ty::Boolean => Kind::Boolean,
        Ty::Int => Kind::Int,
        Ty::Long => Kind::Long,
        Ty::Float => Kind::Float,
        Ty::Double => Kind::Double,
        Ty::Bytes => Kind::Bytes,
        Ty::String => Kind::String,
        Ty::Array(_) => Kind::Array,
        Ty::Map(_) => Kind::Map,
        Ty::Union(_) => Kind::Union,
        Ty::Named(i) => match s.named(*i).map(|n| &n.kind) {
            Some(NamedKind::Enum { .. }) => Kind::Enum,
            Some(NamedKind::Fixed(_)) => Kind::Fixed,
            _ => Kind::Record,
        },
    }
}

/// A path token as a JSON pointer writes it.
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

impl Checker<'_> {
    fn at(&mut self, token: &str, f: impl FnOnce(&mut Self) -> Vec<Finding>) -> Vec<Finding> {
        self.path.push(escape(token));
        let out = f(self);
        self.path.pop();
        out
    }

    fn here(&self, kind: &'static str, info: String) -> Vec<Finding> {
        vec![Finding {
            kind,
            path: format!("/{}", self.path.join("/")),
            info,
        }]
    }

    /// Compares a pair. A pair of named types is remembered with its answer, which is given again (with the path it was
    /// first found at) wherever the pair comes back; a pair met again while it is still being compared is compatible.
    fn check(&mut self, r: &Ty, w: &Ty) -> Vec<Finding> {
        let (Ty::Named(ri), Ty::Named(wi)) = (r, w) else {
            return self.calculate(r, w);
        };
        let key = (*ri, *wi);
        if self.busy.contains(&key) {
            return Vec::new();
        }
        if let Some(found) = self.memo.get(&key) {
            return found.clone();
        }
        self.busy.insert(key);
        let found = self.calculate(r, w);
        self.busy.remove(&key);
        self.memo.insert(key, found.clone());
        found
    }

    fn calculate(&mut self, r: &Ty, w: &Ty) -> Vec<Finding> {
        let (rk, wk) = (kind(self.reader, r), kind(self.writer, w));
        if rk == wk {
            return self.same_kind(r, w);
        }
        if let Ty::Union(branches) = w {
            // Each branch the writer may have written is a question of its own: the Avro Confluent runs starts a new
            // path for it, so what it finds there is reported at "/" (recorded: "widen to union").
            let saved = std::mem::take(&mut self.path);
            let out = branches.iter().flat_map(|wb| self.check(r, wb)).collect();
            self.path = saved;
            return out;
        }
        let promoted = match rk {
            Kind::Long => wk == Kind::Int,
            Kind::Float => matches!(wk, Kind::Int | Kind::Long),
            Kind::Double => matches!(wk, Kind::Int | Kind::Long | Kind::Float),
            Kind::Bytes => wk == Kind::String,
            Kind::String => wk == Kind::Bytes,
            Kind::Union => {
                let Ty::Union(branches) = r else {
                    return Vec::new();
                };
                if branches.iter().any(|rb| self.check(rb, w).is_empty()) {
                    return Vec::new();
                }
                return self.here(
                    "MISSING_UNION_BRANCH",
                    format!("reader union lacking writer type: {}", wk.java_name()),
                );
            }
            _ => false,
        };
        if promoted {
            return Vec::new();
        }
        self.here(
            "TYPE_MISMATCH",
            format!(
                "reader type: {} not compatible with writer type: {}",
                rk.java_name(),
                wk.java_name()
            ),
        )
    }

    fn same_kind(&mut self, r: &Ty, w: &Ty) -> Vec<Finding> {
        match (r, w) {
            (Ty::Array(ri), Ty::Array(wi)) => self.at("items", |c| c.check(ri, wi)),
            (Ty::Map(rv), Ty::Map(wv)) => self.at("values", |c| c.check(rv, wv)),
            (Ty::Union(_), Ty::Union(wbs)) => {
                // Every branch the writer may have written must be readable.
                let mut out = Vec::new();
                for (i, wb) in wbs.iter().enumerate() {
                    out.extend(self.at(&i.to_string(), |c| {
                        if c.check(r, wb).is_empty() {
                            return Vec::new();
                        }
                        let wk = kind(c.writer, wb);
                        c.here(
                            "MISSING_UNION_BRANCH",
                            format!("reader union lacking writer type: {}", wk.java_name()),
                        )
                    }));
                }
                out
            }
            (Ty::Named(ri), Ty::Named(wi)) => self.named(*ri, *wi),
            _ => Vec::new(), // the same primitive
        }
    }

    fn named(&mut self, ri: usize, wi: usize) -> Vec<Finding> {
        let (Some(rn), Some(wn)) = (self.reader.named(ri), self.writer.named(wi)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if rn.name != wn.name && !rn.aliases.contains(&wn.full) {
            let expected = format!("expected: {}", wn.full);
            out.extend(self.at("name", |c| c.here("NAME_MISMATCH", expected)));
        }
        match (&rn.kind, &wn.kind) {
            (NamedKind::Fixed(rs), NamedKind::Fixed(ws)) if rs != ws => {
                let info = format!("expected: {ws}, found: {rs}");
                out.extend(self.at("size", |c| c.here("FIXED_SIZE_MISMATCH", info)));
            }
            (
                NamedKind::Enum {
                    symbols: rs,
                    has_default,
                },
                NamedKind::Enum { symbols: ws, .. },
            ) => {
                let mut missing: Vec<&String> = ws.iter().filter(|s| !rs.contains(s)).collect();
                if !missing.is_empty() && !has_default {
                    missing.sort();
                    let info = format!(
                        "[{}]",
                        missing
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    out.extend(self.at("symbols", |c| c.here("MISSING_ENUM_SYMBOLS", info)));
                }
            }
            (NamedKind::Record(rf), NamedKind::Record(_)) => {
                let rf = rf.clone();
                out.extend(self.at("fields", |c| c.fields(&rf, wi)));
            }
            _ => {}
        }
        out
    }

    /// Each reader field reads the writer field of its name (or of one of its aliases), or has a default to read when
    /// there is none.
    fn fields(&mut self, reader_fields: &[super::schema::Field], wi: usize) -> Vec<Finding> {
        let writer_fields = match self.writer.named(wi).map(|n| &n.kind) {
            Some(NamedKind::Record(f)) => f.clone(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for (i, rf) in reader_fields.iter().enumerate() {
            out.extend(self.at(&i.to_string(), |c| {
                let wf = writer_fields
                    .iter()
                    .find(|wf| wf.name == rf.name)
                    .or_else(|| {
                        rf.aliases
                            .iter()
                            .find_map(|a| writer_fields.iter().find(|wf| &wf.name == a))
                    });
                if let Some(wf) = wf {
                    return c.at("type", |c| c.check(&rf.ty, &wf.ty));
                }
                if rf.has_default {
                    return Vec::new();
                }
                if let Ty::Named(e) = rf.ty
                    && matches!(
                        c.reader.named(e).map(|n| &n.kind),
                        Some(NamedKind::Enum {
                            has_default: true,
                            ..
                        })
                    )
                {
                    // Avro compares such a field's enum with the whole writer record.
                    return c.at("type", |c| c.check(&rf.ty, &Ty::Named(wi)));
                }
                c.here("READER_FIELD_MISSING_DEFAULT_VALUE", rf.name.clone())
            }));
        }
        out
    }
}

/// A finding with Confluent's sentence for it, reader and writer named as the new or the old schema.
pub(crate) fn reason(f: &Finding, reader_is: Role, writer_is: Role) -> CompatReason {
    let (r, w, p) = (reader_is.word(), writer_is.word(), &f.path);
    let description = match f.kind {
        "READER_FIELD_MISSING_DEFAULT_VALUE" => format!(
            "The field '{}' at path '{p}' in the {r} schema has no default value and is missing in the {w} schema",
            f.info
        ),
        "TYPE_MISMATCH" => {
            format!(
                "The type (path '{p}') of a field in the {r} schema does not match with the {w} schema"
            )
        }
        "NAME_MISMATCH" => format!("The name of the schema has changed (path '{p}')"),
        "FIXED_SIZE_MISMATCH" => format!(
            "The size of FIXED type field at path '{p}' in the {r} schema does not match with the {w} schema"
        ),
        "MISSING_ENUM_SYMBOLS" => format!(
            "The {r} schema is missing enum symbols '{}' at path '{p}' in the {w} schema",
            f.info
        ),
        "MISSING_UNION_BRANCH" => {
            format!(
                "The {r} schema is missing a type inside a union field at path '{p}' in the {w} schema"
            )
        }
        _ => String::new(),
    };
    CompatReason {
        kind: f.kind.to_owned(),
        path: f.path.clone(),
        text: format!(
            "{{errorType:'{}', description:'{description}', additionalInfo:'{}'}}",
            f.kind, f.info
        ),
        description,
        info: f.info.clone(),
    }
}
