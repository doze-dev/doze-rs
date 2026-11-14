//! The seam between the registry and a schema language. The registry (subjects, versions, ids, levels, the REST API) knows
//! nothing of Avro, Protobuf or JSON Schema: each is a [`SchemaFormat`] that reads text into a [`ParsedSchema`], which
//! gives the text the registry stores and says why it cannot read data written with another schema of its language.
//!
//! A language is added by implementing the two traits in a module of its own and listing it in [`Formats::all`]; nothing
//! else in the crate changes (docs/port/phase10.md, "Extension points").

use std::any::Any;
use std::fmt::Debug;

use serde::{Deserialize, Serialize};

/// The three schema languages, as the API names them. AVRO is what an absent `schemaType` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SchemaType {
    Avro,
    Json,
    Protobuf,
}

impl SchemaType {
    /// The language a request names: AVRO when it names none, any case.
    #[must_use]
    pub fn named(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "" | "AVRO" => Some(Self::Avro),
            "JSON" => Some(Self::Json),
            "PROTOBUF" => Some(Self::Protobuf),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Avro => "AVRO",
            Self::Json => "JSON",
            Self::Protobuf => "PROTOBUF",
        }
    }

    /// The order `GET /schemas/types` lists them in, as Confluent does.
    pub const LISTED: [Self; 3] = [Self::Json, Self::Protobuf, Self::Avro];
}

/// A reference followed to the schema it names: what a language needs to read a schema that uses another.
#[derive(Debug, Clone)]
pub struct ResolvedRef {
    /// The name the referring schema uses (an Avro full name, a JSON Schema `$ref`, a Protobuf import path).
    pub name: String,
    pub subject: String,
    pub version: u32,
    pub kind: SchemaType,
    pub text: String,
    /// Whether the schema being read names it itself (and not only through another reference).
    pub direct: bool,
    /// The schema the reference reaches, as `subject\0version`: two references reach one schema when their nodes are equal.
    pub node: String,
    /// What the reached schema's own reference names reach, by node. With these a language can tell one name used
    /// for two schemas in two places (JSON Schema needs it; Avro does not).
    pub names: Vec<(String, String)>,
}

/// Which of the two schemas being compared is the new one: Confluent's sentences say "new" and "old".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    New,
    Old,
}

impl Role {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Old => "old",
        }
    }
}

/// One reason a schema may not follow another: Confluent's type for it, the path in the schema it is about, its sentence,
/// and for Avro the names or types involved. `text` is all of it as Confluent writes it in a 409 or a verbose check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompatReason {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    pub description: String,
    #[serde(rename = "info", skip_serializing_if = "String::is_empty")]
    pub info: String,
    #[serde(skip)]
    pub text: String,
}

impl CompatReason {
    /// A reason that is only a sentence.
    #[must_use]
    pub fn plain(kind: &str, description: String) -> Self {
        Self {
            kind: kind.to_owned(),
            path: String::new(),
            text: description.clone(),
            description,
            info: String::new(),
        }
    }
}

/// A schema language.
pub trait SchemaFormat: Debug + Send + Sync {
    /// The language this reads.
    fn kind(&self) -> SchemaType;

    /// Whether the registry bounds the nesting of the text before it is read ([`crate::json::MAX_SCHEMA_DEPTH`]): true for
    /// the languages written in JSON, whose comparison costs more than linear time in depth.
    fn bounded_depth(&self) -> bool {
        true
    }

    /// Reads `text`. `refs` are every schema it depends on, dependencies before dependents.
    ///
    /// # Errors
    /// The text is not a schema of this language; the message is the details of Confluent's 42201.
    fn parse(&self, text: &str, refs: &[ResolvedRef]) -> Result<Box<dyn ParsedSchema>, String>;
}

/// A schema a language has read.
pub trait ParsedSchema: Debug + Send + Sync + Any {
    /// The text the registry stores and compares: the same schema written two ways is one schema with one id.
    /// `normalize` is the request's `?normalize=true`.
    fn canonical(&self, normalize: bool) -> String;

    /// Why a reader built for this schema cannot read data written with `writer`, a schema of the same language.
    /// `reader_is` and `writer_is` say which of the two is the new schema, for the sentences.
    fn can_read(
        &self,
        writer: &dyn ParsedSchema,
        reader_is: Role,
        writer_is: Role,
    ) -> Vec<CompatReason>;

    /// This schema, for a language to look at a writer of its own kind.
    fn as_any(&self) -> &dyn Any;
}

/// The languages this build reads.
#[derive(Debug)]
pub struct Formats(Vec<Box<dyn SchemaFormat>>);

impl Formats {
    /// Every language the crate has. Protobuf (slice 10b) and JSON Schema (10c) are added here.
    #[must_use]
    pub fn all() -> Self {
        Self(vec![Box::new(crate::avro::Avro)])
    }

    /// Only the languages given (a test's).
    #[must_use]
    pub fn of(formats: Vec<Box<dyn SchemaFormat>>) -> Self {
        Self(formats)
    }

    /// The language of a kind, if this build reads it.
    #[must_use]
    pub fn get(&self, kind: SchemaType) -> Option<&dyn SchemaFormat> {
        self.0.iter().find(|f| f.kind() == kind).map(AsRef::as_ref)
    }

    /// The kinds this build reads, in the order the API lists them.
    #[must_use]
    pub fn kinds(&self) -> Vec<SchemaType> {
        SchemaType::LISTED
            .into_iter()
            .filter(|k| self.get(*k).is_some())
            .collect()
    }
}
