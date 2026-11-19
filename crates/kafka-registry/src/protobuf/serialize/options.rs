//! The pieces of an encoded descriptor: a message being written, and the built-in options known by name.

use super::super::ast::{Opt, Val};

/// One encoded message, its fields appended in field-number order by the builders below.
#[derive(Default)]
pub(super) struct Out(pub(super) Vec<u8>);

impl Out {
    pub(super) fn key(&mut self, num: u32, wire: u8) {
        self.varint((u64::from(num) << 3) | u64::from(wire));
    }

    pub(super) fn varint(&mut self, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(b);
                return;
            }
            self.0.push(b | 0x80);
        }
    }

    pub(super) fn int(&mut self, num: u32, v: i64) {
        self.key(num, 0);
        // An int32 below zero is written sign-extended to ten bytes, as protobuf writes it.
        self.varint(v.cast_unsigned());
    }

    pub(super) fn bool(&mut self, num: u32, v: bool) {
        self.key(num, 0);
        self.varint(u64::from(v));
    }

    pub(super) fn bytes(&mut self, num: u32, v: &[u8]) {
        self.key(num, 2);
        self.varint(v.len() as u64);
        self.0.extend_from_slice(v);
    }

    pub(super) fn str(&mut self, num: u32, v: &str) {
        self.bytes(num, v.as_bytes());
    }

    pub(super) fn msg(&mut self, num: u32, m: &Out) {
        self.bytes(num, &m.0);
    }
}

/// The text of a string value (each literal of it unquoted, adjacent ones joined).
pub(super) fn string_value(src: &str, o: &Opt) -> String {
    let lit = src
        .get(o.val_span.0 as usize..o.val_span.1 as usize)
        .unwrap_or_default();
    let mut out = Vec::new();
    let b = lit.as_bytes();
    let mut i = 0;
    while let Some(&q) = b.get(i) {
        if q != b'"' && q != b'\'' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while let Some(&c) = b.get(i).filter(|&&c| c != q) {
            i += if c == b'\\' { 2 } else { 1 };
        }
        let end = (i + 1).min(b.len());
        if let Some(part) = lit.get(start..end) {
            out.extend(super::super::lex::unquote(part, 0).unwrap_or_default());
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn bool_value(o: &Opt) -> Option<bool> {
    match &o.val {
        Val::Ident(v) if v == "true" => Some(true),
        Val::Ident(v) if v == "false" => Some(false),
        _ => None,
    }
}

/// The options of one kind as their message: each built-in option this knows, in field-number order; custom options and
/// unknown ones are left out.
pub(super) fn options(src: &str, opts: &[&Opt], table: &[(&str, u32, Kind)]) -> Option<Out> {
    let mut found: Vec<(u32, &Opt, Kind)> = opts
        .iter()
        .filter_map(|o| {
            let name = o.simple()?;
            table
                .iter()
                .find(|(n, _, _)| *n == name)
                .map(|&(_, num, kind)| (num, *o, kind))
        })
        .collect();
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(|&(n, _, _)| n);
    let mut m = Out::default();
    for (num, o, kind) in found {
        match kind {
            Kind::Bool => {
                if let Some(b) = bool_value(o) {
                    m.bool(num, b);
                }
            }
            Kind::Str => m.str(num, &string_value(src, o)),
            Kind::Enum(names) => {
                if let Val::Ident(v) = &o.val
                    && let Some(p) = names.iter().position(|n| n == v)
                {
                    m.int(num, i64::try_from(p).unwrap_or(0));
                } else if let Val::Num(v, false) = &o.val
                    && let Ok(n) = v.parse::<i64>()
                {
                    m.int(num, n);
                }
            }
        }
    }
    Some(m)
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Bool,
    Str,
    /// An enum's value names, by number from 0.
    Enum(&'static [&'static str]),
}

pub(super) const OPTIMIZE: &[&str] = &["", "SPEED", "CODE_SIZE", "LITE_RUNTIME"];
pub(super) const FILE_OPTIONS: &[(&str, u32, Kind)] = &[
    ("java_package", 1, Kind::Str),
    ("java_outer_classname", 8, Kind::Str),
    ("optimize_for", 9, Kind::Enum(OPTIMIZE)),
    ("java_multiple_files", 10, Kind::Bool),
    ("go_package", 11, Kind::Str),
    ("cc_generic_services", 16, Kind::Bool),
    ("java_generic_services", 17, Kind::Bool),
    ("py_generic_services", 18, Kind::Bool),
    ("java_generate_equals_and_hash", 20, Kind::Bool),
    ("deprecated", 23, Kind::Bool),
    ("java_string_check_utf8", 27, Kind::Bool),
    ("cc_enable_arenas", 31, Kind::Bool),
    ("objc_class_prefix", 36, Kind::Str),
    ("csharp_namespace", 37, Kind::Str),
    ("swift_prefix", 39, Kind::Str),
    ("php_class_prefix", 40, Kind::Str),
    ("php_namespace", 41, Kind::Str),
    ("php_metadata_namespace", 44, Kind::Str),
    ("ruby_package", 45, Kind::Str),
];
pub(super) const MESSAGE_OPTIONS: &[(&str, u32, Kind)] = &[
    ("message_set_wire_format", 1, Kind::Bool),
    ("no_standard_descriptor_accessor", 2, Kind::Bool),
    ("deprecated", 3, Kind::Bool),
    ("map_entry", 7, Kind::Bool),
];
pub(super) const FIELD_OPTIONS: &[(&str, u32, Kind)] = &[
    ("ctype", 1, Kind::Enum(&["STRING", "CORD", "STRING_PIECE"])),
    ("packed", 2, Kind::Bool),
    ("deprecated", 3, Kind::Bool),
    ("lazy", 5, Kind::Bool),
    (
        "jstype",
        6,
        Kind::Enum(&["JS_NORMAL", "JS_STRING", "JS_NUMBER"]),
    ),
    ("weak", 10, Kind::Bool),
];
pub(super) const ENUM_OPTIONS: &[(&str, u32, Kind)] = &[
    ("allow_alias", 2, Kind::Bool),
    ("deprecated", 3, Kind::Bool),
];
pub(super) const VALUE_OPTIONS: &[(&str, u32, Kind)] = &[("deprecated", 1, Kind::Bool)];
pub(super) const SERVICE_OPTIONS: &[(&str, u32, Kind)] = &[("deprecated", 33, Kind::Bool)];
pub(super) const METHOD_OPTIONS: &[(&str, u32, Kind)] = &[
    ("deprecated", 33, Kind::Bool),
    (
        "idempotency_level",
        34,
        Kind::Enum(&["IDEMPOTENCY_UNKNOWN", "NO_SIDE_EFFECTS", "IDEMPOTENT"]),
    ),
];
