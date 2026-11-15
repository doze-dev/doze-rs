//! The schema registry as the console sees it: the registry itself, in this process (never over HTTP to the address it
//! serves), or why there is none. The Schemas pages read and write through it; a record's key or value in the registry's
//! wire format is decoded with it ([`Schemas::decorate`]); the Produce dialog encodes with it.
//!
//! The binary decides which it is at start (`doze-kafka`'s `registry.rs`): on with its address, or off with the reason
//! (switched off, its address taken, its records unreadable, a build without it).

use std::collections::HashMap;
use std::sync::Arc;

use kafka_registry::Registry;
use kafka_registry::codec::{Codec, pretty};

use crate::model::{SchemaShown, Shown};

/// Why there is no registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Off {
    /// `--schema-registry=false`.
    Disabled,
    /// The default address could not be had (another registry on the machine); the words of the bind error.
    Busy(String),
    /// Its records in `meta.redb` could not be read; the words of the error.
    Unreadable(String),
    /// This build has no registry (built without the `registry` feature), or the console runs without a broker's.
    Absent,
}

impl Off {
    /// The page's heading for it.
    #[must_use]
    pub fn title(&self) -> &'static str {
        match self {
            Self::Disabled => "Not running",
            Self::Busy(_) | Self::Unreadable(_) => "Did not start",
            Self::Absent => "Not in this build",
        }
    }

    /// What happened, in words.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Disabled => "doze-kafka was started with the registry off (--schema-registry=false), so nothing listens for it and clients that point schema.registry.url at it get connection refused. This is a choice made at start, not a fault.".to_owned(),
            Self::Busy(why) => format!("Its address could not be had ({why}). The broker runs without it. Give it another with --schema-registry-addr."),
            Self::Unreadable(why) => format!("The registry's records in meta.redb could not be read ({why}). The broker runs without it, and nothing in meta.redb was changed."),
            Self::Absent => "This doze-kafka was built without the schema registry (the `registry` feature), so there is none to show.".to_owned(),
        }
    }
}

/// The registry, or why there is none.
#[derive(Debug, Clone)]
pub enum Schemas {
    On {
        registry: Arc<Registry>,
        /// `http://127.0.0.1:8081`, or `https://` with the broker's certificate.
        url: String,
        /// Clients sign in with the broker's SASL users (HTTP basic authentication).
        basic_auth: bool,
    },
    Off(Off),
}

impl Default for Schemas {
    fn default() -> Self {
        Self::Off(Off::Absent)
    }
}

/// Codecs met while decoding one answer, by id: each schema is read once per request.
#[derive(Debug, Default)]
pub struct Codecs(HashMap<u32, Result<Arc<Codec>, String>>);

/// The User-Agent the console registers with: a version it registered says "this console".
pub const CONSOLE_AGENT: &str = "doze-kafka-console";

/// How many characters a table cell shows of a decoded value.
const PREVIEW: usize = 120;

impl Schemas {
    #[must_use]
    pub fn registry(&self) -> Option<&Arc<Registry>> {
        match self {
            Self::On { registry, .. } => Some(registry),
            Self::Off(_) => None,
        }
    }

    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::On { url, .. } => Some(url),
            Self::Off(_) => None,
        }
    }

    fn codec(&self, codecs: &mut Codecs, id: u32) -> Result<Arc<Codec>, String> {
        let Some(r) = self.registry() else {
            return Err("This console has no schema registry to read it with.".to_owned());
        };
        codecs
            .0
            .entry(id)
            .or_insert_with(|| {
                r.codec(id).map(Arc::new).map_err(|e| {
                    if e.error_code == 40403 {
                        format!("Schema id {id} is not registered here ({}).", e.error_code)
                    } else {
                        e.message
                    }
                })
            })
            .clone()
    }

    /// The subject and version that hold `id`, `<topic>-value` (or `-key`) first.
    fn holder(&self, id: u32, topic: &str, key: bool) -> Option<(String, u32)> {
        let held = self.registry()?.id_versions(id, false).ok()?;
        let named = format!("{topic}-{}", if key { "key" } else { "value" });
        held.iter()
            .find(|(s, _)| *s == named)
            .or_else(|| held.first())
            .cloned()
    }

    /// Reads `s` with its schema, if its bytes are in the wire format: `s.schema` says what they mean, or why they could not
    /// be read. A value kept only in part (a table's) is named, not decoded.
    pub fn decorate(&self, s: &mut Shown, topic: &str, key: bool, codecs: &mut Codecs) {
        let Some(framed) = s.framed.take() else {
            return;
        };
        let Ok((id, _)) = kafka_registry::wire::split(&framed.bytes) else {
            return;
        };
        let mut out = SchemaShown {
            id,
            ..SchemaShown::default()
        };
        match self.codec(codecs, id) {
            Err(why) => {
                out.preview = format!("schema id {id} · not decoded");
                out.error = Some(why);
            }
            Ok(c) => {
                out.kind = Some(c.kind().name().to_owned());
                if let Some((subject, version)) = self.holder(id, topic, key) {
                    out.subject = Some(subject);
                    out.version = Some(version);
                }
                if framed.whole {
                    read(&c, &framed.bytes, &mut out);
                } else {
                    out.preview = format!("schema id {id} · open the record to decode");
                }
            }
        }
        s.schema = Some(out);
    }
}

impl Schemas {
    /// Every row's key and value read with their schemas (each schema read once).
    pub fn decorate_rows(&self, rows: &mut [crate::model::RecordRow], topic: &str) {
        let mut codecs = Codecs::default();
        for r in rows {
            self.decorate(&mut r.key, topic, true, &mut codecs);
            self.decorate(&mut r.value, topic, false, &mut codecs);
        }
    }

    /// A record's key and value read with their schemas.
    pub fn decorate_record(&self, r: &mut crate::model::RecordView) {
        let mut codecs = Codecs::default();
        self.decorate(&mut r.key, &r.topic, true, &mut codecs);
        self.decorate(&mut r.value, &r.topic, false, &mut codecs);
    }
}

fn read(c: &Codec, bytes: &[u8], out: &mut SchemaShown) {
    match c.decode(bytes) {
        Ok(d) => {
            let compact = d.json.to_text();
            out.preview = if compact.chars().count() > PREVIEW {
                format!("{}…", compact.chars().take(PREVIEW).collect::<String>())
            } else {
                compact
            };
            out.json = Some(crate::gather::decode::clean(&pretty(&d.json)));
            out.message = d.message;
            out.mismatch = d.mismatch;
        }
        Err(why) => {
            out.preview = format!("schema id {} · does not decode", c.id());
            out.error = Some(why.to_string());
        }
    }
}

/// The topic a subject is named for (TopicNameStrategy: `<topic>-value`, `<topic>-key`), and which of the two.
#[must_use]
pub fn topic_of(subject: &str) -> Option<(&str, &'static str)> {
    if let Some(t) = subject.strip_suffix("-value").filter(|t| !t.is_empty()) {
        return Some((t, "value"));
    }
    subject
        .strip_suffix("-key")
        .filter(|t| !t.is_empty())
        .map(|t| (t, "key"))
}

/// The topics a subject is used on by its name: TopicNameStrategy (`<topic>-value`, `<topic>-key`) and
/// TopicRecordNameStrategy (`<topic>-<record full name>`), among `topics`. A RecordNameStrategy subject (the record's full
/// name alone) names no topic: the records that carry its ids say where it is used.
#[must_use]
pub fn named_topics(subject: &str, topics: &[String]) -> Vec<(String, &'static str)> {
    if let Some((t, position)) = topic_of(subject)
        && topics.iter().any(|x| x == t)
    {
        return vec![(t.to_owned(), position)];
    }
    let mut out: Vec<(String, &'static str)> = topics
        .iter()
        .filter(|t| {
            subject
                .strip_prefix(t.as_str())
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|record| {
                    record.contains('.') || record.starts_with(char::is_uppercase)
                })
        })
        .map(|t| (t.clone(), "record"))
        .collect();
    out.sort();
    out
}

/// The subject of a topic's values, else of its keys, among `subjects` (the topics list's column).
#[must_use]
pub fn subject_of<'a>(topic: &str, subjects: &'a [String]) -> Option<&'a str> {
    let value = format!("{topic}-value");
    let key = format!("{topic}-key");
    subjects
        .iter()
        .find(|s| **s == value)
        .or_else(|| subjects.iter().find(|s| **s == key))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_name_their_topics_by_the_strategies_clients_use() {
        let topics: Vec<String> = ["orders", "pay", "orders.v2"].map(str::to_owned).to_vec();
        assert_eq!(
            named_topics("orders-value", &topics),
            vec![("orders".to_owned(), "value")]
        );
        assert_eq!(
            named_topics("orders-key", &topics),
            vec![("orders".to_owned(), "key")]
        );
        assert_eq!(
            named_topics("pay-shop.Payment", &topics),
            vec![("pay".to_owned(), "record")]
        );
        assert!(named_topics("shop.Order", &topics).is_empty());
        assert!(named_topics("gone-value", &topics).is_empty());
        assert!(named_topics("-value", &topics).is_empty());
        let subjects: Vec<String> = ["orders-key", "orders-value", "pay-key"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(subject_of("orders", &subjects), Some("orders-value"));
        assert_eq!(subject_of("pay", &subjects), Some("pay-key"));
        assert_eq!(subject_of("x", &subjects), None);
    }

    #[test]
    fn off_says_why_in_words() {
        assert!(Off::Disabled.reason().contains("--schema-registry=false"));
        assert!(
            Off::Unreadable("bad record".into())
                .reason()
                .contains("(bad record)")
        );
        assert_eq!(Off::Absent.title(), "Not in this build");
    }
}

/// How a client reaches the schema registry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RegistryConnect {
    pub url: String,
    /// It asks for the broker's SASL users as HTTP basic authentication.
    pub basic_auth: bool,
    /// The lines a serializer's properties take.
    pub properties: String,
    /// `SCHEMA_REGISTRY_URL=...`.
    pub environment: String,
}

impl RegistryConnect {
    #[must_use]
    pub fn new(url: &str, basic_auth: bool) -> Self {
        let mut properties = format!("schema.registry.url={url}");
        if basic_auth {
            properties.push_str(
                "\nbasic.auth.credentials.source=USER_INFO\nbasic.auth.user.info=<user>:<password>",
            );
        }
        Self {
            url: url.to_owned(),
            basic_auth,
            properties,
            environment: format!("SCHEMA_REGISTRY_URL={url}"),
        }
    }
}
