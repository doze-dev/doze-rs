//! The `.proto` text as written: declarations in their order, each with the spans of the source the stored text
//! repeats (types, numbers, option names and values, reserved and extension ranges), and with what the linker needs
//! (names, numbers, labels) already read out.

/// A span of the source, in bytes.
pub(super) type Span = (u32, u32);

/// The largest field number.
pub(super) const MAX_FIELD: i64 = 536_870_911;

/// Stands for `max` in a range until the range is known to be a field's or an enum value's.
pub(super) const MAX_WORD: i64 = i64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Label {
    Optional,
    Required,
    Repeated,
}

impl Label {
    pub(super) fn word(self) -> &'static str {
        match self {
            Self::Optional => "optional",
            Self::Required => "required",
            Self::Repeated => "repeated",
        }
    }
}

#[derive(Debug)]
pub(super) struct File {
    /// The `syntax` string's source and value, if the file has one.
    pub(super) syntax: Option<(Span, String)>,
    pub(super) decls: Vec<Decl>,
}

#[derive(Debug)]
pub(super) enum Decl {
    Package(String, Span),
    /// The modifier (`public`, `weak` or none), the path, the path's source.
    Import(&'static str, String, Span),
    Option(Opt),
    Message(Message),
    Enum(Enum),
    Extend(Extend),
    Service(Service),
}

/// An option: `name = value`, a statement or one of a field's.
#[derive(Debug)]
pub(super) struct Opt {
    /// The name's parts; a part in parentheses names an extension.
    pub(super) name: Vec<(String, bool)>,
    pub(super) name_span: Span,
    pub(super) val: Val,
    pub(super) val_span: Span,
    pub(super) at: u32,
}

impl Opt {
    /// The name when it is one plain word (a built-in option).
    pub(super) fn simple(&self) -> Option<&str> {
        match self.name.as_slice() {
            [(n, false)] => Some(n),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) enum Val {
    Ident(String),
    Str,
    /// A number as written (sign included); float or integer.
    Num(String, bool),
    /// A message literal in braces, not read further.
    Aggregate,
}

#[derive(Debug)]
pub(super) struct Message {
    pub(super) name: String,
    pub(super) at: u32,
    pub(super) body: Vec<Item>,
}

#[derive(Debug)]
pub(super) enum Item {
    Field(Field),
    Group(Group),
    Oneof(Oneof),
    Option(Opt),
    Message(Message),
    Enum(Enum),
    Extend(Extend),
    Reserved(Reserved),
    Extensions(Reserved, Vec<Opt>),
}

/// `reserved` (ranges or names) or `extensions` (ranges): the source and what it lists.
#[derive(Debug)]
pub(super) struct Reserved {
    pub(super) span: Span,
    pub(super) ranges: Vec<(i64, i64)>,
    pub(super) names: Vec<String>,
    pub(super) at: u32,
}

/// A field's type as written: a name (scalar or message or enum), or a map.
#[derive(Debug)]
pub(super) enum TypeRef {
    Named(String, Span),
    Map(String, Span, String, Span),
}

#[derive(Debug)]
pub(super) struct Field {
    pub(super) label: Option<Label>,
    pub(super) ty: TypeRef,
    pub(super) name: String,
    pub(super) number: i64,
    pub(super) tag: Span,
    pub(super) opts: Vec<Opt>,
    pub(super) at: u32,
}

#[derive(Debug)]
pub(super) struct Group {
    pub(super) label: Option<Label>,
    pub(super) name: String,
    pub(super) number: i64,
    pub(super) tag: Span,
    pub(super) opts: Vec<Opt>,
    pub(super) body: Vec<Item>,
    pub(super) at: u32,
}

#[derive(Debug)]
pub(super) struct Oneof {
    pub(super) name: String,
    pub(super) at: u32,
    /// Fields, groups and options.
    pub(super) body: Vec<Item>,
}

#[derive(Debug)]
pub(super) struct Enum {
    pub(super) name: String,
    pub(super) at: u32,
    pub(super) body: Vec<EnumItem>,
}

#[derive(Debug)]
pub(super) enum EnumItem {
    Option(Opt),
    Reserved(Reserved),
    Value(EnumValue),
}

#[derive(Debug)]
pub(super) struct EnumValue {
    pub(super) name: String,
    pub(super) number: i64,
    pub(super) num_span: Span,
    pub(super) opts: Vec<Opt>,
    pub(super) at: u32,
}

#[derive(Debug)]
pub(super) struct Extend {
    pub(super) extendee: String,
    pub(super) span: Span,
    pub(super) at: u32,
    /// Fields and groups.
    pub(super) body: Vec<Item>,
}

#[derive(Debug)]
pub(super) struct Service {
    pub(super) name: String,
    pub(super) at: u32,
    pub(super) opts: Vec<Opt>,
    pub(super) rpcs: Vec<Rpc>,
}

/// One end of an rpc: streamed or not, the message type and its source.
pub(super) type RpcType = (bool, String, Span);

#[derive(Debug)]
pub(super) struct Rpc {
    pub(super) name: String,
    pub(super) input: RpcType,
    pub(super) output: RpcType,
    pub(super) opts: Vec<Opt>,
    pub(super) at: u32,
}

/// The parser.
mod parse;

pub(super) use parse::parse;

/// An integer literal's value: decimal, `0x` hex or `0` octal.
pub(super) fn int_value(t: &str) -> Option<u64> {
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else if t.len() > 1 && t.starts_with('0') {
        u64::from_str_radix(t.get(1..)?, 8).ok()
    } else {
        t.parse().ok()
    }
}
