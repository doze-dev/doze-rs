//! Protobuf: `.proto` text (proto2 and proto3) read by a parser of its own (`ast`), linked with its imports and
//! checked by the language's rules (`link`), stored as Confluent prints it (`print`), compared by Confluent's rules
//! with its sentences (`compat`); and schemas sent as a base64 `FileDescriptorProto` (`desc`), as
//! confluent-kafka's serializers register them. No third-party Protobuf crate: what Confluent answers is its own
//! printing and its own rules, which a compiler library does not give, and `protox` with `prost-reflect` would weigh
//! more than the rest of the registry (docs/port/phase10.md, "Weight first").

mod ast;
mod compat;
mod desc;
mod lex;
mod link;
mod print;
mod wkt;

use std::any::Any;

use crate::format::{CompatReason, ParsedSchema, ResolvedRef, Role, SchemaFormat, SchemaType};

/// The Protobuf language.
#[derive(Debug, Clone, Copy, Default)]
pub struct Protobuf;

/// A Protobuf schema the registry has read.
#[derive(Debug)]
pub struct ProtobufSchema {
    stored: String,
    pool: link::Pool,
    /// The schema's own file among the pool's (its imports come before it).
    file: usize,
}

/// Why text is not a schema: where (a byte offset, when known) and what.
#[derive(Debug)]
pub(crate) struct Error {
    at: Option<u32>,
    msg: String,
    syntax: bool,
}

impl Error {
    fn syntax(at: u32, msg: &str) -> Self {
        Self {
            at: Some(at),
            msg: msg.to_owned(),
            syntax: true,
        }
    }

    fn at(at: u32, msg: String) -> Self {
        Self {
            at: Some(at),
            msg,
            syntax: false,
        }
    }

    fn plain(msg: String) -> Self {
        Self {
            at: None,
            msg,
            syntax: false,
        }
    }

    /// The message with its line and column in `src`, as Confluent words a syntax error (`Syntax error in :1:28:`).
    fn render(&self, src: &str) -> String {
        let Some(at) = self.at else {
            return self.msg.clone();
        };
        let before = src.get(..at as usize).unwrap_or(src);
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        let kind = if self.syntax { "Syntax error in " } else { "" };
        format!("{kind}:{line}:{col}: {}", self.msg)
    }
}

impl SchemaFormat for Protobuf {
    fn kind(&self) -> SchemaType {
        SchemaType::Protobuf
    }

    /// The comparison walks messages once each: linear in depth, so the text needs no bound before it is read (the
    /// parser bounds nesting itself).
    fn bounded_depth(&self) -> bool {
        false
    }

    fn parse(&self, text: &str, refs: &[ResolvedRef]) -> Result<Box<dyn ParsedSchema>, String> {
        parse(text, refs).map(|s| Box::new(s) as Box<dyn ParsedSchema>)
    }
}

/// Reads `text`: `.proto` text, or failing that a base64 `FileDescriptorProto`. `refs` are the files it may import, by
/// the names the references give them.
///
/// # Errors
/// Neither reading gives a schema; the message is about the text (or about the descriptor, when it is one).
// fuzz:parser protobuf_schema
pub fn parse(text: &str, refs: &[ResolvedRef]) -> Result<ProtobufSchema, String> {
    let sources: Vec<(&str, &str)> = refs
        .iter()
        .map(|r| (r.name.as_str(), r.text.as_str()))
        .collect();
    let err = match read(text, &sources, false) {
        Ok(s) => return Ok(s),
        Err(e) => e,
    };
    match desc::descriptor_text(text) {
        Some(t) => read(&t, &sources, true),
        None => Err(err),
    }
}

fn read(text: &str, sources: &[(&str, &str)], descriptor: bool) -> Result<ProtobufSchema, String> {
    let ast =
        ast::parse(text).map_err(|e| format!("Could not parse Protobuf - {}", e.render(text)))?;
    let pool = link::link(text, &ast, sources, descriptor)?;
    let file = pool.files.len().saturating_sub(1);
    Ok(ProtobufSchema {
        stored: print::print(text, &ast),
        pool,
        file,
    })
}

impl ProtobufSchema {
    /// The full name of the message a value's message indexes name (`[0]` is the first message of the file, `[1, 0]`
    /// the first message nested in the second), as Confluent's serializers number them.
    #[must_use]
    pub fn message_at(&self, indexes: &[usize]) -> Option<&str> {
        let (first, rest) = indexes.split_first()?;
        let mut m = self
            .pool
            .msg(*self.pool.files.get(self.file)?.msgs.get(*first)?)?;
        for i in rest {
            m = self.pool.msg(*m.nested.get(*i)?)?;
        }
        Some(&m.full)
    }
}

impl ParsedSchema for ProtobufSchema {
    /// Confluent's printing; `normalize` changes nothing for Protobuf here (as in the Go edition).
    fn canonical(&self, _normalize: bool) -> String {
        self.stored.clone()
    }

    fn can_read(
        &self,
        writer: &dyn ParsedSchema,
        reader_is: Role,
        writer_is: Role,
    ) -> Vec<CompatReason> {
        let Some(writer) = writer.as_any().downcast_ref::<Self>() else {
            return Vec::new();
        };
        compat::can_read(
            (&self.pool, self.file),
            (&writer.pool, writer.file),
            reader_is,
            writer_is,
        )
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests;
