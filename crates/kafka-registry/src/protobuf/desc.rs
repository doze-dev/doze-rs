//! Schemas given as descriptors. confluent-kafka for Python (and Go) registers a Protobuf message as the base64 of
//! its serialized `FileDescriptorProto`, and Confluent's registry accepts that when the text is not `.proto`. The
//! descriptor is decoded here (only what a schema says: no third-party Protobuf runtime) and printed as `.proto` text
//! the way Confluent prints it (type names keep their leading dot, a map is its repeated `*Entry` message with
//! `map_entry` set, a proto2 file has no syntax line), which then goes through the same parser and printer as any
//! other schema, so one message registered both ways is one schema. Held to what Confluent stored for the recorded
//! descriptors (`tests/data/protobuf/protodesc.json`).

use std::fmt::Write as _;

/// How deeply messages may nest in a descriptor.
const MAX_DEPTH: u32 = 100;

/// A field of the wire format: its number and its value (a varint, or the bytes of a length-delimited field).
enum V<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let c = *b.get(*i)?;
        *i += 1;
        v |= u64::from(c & 0x7f) << shift;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// The fields of one encoded message, in order; `None` when the bytes are not a message.
fn fields(b: &[u8]) -> Option<Vec<(u64, V<'_>)>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let key = varint(b, &mut i)?;
        let (num, wire) = (key >> 3, key & 7);
        if num == 0 {
            return None;
        }
        let v = match wire {
            0 => V::Int(varint(b, &mut i)?),
            1 | 5 => {
                i += if wire == 1 { 8 } else { 4 };
                b.get(..i)?;
                continue;
            }
            2 => {
                let n = usize::try_from(varint(b, &mut i)?).ok()?;
                let s = b.get(i..i.checked_add(n)?)?;
                i += n;
                V::Bytes(s)
            }
            _ => return None,
        };
        out.push((num, v));
    }
    Some(out)
}

fn text(v: &V<'_>) -> Option<String> {
    match v {
        V::Bytes(b) => String::from_utf8(b.to_vec()).ok(),
        V::Int(_) => None,
    }
}

fn int(v: &V<'_>) -> Option<i64> {
    match v {
        // Negative int32 values are sign-extended to ten bytes.
        V::Int(n) => Some(n.cast_signed()),
        V::Bytes(_) => None,
    }
}

fn sub<'a>(v: &V<'a>) -> Option<Vec<(u64, V<'a>)>> {
    match v {
        V::Bytes(b) => fields(b),
        V::Int(_) => None,
    }
}

/// The value of field `n` among `fs` (the last one, as Protobuf merges).
fn get<'a, 'b>(fs: &'b [(u64, V<'a>)], n: u64) -> Option<&'b V<'a>> {
    fs.iter().rev().find(|(k, _)| *k == n).map(|(_, v)| v)
}

fn all<'a, 'b>(fs: &'b [(u64, V<'a>)], n: u64) -> impl Iterator<Item = &'b V<'a>> {
    fs.iter().filter(move |(k, _)| *k == n).map(|(_, v)| v)
}

fn get_text(fs: &[(u64, V<'_>)], n: u64) -> Option<String> {
    get(fs, n).and_then(text)
}

fn get_bool(fs: &[(u64, V<'_>)], n: u64) -> Option<bool> {
    get(fs, n).and_then(int).map(|v| v != 0)
}

/// Repeated int32 values, packed or not.
fn ints(fs: &[(u64, V<'_>)], n: u64) -> Vec<i64> {
    let mut out = Vec::new();
    for v in all(fs, n) {
        match v {
            V::Int(x) => out.push(x.cast_signed()),
            V::Bytes(b) => {
                let mut i = 0;
                while let Some(x) = (i < b.len()).then(|| varint(b, &mut i)).flatten() {
                    out.push(x.cast_signed());
                }
            }
        }
    }
    out
}

/// Standard base64 with padding; line breaks are skipped.
fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    let mut pad = 0;
    let mut n = 0;
    for c in s.bytes().filter(|c| *c != b'\n' && *c != b'\r') {
        n += 1;
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                pad += 1;
                continue;
            }
            _ => return None,
        };
        if pad > 0 {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    (n % 4 == 0 && pad <= 2).then_some(out)
}

/// Go's `strconv.Quote` for the strings a descriptor carries.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `.proto` text of a base64 `FileDescriptorProto`, or `None` when the string is not one.
pub(super) fn descriptor_text(s: &str) -> Option<String> {
    let raw = base64(s.trim())?;
    if raw.is_empty() {
        return None;
    }
    let fd = fields(&raw)?;
    let any = |n| get(&fd, n).is_some();
    if !(get_text(&fd, 1).is_some_and(|n| !n.is_empty()) || any(4) || any(5) || any(6)) {
        return None;
    }
    let mut p = Printer {
        out: String::new(),
        proto2: !matches!(get_text(&fd, 12).as_deref(), Some(s) if s != "proto2" && !s.is_empty()),
    };
    p.file(&fd)?;
    Some(p.out)
}

struct Printer {
    out: String,
    proto2: bool,
}

impl Printer {
    fn file(&mut self, fd: &[(u64, V<'_>)]) -> Option<()> {
        if !self.proto2 {
            let _ = writeln!(self.out, "syntax = {};", quote(&get_text(fd, 12)?));
        }
        if let Some(p) = get_text(fd, 2).filter(|p| !p.is_empty()) {
            let _ = writeln!(self.out, "package {p};");
        }
        let (public, weak) = (ints(fd, 10), ints(fd, 11));
        for (i, dep) in all(fd, 3).enumerate() {
            let i = i64::try_from(i).ok()?;
            let m = if weak.contains(&i) {
                "weak "
            } else if public.contains(&i) {
                "public "
            } else {
                ""
            };
            let _ = writeln!(self.out, "import {m}{};", quote(&text(dep)?));
        }
        if let Some(o) = get(fd, 8).and_then(sub) {
            for (n, name, is_text) in [
                (1, "java_package", true),
                (8, "java_outer_classname", true),
                (10, "java_multiple_files", false),
                (11, "go_package", true),
                (23, "deprecated", false),
                (37, "csharp_namespace", true),
            ] {
                let v = if is_text {
                    get_text(&o, n).map(|t| quote(&t))
                } else {
                    get_bool(&o, n).map(|b| b.to_string())
                };
                if let Some(v) = v {
                    let _ = writeln!(self.out, "option {name} = {v};");
                }
            }
        }
        for m in all(fd, 4) {
            self.message(&sub(m)?, 0)?;
        }
        for e in all(fd, 5) {
            self.enumeration(&sub(e)?, 0)?;
        }
        for s in all(fd, 6) {
            self.service(&sub(s)?)?;
        }
        Some(())
    }

    fn service(&mut self, s: &[(u64, V<'_>)]) -> Option<()> {
        let _ = writeln!(self.out, "service {} {{", get_text(s, 1)?);
        for m in all(s, 2) {
            let m = sub(m)?;
            let stream = |n| {
                if get_bool(&m, n) == Some(true) {
                    "stream "
                } else {
                    ""
                }
            };
            let _ = writeln!(
                self.out,
                "  rpc {} ({}{}) returns ({}{});",
                get_text(&m, 1)?,
                stream(5),
                get_text(&m, 2)?,
                stream(6),
                get_text(&m, 3)?
            );
        }
        self.out.push_str("}\n");
        Some(())
    }

    fn message(&mut self, m: &[(u64, V<'_>)], depth: u32) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        let pad = "  ".repeat(depth as usize);
        let name = get_text(m, 1)?;
        let map_entry = get(m, 7).and_then(sub).and_then(|o| get_bool(&o, 7)) == Some(true);
        if !map_entry && [2, 3, 4, 9, 10, 8].iter().all(|&n| get(m, n).is_none()) {
            let _ = writeln!(self.out, "{pad}message {name} {{}}");
            return Some(());
        }
        let _ = writeln!(self.out, "{pad}message {name} {{");
        if map_entry {
            let _ = writeln!(self.out, "{pad}  option map_entry = true;");
        }
        for r in all(m, 9) {
            let r = sub(r)?;
            let (a, b) = (get(&r, 1).and_then(int)?, get(&r, 2).and_then(int)? - 1);
            let to = if a == b {
                String::new()
            } else {
                format!(" to {b}")
            };
            let _ = writeln!(self.out, "{pad}  reserved {a}{to};");
        }
        for r in all(m, 10) {
            let _ = writeln!(self.out, "{pad}  reserved {};", quote(&text(r)?));
        }
        let fs: Vec<_> = all(m, 2).map(sub).collect::<Option<_>>()?;
        let in_oneof = |f: &[(u64, V<'_>)]| get(f, 9).is_some() && get_bool(f, 17) != Some(true);
        for f in fs.iter().filter(|f| !in_oneof(f)) {
            self.field(f, depth + 1, true)?;
        }
        for (i, o) in all(m, 8).enumerate() {
            let i = i64::try_from(i).ok()?;
            let members: Vec<_> = fs
                .iter()
                .filter(|f| in_oneof(f) && get(f, 9).and_then(int) == Some(i))
                .collect();
            if members.is_empty() {
                continue; // the synthetic oneof of a proto3 optional
            }
            let _ = writeln!(self.out, "{pad}  oneof {} {{", get_text(&sub(o)?, 1)?);
            for f in members {
                self.field(f, depth + 2, false)?;
            }
            let _ = writeln!(self.out, "{pad}  }}");
        }
        for n in all(m, 3) {
            self.message(&sub(n)?, depth + 1)?;
        }
        for e in all(m, 4) {
            self.enumeration(&sub(e)?, depth + 1)?;
        }
        let _ = writeln!(self.out, "{pad}}}");
        Some(())
    }

    fn field(&mut self, f: &[(u64, V<'_>)], depth: u32, labelled: bool) -> Option<()> {
        let pad = "  ".repeat(depth as usize);
        let ty = get(f, 5).and_then(int).unwrap_or(0);
        let type_name = match super::link::SCALARS.get(usize::try_from(ty).ok()?) {
            Some(s) if !s.is_empty() => (*s).to_owned(),
            _ => get_text(f, 6)?,
        };
        let label = get(f, 4).and_then(int).unwrap_or(1);
        let prefix = match label {
            _ if !labelled => "",
            3 => "repeated ",
            _ if get_bool(f, 17) == Some(true) || (self.proto2 && label == 1) => "optional ",
            2 if self.proto2 => "required ",
            _ => "",
        };
        let mut opts = Vec::new();
        if let Some(o) = get(f, 8).and_then(sub) {
            for (n, name) in [(3, "deprecated"), (2, "packed")] {
                if let Some(b) = get_bool(&o, n) {
                    opts.push(format!("{name} = {b}"));
                }
            }
        }
        if let Some(d) = get_text(f, 7) {
            opts.push(format!("default = {}", if ty == 9 { quote(&d) } else { d }));
        }
        if let Some(j) = get_text(f, 10) {
            // Set by protoc on every field, and printed by Confluent whenever it is set.
            opts.push(format!("json_name = {}", quote(&j)));
        }
        let o = match opts.as_slice() {
            [] => String::new(),
            [one] => format!(" [{one}]"),
            _ => format!(" [\n{pad}  {}\n{pad}]", opts.join(&format!(",\n{pad}  "))),
        };
        let _ = writeln!(
            self.out,
            "{pad}{prefix}{type_name} {} = {}{o};",
            get_text(f, 1)?,
            get(f, 3).and_then(int)?
        );
        Some(())
    }

    fn enumeration(&mut self, e: &[(u64, V<'_>)], depth: u32) -> Option<()> {
        let pad = "  ".repeat(depth as usize);
        let _ = writeln!(self.out, "{pad}enum {} {{", get_text(e, 1)?);
        if let Some(a) = get(e, 3).and_then(sub).and_then(|o| get_bool(&o, 2)) {
            let _ = writeln!(self.out, "{pad}  option allow_alias = {a};");
        }
        for v in all(e, 2) {
            let v = sub(v)?;
            let n = get(&v, 2).and_then(int).unwrap_or(0);
            let _ = writeln!(
                self.out,
                "{pad}  {} = {};",
                get_text(&v, 1)?,
                i32::try_from(n).ok()?
            );
        }
        let _ = writeln!(self.out, "{pad}}}");
        Some(())
    }
}
