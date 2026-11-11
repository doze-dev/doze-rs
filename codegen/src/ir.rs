//! Resolves a service's reachable shapes into the intermediate form the tables are written from.
//!
//! The IR is where the model's quirks are settled once: which shapes carry constraints, what a member is
//! called on the wire, how it is bound to HTTP, which members are required, what a union holds.
//! Everything the tables do not need (documentation, Rust field names, boxing) is left out.

mod build;
mod members;
mod shapes;

use std::collections::BTreeMap;

use crate::model::Constraints;

pub use build::build;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    AwsJson10,
    AwsJson11,
    AwsQuery,
    RestJson1,
    RestXml,
    Rpcv2Cbor,
}

impl Protocol {
    pub fn is_rest(self) -> bool {
        matches!(self, Self::RestJson1 | Self::RestXml)
    }
}

/// The Rust-side type of a member: a primitive, a named type, or a list or map of those.
#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Prim(Prim),
    Named(String),
    Vec(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Bool,
    Int,
    Float,
    Str,
    Blob,
    Timestamp,
    /// `@streaming` blob.
    Stream,
    /// `@streaming` union.
    EventStream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ts {
    EpochSeconds,
    DateTime,
    HttpDate,
}

/// What a member points at, as the wire sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum STarget {
    Bool,
    Int,
    Float,
    Str,
    Blob,
    Stream,
    EventStream,
    Timestamp(Option<Ts>),
    List(Box<SMember>),
    Map(Box<SMember>, Box<SMember>),
    Struct(String),
    Union(String),
}

/// How a member travels in an HTTP request.
#[derive(Clone, Debug, PartialEq)]
pub enum SBinding {
    Body,
    Label,
    Query(String),
    QueryParams,
    Header(String),
    PrefixHeaders(String),
    Payload,
    ResponseCode,
    HostLabel,
}

/// Wire metadata for one member.
#[derive(Clone, Debug, PartialEq)]
pub struct SMember {
    pub name: String,
    pub wire: String,
    pub xml_name: Option<String>,
    pub xml_attribute: bool,
    pub flattened: bool,
    pub binding: SBinding,
    pub target: STarget,
    /// `@required`; only recorded for query/header-bound members, where it discriminates routes.
    pub required: bool,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub wire: String,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub ty: Ty,
    pub optional: bool,
    pub default: Option<serde_json::Value>,
    pub schema: SMember,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    Client,
    Server,
}

#[derive(Clone, Debug)]
pub struct ErrorDef {
    pub query_code: String,
    pub http_status: u16,
    pub fault: FaultKind,
}

#[derive(Clone, Debug)]
pub struct UnionVariant {
    /// `None` for `smithy.api#Unit` members.
    pub ty: Option<Ty>,
    pub schema: SMember,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    Enum(Vec<EnumVariant>),
    StrNewtype(Constraints),
    IntNewtype(Constraints),
    FloatNewtype(Constraints),
    BlobNewtype(Constraints),
    ListNewtype(Ty, Constraints),
    MapNewtype(Ty, Ty, Constraints),
    Struct {
        fields: Vec<Field>,
        error: Option<ErrorDef>,
        xml_name: Option<String>,
    },
    Union(Vec<UnionVariant>),
}

#[derive(Clone, Debug)]
pub struct TypeDef {
    pub name: String,
    pub kind: TypeKind,
}

#[derive(Clone, Debug)]
pub struct HttpIr {
    pub method: String,
    pub uri: String,
    pub code: u16,
}

#[derive(Clone, Debug)]
pub struct OpIr {
    pub name: String,
    pub input: Option<String>,
    pub output: Option<String>,
    /// Type names of the error structures, deduplicated, in model order.
    pub errors: Vec<String>,
    pub http: Option<HttpIr>,
}

#[derive(Debug)]
pub struct Ir {
    pub types: BTreeMap<String, TypeDef>,
    pub ops: Vec<OpIr>,
}
