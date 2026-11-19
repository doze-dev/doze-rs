//! A `.proto` schema normalized as Confluent normalizes it (`?normalize=true`, or the `normalize` setting): its
//! `ProtobufSchemaUtils.toNormalizedString`, written again here from Confluent's source (cp-schema-registry 8.3.2, the
//! same as 7.7.1 but where said) and held to its answers by tools/registry-diff (`protobuf-normalize`):
//!
//! - no `syntax` line for proto2; imports and public imports sorted, each once;
//! - every type name qualified with a leading dot (`.shop.v1.Money`), found as Confluent finds it: the main file's
//!   package and the enclosing names tried innermost first, among every type of the file, its references and the
//!   well-known files; a map's value type too, and a field of a map-entry message (a descriptor's) is its `map<,>`;
//! - options sorted by name, a custom option named by its extension's full name, a dotted one (`(a).b = 1`) made a
//!   message literal, a non-repeated custom option given twice merged into one (8.3.2; 7.7.1 makes a repeated key a
//!   list); a message literal's keys sorted and a one-item list written as the item; numbers as Java's `DecimalFormat`
//!   writes the number commons-lang's `createNumber` reads (`0x10` is `16`, `2.50` is `2.5`, three decimals at most);
//! - fields, a oneof's fields, groups and an extend's fields by number; oneofs by their first field, empty ones gone;
//!   enum values by number, then name; reserved and extension ranges one to a statement, by start (names after
//!   numbers, by name), a range of one number written as the number; the extends of one message type made one;
//! - messages, enums, services and rpcs stay in their order: a client numbers messages by their place.
//!
//! What it writes is the stored text itself, as Confluent stores the normalized string.

use std::collections::HashMap;

use super::ast::{
    Decl, Enum, EnumItem, Extend, Field, File, Group, Item, Label, MAX_FIELD, Message, Oneof, Opt,
    Reserved, Service, TypeRef, Val,
};
use super::lex::{Tok, lex};
use super::link::SCALARS;

/// A message's field, as Confluent's merging of options looks it up.
#[derive(Debug, Clone)]
struct FieldInfo {
    name: String,
    repeated: bool,
    ty: String,
}

/// What Confluent knows of a type by its full name: whether it is a map entry (its key and value types), and its
/// fields when it is a message.
#[derive(Debug, Default)]
struct TypeInfo {
    map: Option<(String, String)>,
    fields: Option<Vec<FieldInfo>>,
}

/// Every type and extension a schema sees, by full name (no leading dot), and the main file's package.
#[derive(Debug, Default)]
struct Names {
    package: Vec<String>,
    types: HashMap<String, TypeInfo>,
    exts: HashMap<String, FieldInfo>,
}

fn field_ty(t: &TypeRef) -> String {
    match t {
        TypeRef::Named(n, _) => n.clone(),
        TypeRef::Map(k, _, v, _) => format!("map<{k}, {v}>"),
    }
}

fn info(f: &Field) -> FieldInfo {
    FieldInfo {
        name: f.name.clone(),
        repeated: f.label == Some(Label::Repeated),
        ty: field_ty(&f.ty),
    }
}

fn join(path: &[String], name: &str) -> String {
    let mut p = path.join(".");
    if !p.is_empty() {
        p.push('.');
    }
    p.push_str(name);
    p
}

impl Names {
    /// Collects a file's types and extensions under its package, as Confluent's `Context.collectTypeInfo`.
    fn collect(&mut self, file: &File) {
        let package: Vec<String> = file
            .decls
            .iter()
            .find_map(|d| match d {
                Decl::Package(p) => Some(p.split('.').map(str::to_owned).collect()),
                _ => None,
            })
            .unwrap_or_default();
        let mut path = package.clone();
        for d in &file.decls {
            match d {
                Decl::Message(m) => self.message(&mut path, m),
                Decl::Enum(e) => {
                    self.types.insert(join(&path, &e.name), TypeInfo::default());
                }
                Decl::Extend(e) => self.extend(&path, e),
                _ => {}
            }
        }
        self.package = package;
    }

    fn extend(&mut self, path: &[String], e: &Extend) {
        for i in &e.body {
            if let Item::Field(f) = i {
                self.exts.insert(join(path, &f.name), info(f));
            }
        }
    }

    fn message(&mut self, path: &mut Vec<String>, m: &Message) {
        let full = join(path, &m.name);
        path.push(m.name.clone());
        let mut fields = Vec::new();
        let mut map_entry = false;
        for i in &m.body {
            match i {
                Item::Field(f) => fields.push(info(f)),
                Item::Oneof(o) => {
                    for x in &o.body {
                        if let Item::Field(f) = x {
                            fields.push(info(f));
                        }
                    }
                }
                Item::Option(o) => {
                    map_entry |= o.simple() == Some("map_entry")
                        && matches!(&o.val, Val::Ident(v) if v == "true");
                }
                Item::Message(n) => self.message(path, n),
                Item::Enum(e) => {
                    self.types.insert(join(path, &e.name), TypeInfo::default());
                }
                Item::Extend(e) => self.extend(path, e),
                _ => {}
            }
        }
        path.pop();
        let pick = |n: &str| fields.iter().find(|f| f.name == n).map(|f| f.ty.clone());
        let map = if map_entry {
            Some((
                pick("key").unwrap_or_default(),
                pick("value").unwrap_or_default(),
            ))
        } else {
            None
        };
        self.types.insert(
            full,
            TypeInfo {
                map,
                fields: Some(fields),
            },
        );
    }

    /// A name as Confluent's `Context.resolveFull` finds it from `scope` (the names entered so far): with a leading
    /// dot, as it is; otherwise prefixed by the package and the scope, innermost first, then on its own.
    fn resolve(&self, name: &str, scope: &[String], has: impl Fn(&str) -> bool) -> Option<String> {
        if let Some(n) = name.strip_prefix('.') {
            return has(n).then(|| n.to_owned());
        }
        let mut prefix: Vec<&str> = self.package.iter().map(String::as_str).collect();
        prefix.extend(scope.iter().map(String::as_str));
        while !prefix.is_empty() {
            let n = format!("{}.{name}", prefix.join("."));
            if has(&n) {
                return Some(n);
            }
            prefix.pop();
        }
        has(name).then(|| name.to_owned())
    }

    fn resolve_type(&self, name: &str, scope: &[String]) -> Option<String> {
        self.resolve(name, scope, |n| self.types.contains_key(n))
    }

    fn resolve_ext(&self, name: &str, scope: &[String]) -> Option<String> {
        self.resolve(name, scope, |n| self.exts.contains_key(n))
    }
}

/// An option's value as Wire reads it (`OptionElement` and the values of its maps and lists).
#[derive(Debug, Clone, PartialEq)]
enum V {
    Str(String),
    Bool(String),
    Enum(String),
    Num(String),
    Map(Vec<(String, V)>),
    List(Vec<V>),
    Opt(Box<Named>),
}

/// An option: its name (in parentheses for an extension) and value.
#[derive(Debug, Clone, PartialEq)]
struct Named {
    name: String,
    paren: bool,
    val: V,
}

/// The characters a string literal stands for, as Wire's `readQuotedString` reads them: an escape of a number is the
/// character of that code, an escape it does not know is the character itself.
fn wire_unquote(lit: &str) -> String {
    let mut out = String::new();
    let mut chars = lit.chars().peekable();
    while let Some(q) = chars.next() {
        if q != '"' && q != '\'' {
            continue;
        }
        while let Some(c) = chars.next() {
            if c == q {
                break;
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some(e) = chars.next() else { break };
            let numeric = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
                           radix: u32,
                           len: usize,
                           first: Option<u32>| {
                let mut v = first;
                for _ in 0..len - usize::from(first.is_some()) {
                    match chars
                        .peek()
                        .and_then(|c| c.to_digit(16))
                        .filter(|&d| d < radix)
                    {
                        Some(d) => {
                            v = Some(v.map_or(d, |v| v * radix + d));
                            chars.next();
                        }
                        None => break,
                    }
                }
                v.and_then(char::from_u32).unwrap_or('\0')
            };
            out.push(match e {
                'a' => '\u{7}',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\u{b}',
                'x' | 'X' => numeric(&mut chars, 16, 2, None),
                '0'..='7' => numeric(&mut chars, 8, 3, e.to_digit(8)),
                other => other,
            });
        }
    }
    out
}

/// Confluent's `escapeChars`.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// A Java `String` order (UTF-16 code units).
fn java_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Reads a message literal (`{ a: 1 b { c: "x" } d: [1, 2] }`) as Wire's `OptionReader.readMap` does.
struct Literal<'a> {
    src: &'a str,
    toks: Vec<super::lex::Token>,
    i: usize,
}

impl Literal<'_> {
    fn text(&self, i: usize) -> &str {
        self.toks
            .get(i)
            .and_then(|t| self.src.get(t.start as usize..t.end as usize))
            .unwrap_or_default()
    }

    fn is(&self, c: u8) -> bool {
        self.toks
            .get(self.i)
            .is_some_and(|t| t.kind == Tok::Punct(c))
    }

    fn eat(&mut self, c: u8) -> bool {
        let yes = self.is(c);
        if yes {
            self.i += 1;
        }
        yes
    }

    /// A dotted word (`a.b.c`), as Wire's `readWord` takes dots into a word.
    fn word(&mut self) -> String {
        let mut w = String::new();
        loop {
            match self.toks.get(self.i).map(|t| t.kind) {
                Some(Tok::Ident | Tok::Int | Tok::Float) => {
                    w.push_str(self.text(self.i));
                    self.i += 1;
                }
                _ => break,
            }
            if self.is(b'.')
                && matches!(self.toks.get(self.i + 1).map(|t| t.kind), Some(Tok::Ident))
            {
                w.push('.');
                self.i += 1;
            } else {
                break;
            }
        }
        w
    }

    fn map(&mut self, depth: usize) -> Vec<(String, V)> {
        let mut out: Vec<(String, V)> = Vec::new();
        self.eat(b'{');
        while self.i < self.toks.len() && !self.eat(b'}') {
            let key = if self.eat(b'[') {
                let mut k = String::new();
                while self.i < self.toks.len() && !self.eat(b']') {
                    k.push_str(self.text(self.i));
                    self.i += 1;
                }
                format!("[{k}]")
            } else {
                self.word()
            };
            if key.is_empty() {
                self.i += 1;
                continue;
            }
            self.eat(b':');
            let v = self.value(depth + 1);
            match out.iter_mut().find(|(k, _)| *k == key) {
                None => out.push((key, v)),
                Some((_, prev)) => {
                    let mut list = match std::mem::replace(prev, V::List(Vec::new())) {
                        V::List(l) => l,
                        other => vec![other],
                    };
                    match v {
                        V::List(l) => list.extend(l),
                        v => list.push(v),
                    }
                    *prev = V::List(list);
                }
            }
            if !self.eat(b',') {
                self.eat(b';');
            }
        }
        out
    }

    fn value(&mut self, depth: usize) -> V {
        if depth > 100 {
            return V::Map(Vec::new());
        }
        let Some(t) = self.toks.get(self.i).copied() else {
            return V::Enum(String::new());
        };
        match t.kind {
            Tok::Punct(b'{') => V::Map(self.map(depth)),
            Tok::Punct(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                while self.i < self.toks.len() && !self.eat(b']') {
                    items.push(self.value(depth + 1));
                    self.eat(b',');
                }
                V::List(items)
            }
            Tok::Str => {
                let mut s = String::new();
                while self.toks.get(self.i).is_some_and(|t| t.kind == Tok::Str) {
                    s.push_str(&wire_unquote(self.text(self.i)));
                    self.i += 1;
                }
                V::Str(s)
            }
            Tok::Punct(b'-') => {
                self.i += 1;
                V::Num(format!("-{}", self.word()))
            }
            Tok::Int | Tok::Float => V::Num(self.word()),
            _ => {
                let w = self.word();
                if w.is_empty() {
                    self.i += 1;
                }
                match w.as_str() {
                    "true" | "false" => V::Bool(w),
                    _ => V::Enum(w),
                }
            }
        }
    }
}

/// An option of the schema as Wire reads it: the first part of its name, then its value, a dotted name nesting it.
fn named(src: &str, o: &Opt) -> Named {
    let span = |(a, b): (u32, u32)| src.get(a as usize..b as usize).unwrap_or_default();
    let mut val = match &o.val {
        Val::Str => V::Str(wire_unquote(span(o.val_span))),
        Val::Ident(w) if w == "true" || w == "false" => V::Bool(w.clone()),
        Val::Ident(w) => V::Enum(w.clone()),
        Val::Num(n, _) => V::Num(n.clone()),
        Val::Aggregate => {
            let text = span(o.val_span);
            let toks = lex(text).unwrap_or_default();
            V::Map(
                Literal {
                    src: text,
                    toks,
                    i: 0,
                }
                .map(0),
            )
        }
    };
    let (first, rest) = o
        .name
        .split_first()
        .map_or((None, &[][..]), |(f, r)| (Some(f), r));
    for (n, paren) in rest.iter().rev() {
        val = V::Opt(Box::new(Named {
            name: n.clone(),
            paren: *paren,
            val,
        }));
    }
    let (name, paren) = first.cloned().unwrap_or_default();
    Named { name, paren, val }
}

/// Confluent's `transform`: a dotted option made a message literal.
fn transform(o: Named) -> Named {
    fn map_of(inner: Named) -> V {
        let v = match inner.val {
            V::Opt(n) => map_of(*n),
            v => v,
        };
        V::Map(vec![(inner.name, v)])
    }
    match o.val {
        V::Opt(inner) => Named {
            val: map_of(*inner),
            ..o
        },
        _ => o,
    }
}

/// How Confluent's normalizing printer writes a number: commons-lang's `NumberUtils.createNumber`, then a
/// `java.text.DecimalFormat` without grouping (at most three decimals, half to even). Text it cannot read (`inf`, `nan`)
/// is written as it is.
pub(super) fn java_format_number(text: &str) -> String {
    format_number(text).unwrap_or_else(|| text.to_owned())
}

fn format_number(text: &str) -> Option<String> {
    let (neg, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let sign = if neg { "-" } else { "" };
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        let v = u128::from_str_radix(hex, 16).ok()?;
        return Some(if v == 0 {
            "0".into()
        } else {
            format!("{sign}{v}")
        });
    }
    if !body.contains(['.', 'e', 'E']) {
        if !body.bytes().all(|b| b.is_ascii_digit()) || body.is_empty() {
            return None;
        }
        let digits = if body.len() > 1 && body.starts_with('0') {
            // Integer.decode reads a leading zero as octal.
            u128::from_str_radix(&body[1..], 8).ok()?.to_string()
        } else {
            body.trim_start_matches('0').to_owned()
        };
        let digits = if digits.is_empty() {
            "0".to_owned()
        } else {
            digits
        };
        return Some(if digits == "0" {
            digits
        } else {
            format!("{sign}{digits}")
        });
    }
    let (mant, frac_exp) = match body.split_once('.') {
        Some((m, rest)) => (m, rest),
        None => (body, ""),
    };
    let frac = frac_exp.split(['e', 'E']).next().unwrap_or_default();
    let mant = mant.split(['e', 'E']).next().unwrap_or_default();
    if !mant.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let all_zero = mant.bytes().chain(frac.bytes()).all(|b| b == b'0');
    let f: f32 = text.parse().ok()?;
    let d: f64 = text.parse().ok()?;
    let exact = Exact::parse(text)?;
    if f.is_finite() && !(f == 0.0 && !all_zero) && format!("{f:e}") == format!("{d:e}") {
        return Some(format_double(f64::from(f)));
    }
    if d.is_finite() && !(d == 0.0 && !all_zero) {
        if Exact::parse(&format!("{d:e}")).is_some_and(|x| x == exact) {
            return Some(format_double(d));
        }
        return Some(exact.format3());
    }
    Some(exact.format3())
}

/// `DecimalFormat.format(double)`: the shortest digits of the double, with at most three decimals (rounded as the
/// double's exact value rounds, half to even).
fn format_double(x: f64) -> String {
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let short = format!("{:e}", x.abs());
    let Some(e) = Exact::parse(&short) else {
        return format!("{x}");
    };
    let frac_digits = i64::try_from(e.digits.len()).unwrap_or(0) - 1 - e.exp_of_first();
    if frac_digits <= 3 {
        return format!("{sign}{}", e.plain());
    }
    let s = format!("{:.3}", x.abs());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{sign}{s}")
}

/// An exact decimal: digits (no leading or trailing zeros) times ten to `exp` (of the last digit).
#[derive(Debug, PartialEq, Eq)]
struct Exact {
    neg: bool,
    digits: Vec<u8>,
    exp: i64,
}

impl Exact {
    fn parse(text: &str) -> Option<Self> {
        let (neg, body) = match text.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (m, e) = match body.find(['e', 'E']) {
            Some(i) => (&body[..i], body[i + 1..].parse::<i64>().ok()?),
            None => (body, 0),
        };
        let (int, frac) = m.split_once('.').unwrap_or((m, ""));
        if !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
        let mut exp = e.checked_sub(i64::try_from(frac.len()).ok()?)?;
        while digits.last() == Some(&0) {
            digits.pop();
            exp += 1;
        }
        let lead = digits.iter().take_while(|&&d| d == 0).count();
        digits.drain(..lead);
        if digits.is_empty() {
            return Some(Self {
                neg: false,
                digits,
                exp: 0,
            });
        }
        Some(Self { neg, digits, exp })
    }

    /// The power of ten of the first digit.
    fn exp_of_first(&self) -> i64 {
        self.exp + i64::try_from(self.digits.len()).unwrap_or(0) - 1
    }

    /// Written out without an exponent (a magnitude, no sign).
    fn plain(&self) -> String {
        if self.digits.is_empty() {
            return "0".into();
        }
        let ds: String = self.digits.iter().map(|d| char::from(b'0' + d)).collect();
        if self.exp >= 0 {
            return format!("{ds}{}", "0".repeat(usize::try_from(self.exp).unwrap_or(0)));
        }
        let frac = usize::try_from(-self.exp).unwrap_or(0);
        if ds.len() > frac {
            let cut = ds.len() - frac;
            format!("{}.{}", &ds[..cut], &ds[cut..])
        } else {
            format!("0.{}{ds}", "0".repeat(frac - ds.len()))
        }
    }

    /// `DecimalFormat.format(BigDecimal)`: at most three decimals, half to even.
    fn format3(&self) -> String {
        let sign = if self.neg { "-" } else { "" };
        if self.exp >= -3 {
            return format!("{sign}{}", self.plain());
        }
        // Keep the digits down to 10^-3, and decide on the rest.
        let keep = i64::try_from(self.digits.len()).unwrap_or(0) + self.exp + 3;
        let (mut kept, rest): (Vec<u8>, Vec<u8>) = if keep <= 0 {
            (Vec::new(), {
                let mut r = vec![0; usize::try_from(-keep).unwrap_or(0)];
                r.extend(&self.digits);
                r
            })
        } else {
            let k = usize::try_from(keep).unwrap_or(0);
            (self.digits[..k].to_vec(), self.digits[k..].to_vec())
        };
        let half = match rest.first() {
            Some(&d) if d > 5 => std::cmp::Ordering::Greater,
            Some(&5) if rest.len() > 1 => std::cmp::Ordering::Greater,
            Some(&5) => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Less,
        };
        let up = match half {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => kept.last().is_some_and(|d| d % 2 == 1),
            std::cmp::Ordering::Less => false,
        };
        if up {
            let mut i = kept.len();
            loop {
                if i == 0 {
                    kept.insert(0, 1);
                    break;
                }
                i -= 1;
                if kept[i] == 9 {
                    kept[i] = 0;
                } else {
                    kept[i] += 1;
                    break;
                }
            }
        }
        let rounded = Self {
            neg: false,
            digits: kept,
            exp: -3,
        };
        let text = Self::parse(&format!("{}e-3", rounded.plain_digits()))
            .map_or_else(|| "0".into(), |r| r.plain());
        format!("{sign}{text}")
    }

    fn plain_digits(&self) -> String {
        if self.digits.is_empty() {
            "0".into()
        } else {
            self.digits.iter().map(|d| char::from(b'0' + d)).collect()
        }
    }
}

/// Confluent's `appendIndented`: each line of `value` (Java's `split("\n")`, trailing empty lines dropped) indented by
/// two spaces.
fn indented(out: &mut String, value: &str) {
    let mut lines: Vec<&str> = value.split('\n').collect();
    while lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.is_empty() && !value.contains('\n') {
        lines.push(value);
    }
    for l in lines {
        out.push_str("  ");
        out.push_str(l);
        out.push('\n');
    }
}

/// The printer: the names a schema sees and the names entered (messages and enums) around what is printed.
struct Printer<'a> {
    src: &'a str,
    names: &'a Names,
    scope: Vec<String>,
}

type Res<T> = Result<T, String>;

fn unresolved(name: &str) -> String {
    format!("Could not resolve type: {name}")
}

/// A field number or range bound as Confluent writes a range: `max` from the largest field number.
fn bound(n: i64) -> String {
    if n >= MAX_FIELD {
        "max".into()
    } else {
        n.to_string()
    }
}

/// Reserved ranges and names, one to a statement, by start (names last, by name).
fn reserved_lines(rs: &[&Reserved], word: &str, opts: &[String]) -> Vec<String> {
    let mut items: Vec<(i64, String, String)> = Vec::new();
    for (r, o) in rs
        .iter()
        .zip(opts.iter().chain(std::iter::repeat(&String::new())))
    {
        for &(a, b) in &r.ranges {
            let text = if a == b {
                a.to_string()
            } else {
                format!("{a} to {}", bound(b))
            };
            let key = if a == b {
                a.to_string()
            } else {
                format!("{a}..{b}")
            };
            items.push((a, key, format!("{word} {text}{o};\n")));
        }
        for n in &r.names {
            items.push((i64::from(i32::MAX), n.clone(), format!("{word} \"{n}\";\n")));
        }
    }
    if word == "extensions" {
        items.sort_by_key(|x| x.0);
    } else {
        items.sort_by(|x, y| x.0.cmp(&y.0).then_with(|| java_cmp(&x.1, &y.1)));
    }
    items.into_iter().map(|x| x.2).collect()
}

fn is_scalar(t: &str) -> bool {
    SCALARS.contains(&t) && !t.is_empty()
}

/// Confluent's `toKind` of a field's type, for its `default`.
fn kind_of(simple: &str, default: &str) -> V {
    match simple {
        "bool" => V::Bool(default.to_owned()),
        "string" => V::Str(default.to_owned()),
        "bytes" | "double" | "float" | "fixed32" | "fixed64" | "int32" | "int64" | "sfixed32"
        | "sfixed64" | "sint32" | "sint64" | "uint32" | "uint64" => V::Num(default.to_owned()),
        _ => V::Enum(default.to_owned()),
    }
}

impl Printer<'_> {
    fn enter<T>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scope.push(name.to_owned());
        let out = f(self);
        self.scope.pop();
        out
    }

    fn type_name(&self, name: &str) -> Res<String> {
        self.names
            .resolve_type(name, &self.scope)
            .map(|r| format!(".{r}"))
            .ok_or_else(|| unresolved(name))
    }

    fn file(&mut self, f: &File) -> Res<String> {
        let mut sb = String::new();
        if let Some((_, s)) = &f.syntax
            && s == "proto3"
        {
            sb.push_str("syntax = \"proto3\";\n");
        }
        for d in &f.decls {
            if let Decl::Package(p) = d {
                sb.push_str(&format!("package {p};\n"));
            }
        }
        let mut imports: Vec<&str> = Vec::new();
        let mut public: Vec<&str> = Vec::new();
        for d in &f.decls {
            if let Decl::Import(kind, path, _) = d {
                if *kind == "public" {
                    public.push(path);
                } else {
                    imports.push(path);
                }
            }
        }
        if !imports.is_empty() || !public.is_empty() {
            sb.push('\n');
            for list in [&mut imports, &mut public] {
                list.sort_by(|a, b| java_cmp(a, b));
                list.dedup();
            }
            for i in &imports {
                sb.push_str(&format!("import \"{i}\";\n"));
            }
            for i in &public {
                sb.push_str(&format!("import public \"{i}\";\n"));
            }
        }
        let opts: Vec<&Opt> = f
            .decls
            .iter()
            .filter_map(|d| match d {
                Decl::Option(o) => Some(o),
                _ => None,
            })
            .collect();
        let opts = self.options(opts.iter().map(|o| named(self.src, o)).collect());
        if !opts.is_empty() {
            sb.push('\n');
            for o in &opts {
                sb.push_str(&format!("option {};\n", self.option(o)));
            }
        }
        if f.decls
            .iter()
            .any(|d| matches!(d, Decl::Message(_) | Decl::Enum(_)))
        {
            sb.push('\n');
            for d in &f.decls {
                if let Decl::Message(m) = d {
                    let s = self.enter(&m.name, |p| p.message(m))?;
                    sb.push_str(&s);
                }
            }
            for d in &f.decls {
                if let Decl::Enum(e) = d {
                    let s = self.enter(&e.name, |p| p.enumeration(e));
                    sb.push_str(&s);
                }
            }
        }
        let extends: Vec<&Extend> = f
            .decls
            .iter()
            .filter_map(|d| match d {
                Decl::Extend(e) => Some(e),
                _ => None,
            })
            .collect();
        if !extends.is_empty() {
            sb.push('\n');
            for e in self.extends(&extends)? {
                sb.push_str(&e);
            }
        }
        let services: Vec<&Service> = f
            .decls
            .iter()
            .filter_map(|d| match d {
                Decl::Service(s) => Some(s),
                _ => None,
            })
            .collect();
        if !services.is_empty() {
            sb.push('\n');
            for s in services {
                sb.push_str(&self.service(s)?);
            }
        }
        Ok(sb)
    }

    fn service(&self, s: &Service) -> Res<String> {
        let mut sb = format!("service {} {{", s.name);
        let opts = self.options(s.opts.iter().map(|o| named(self.src, o)).collect());
        if !opts.is_empty() {
            sb.push('\n');
            for o in &opts {
                indented(&mut sb, &format!("option {};\n", self.option(o)));
            }
        }
        if !s.rpcs.is_empty() {
            sb.push('\n');
            for r in &s.rpcs {
                let end = |(stream, name, _): &(bool, String, (u32, u32))| -> Res<String> {
                    Ok(format!(
                        "{}{}",
                        if *stream { "stream " } else { "" },
                        self.type_name(name)?
                    ))
                };
                let mut t = format!(
                    "rpc {} ({}) returns ({})",
                    r.name,
                    end(&r.input)?,
                    end(&r.output)?
                );
                let opts = self.options(r.opts.iter().map(|o| named(self.src, o)).collect());
                if !opts.is_empty() {
                    t.push_str(" {\n");
                    for o in &opts {
                        indented(&mut t, &format!("option {};\n", self.option(o)));
                    }
                    t.push('}');
                }
                t.push_str(";\n");
                indented(&mut sb, &t);
            }
        }
        sb.push_str("}\n");
        Ok(sb)
    }

    fn enumeration(&self, e: &Enum) -> String {
        let mut sb = format!("enum {} {{", e.name);
        let reserved: Vec<&Reserved> = e
            .body
            .iter()
            .filter_map(|i| match i {
                EnumItem::Reserved(r) => Some(r),
                _ => None,
            })
            .collect();
        let raw_opts: Vec<&Opt> = e
            .body
            .iter()
            .filter_map(|i| match i {
                EnumItem::Option(o) => Some(o),
                _ => None,
            })
            .collect();
        let mut values: Vec<&super::ast::EnumValue> = e
            .body
            .iter()
            .filter_map(|i| match i {
                EnumItem::Value(v) => Some(v),
                _ => None,
            })
            .collect();
        if !reserved.is_empty() {
            sb.push('\n');
            for l in reserved_lines(&reserved, "reserved", &[]) {
                indented(&mut sb, &l);
            }
        }
        if reserved.is_empty() && (!raw_opts.is_empty() || !values.is_empty()) {
            sb.push('\n');
        }
        for o in &self.options(raw_opts.iter().map(|o| named(self.src, o)).collect()) {
            indented(&mut sb, &format!("option {};\n", self.option(o)));
        }
        values.sort_by(|a, b| {
            a.number
                .cmp(&b.number)
                .then_with(|| java_cmp(&a.name, &b.name))
        });
        for v in values {
            let mut s = format!("{} = {}", v.name, v.number);
            let opts = self.options(v.opts.iter().map(|o| named(self.src, o)).collect());
            if !opts.is_empty() {
                s.push(' ');
                s.push_str(&self.option_list(&opts));
            }
            s.push_str(";\n");
            indented(&mut sb, &s);
        }
        sb.push_str("}\n");
        sb
    }

    fn message(&mut self, m: &Message) -> Res<String> {
        let mut sb = format!("message {} {{", m.name);
        let pick = |f: fn(&Item) -> bool| m.body.iter().filter(move |i| f(i));
        let reserved: Vec<&Reserved> = m
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Reserved(r) => Some(r),
                _ => None,
            })
            .collect();
        if !reserved.is_empty() {
            sb.push('\n');
            for l in reserved_lines(&reserved, "reserved", &[]) {
                indented(&mut sb, &l);
            }
        }
        let opts: Vec<Named> = pick(|i| matches!(i, Item::Option(_)))
            .filter_map(|i| match i {
                Item::Option(o) => Some(named(self.src, o)),
                _ => None,
            })
            .collect();
        let opts = self.options(opts);
        if !opts.is_empty() {
            sb.push('\n');
            for o in &opts {
                indented(&mut sb, &format!("option {};\n", self.option(o)));
            }
        }
        let mut fields: Vec<&Field> = m
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Field(f) => Some(f),
                _ => None,
            })
            .collect();
        if !fields.is_empty() {
            sb.push('\n');
            fields.sort_by_key(|f| f.number);
            for f in fields {
                indented(&mut sb, &self.field(f)?);
            }
        }
        let oneofs: Vec<&Oneof> = m
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Oneof(o) => Some(o),
                _ => None,
            })
            .collect();
        if !oneofs.is_empty() {
            sb.push('\n');
            let mut kept: Vec<(i64, &Oneof, Vec<&Field>)> = oneofs
                .iter()
                .filter_map(|o| {
                    let mut fs: Vec<&Field> = o
                        .body
                        .iter()
                        .filter_map(|i| match i {
                            Item::Field(f) => Some(f),
                            _ => None,
                        })
                        .collect();
                    fs.sort_by_key(|f| f.number);
                    Some((fs.first()?.number, *o, fs))
                })
                .collect();
            kept.sort_by_key(|k| k.0);
            for (_, o, fs) in kept {
                indented(&mut sb, &self.oneof(o, &fs)?);
            }
        }
        let mut groups: Vec<&Group> = m
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Group(g) => Some(g),
                _ => None,
            })
            .collect();
        if !groups.is_empty() {
            sb.push('\n');
            groups.sort_by_key(|g| g.number);
            for g in groups {
                indented(&mut sb, &self.group(g)?);
            }
        }
        let ext_ranges: Vec<(&Reserved, String)> = m
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Extensions(r, o) => {
                    let opts = self.options(o.iter().map(|o| named(self.src, o)).collect());
                    let text = if opts.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", self.option_list(&opts))
                    };
                    Some((r, text))
                }
                _ => None,
            })
            .collect();
        if !ext_ranges.is_empty() {
            sb.push('\n');
            let rs: Vec<&Reserved> = ext_ranges.iter().map(|x| x.0).collect();
            let os: Vec<String> = ext_ranges.iter().map(|x| x.1.clone()).collect();
            for l in reserved_lines(&rs, "extensions", &os) {
                indented(&mut sb, &l);
            }
        }
        let extends: Vec<&Extend> = m
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Extend(e) => Some(e),
                _ => None,
            })
            .collect();
        if !extends.is_empty() {
            sb.push('\n');
            for e in self.extends(&extends)? {
                indented(&mut sb, &e);
            }
        }
        let nested: Vec<&Item> = m
            .body
            .iter()
            .filter(|i| match i {
                Item::Message(n) => self
                    .names
                    .resolve_type(&n.name, &self.scope)
                    .and_then(|r| self.names.types.get(&r))
                    .is_none_or(|t| t.map.is_none()),
                Item::Enum(_) => true,
                _ => false,
            })
            .collect();
        if !nested.is_empty() {
            sb.push('\n');
            for i in &nested {
                if let Item::Message(n) = i {
                    let s = self.enter(&n.name, |p| p.message(n))?;
                    indented(&mut sb, &s);
                }
            }
            for i in &nested {
                if let Item::Enum(e) = i {
                    let s = self.enter(&e.name, |p| p.enumeration(e));
                    indented(&mut sb, &s);
                }
            }
        }
        sb.push_str("}\n");
        Ok(sb)
    }

    fn oneof(&self, o: &Oneof, fields: &[&Field]) -> Res<String> {
        let mut sb = format!("oneof {} {{", o.name);
        let opts: Vec<Named> = o
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Option(x) => Some(named(self.src, x)),
                _ => None,
            })
            .collect();
        let opts = self.options(opts);
        if !opts.is_empty() {
            sb.push('\n');
            for x in &opts {
                indented(&mut sb, &format!("option {};\n", self.option(x)));
            }
        }
        if !fields.is_empty() {
            sb.push('\n');
            for f in fields {
                indented(&mut sb, &self.field(f)?);
            }
        }
        let mut groups: Vec<&Group> = o
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Group(g) => Some(g),
                _ => None,
            })
            .collect();
        if !groups.is_empty() {
            sb.push('\n');
            groups.sort_by_key(|g| g.number);
            for g in groups {
                indented(&mut sb, &self.group(g)?);
            }
        }
        sb.push_str("}\n");
        Ok(sb)
    }

    fn group(&self, g: &Group) -> Res<String> {
        let label = g.label.map_or(String::new(), |l| format!("{} ", l.word()));
        let mut sb = format!("{label}group {} = {} {{", g.name, g.number);
        let mut fields: Vec<&Field> = g
            .body
            .iter()
            .filter_map(|i| match i {
                Item::Field(f) => Some(f),
                _ => None,
            })
            .collect();
        if !fields.is_empty() {
            sb.push('\n');
            fields.sort_by_key(|f| f.number);
            for f in fields {
                indented(&mut sb, &self.field(f)?);
            }
        }
        sb.push_str("}\n");
        Ok(sb)
    }

    /// Extends, those of one message type made one (in the order the types first appear), each's fields by number.
    fn extends(&self, es: &[&Extend]) -> Res<Vec<String>> {
        let mut grouped: Vec<(String, Vec<&Field>)> = Vec::new();
        for e in es {
            let name = self.type_name(&e.extendee)?;
            let fields = e.body.iter().filter_map(|i| match i {
                Item::Field(f) => Some(f),
                _ => None,
            });
            match grouped.iter_mut().find(|(n, _)| *n == name) {
                Some((_, v)) => v.extend(fields),
                None => grouped.push((name, fields.collect())),
            }
        }
        grouped
            .into_iter()
            .map(|(name, mut fields)| {
                let mut sb = format!("extend {name} {{");
                if !fields.is_empty() {
                    sb.push('\n');
                    fields.sort_by_key(|f| f.number);
                    for f in fields {
                        indented(&mut sb, &self.field(f)?);
                    }
                }
                sb.push_str("}\n");
                Ok(sb)
            })
            .collect()
    }

    fn field(&self, f: &Field) -> Res<String> {
        let mut label = f.label;
        let (ty, simple) = match &f.ty {
            TypeRef::Named(n, _) if is_scalar(n) => (n.clone(), n.clone()),
            TypeRef::Named(n, _) => {
                let full = self.type_name(n)?;
                let info = self.names.types.get(&full[1..]);
                match info.and_then(|t| t.map.as_ref()) {
                    Some((k, v)) => {
                        label = None;
                        (format!("map<{k}, {}>", self.map_value(v)), String::new())
                    }
                    None => {
                        let simple = full.rsplit('.').next().unwrap_or_default().to_owned();
                        (full, simple)
                    }
                }
            }
            TypeRef::Map(k, _, v, _) => {
                label = None;
                (format!("map<{k}, {}>", self.map_value(v)), String::new())
            }
        };
        let mut s = String::new();
        if let Some(l) = label {
            s.push_str(l.word());
            s.push(' ');
        }
        s.push_str(&format!("{ty} {} = {}", f.name, f.number));
        // Wire takes `default` and `json_name` out of the options and Confluent adds them back at the end.
        let mut opts: Vec<Named> = Vec::new();
        let mut default = None;
        let mut json = None;
        for o in &f.opts {
            match o.simple() {
                Some("default") if default.is_none() => default = Some(o),
                Some("json_name") if json.is_none() => json = Some(o),
                _ => opts.push(named(self.src, o)),
            }
        }
        if let Some(d) = default {
            let text = match &d.val {
                Val::Str => wire_unquote(
                    self.src
                        .get(d.val_span.0 as usize..d.val_span.1 as usize)
                        .unwrap_or_default(),
                ),
                Val::Ident(w) | Val::Num(w, _) => w.clone(),
                Val::Aggregate => String::new(),
            };
            let val = match kind_of(&simple, &text) {
                // A bytes default stays a string: Confluent writes it bare (`[default = abc]`), which no reader takes.
                V::Num(t) if simple == "bytes" => V::Str(t),
                v => v,
            };
            opts.push(Named {
                name: "default".into(),
                paren: false,
                val,
            });
        }
        if let Some(j) = json {
            opts.push(Named {
                name: "json_name".into(),
                paren: false,
                val: V::Str(wire_unquote(
                    self.src
                        .get(j.val_span.0 as usize..j.val_span.1 as usize)
                        .unwrap_or_default(),
                )),
            });
        }
        let opts = self.options(opts);
        if !opts.is_empty() {
            s.push(' ');
            s.push_str(&self.option_list(&opts));
        }
        s.push_str(";\n");
        Ok(s)
    }

    /// A map's value type, qualified when it names a type.
    fn map_value(&self, v: &str) -> String {
        self.names
            .resolve_type(v, &self.scope)
            .map_or_else(|| v.to_owned(), |r| format!(".{r}"))
    }

    /// Confluent's `filterOptions` when normalizing.
    fn options(&self, opts: Vec<Named>) -> Vec<Named> {
        let mut out: Vec<Named> = opts
            .into_iter()
            .map(|o| {
                if o.paren
                    && let Some(r) = self.names.resolve_ext(&o.name, &self.scope)
                {
                    return transform(Named { name: r, ..o });
                }
                o
            })
            .collect();
        out.sort_by(|a, b| java_cmp(&a.name, &b.name));
        let mut grouped: Vec<(String, Vec<Named>)> = Vec::new();
        for o in out {
            match grouped.iter_mut().find(|(n, _)| *n == o.name) {
                Some((_, v)) => v.push(o),
                None => grouped.push((o.name.clone(), vec![o])),
            }
        }
        let mut result = Vec::new();
        for (name, list) in grouped {
            match self.names.exts.get(&name).filter(|f| !f.repeated) {
                Some(root) => {
                    let mut it = list.into_iter();
                    if let Some(first) = it.next() {
                        result.push(it.fold(first, |a, b| self.merge(root, a, b)));
                    }
                }
                None => result.extend(list),
            }
        }
        result
    }

    /// Confluent's `mergeOption` (8.3.2).
    fn merge(&self, field: &FieldInfo, existing: Named, replacement: Named) -> Named {
        let replacement = transform(replacement);
        match (existing.val, replacement.val) {
            (V::Map(a), V::Map(b)) => Named {
                name: replacement.name,
                paren: replacement.paren,
                val: V::Map(self.merge_maps(Some(field), a, b)),
            },
            (_, val) => Named { val, ..replacement },
        }
    }

    fn message_fields(&self, field: Option<&FieldInfo>) -> Option<&Vec<FieldInfo>> {
        let f = field?;
        let full = self.names.resolve_type(&f.ty, &self.scope)?;
        self.names.types.get(&full)?.fields.as_ref()
    }

    fn merge_maps(
        &self,
        container: Option<&FieldInfo>,
        a: Vec<(String, V)>,
        b: Vec<(String, V)>,
    ) -> Vec<(String, V)> {
        let fields = self.message_fields(container);
        let mut merged = a;
        for (k, v) in b {
            let nested = fields.and_then(|fs| fs.iter().find(|f| f.name == k));
            match merged.iter_mut().find(|(x, _)| *x == k) {
                Some((_, old)) => {
                    let prev = std::mem::replace(old, V::List(Vec::new()));
                    *old = self.merge_values(nested, prev, v);
                }
                None => merged.push((k, v)),
            }
        }
        merged
    }

    fn merge_values(&self, field: Option<&FieldInfo>, v1: V, v2: V) -> V {
        let singular = field.is_some_and(|f| !f.repeated);
        if singular
            && matches!((&v1, &v2), (V::Map(_), V::Map(_)))
            && self.message_fields(field).is_some()
            && let (V::Map(a), V::Map(b)) = (v1.clone(), v2.clone())
        {
            return V::Map(self.merge_maps(field, a, b));
        }
        if singular {
            return v2;
        }
        let mut set: Vec<V> = Vec::new();
        for v in [v1, v2] {
            let items = match v {
                V::List(l) => l,
                v => vec![v],
            };
            for x in items {
                if !set.contains(&x) {
                    set.push(x);
                }
            }
        }
        V::List(set)
    }

    /// Confluent's `toString(OptionElement)`.
    fn option(&self, o: &Named) -> String {
        let mut sb = if o.paren {
            format!("({})", o.name)
        } else {
            o.name.clone()
        };
        match &o.val {
            V::Str(s) => sb.push_str(&format!(" = \"{}\"", escape(s))),
            V::Bool(w) | V::Enum(w) => sb.push_str(&format!(" = {w}")),
            V::Num(n) => sb.push_str(&format!(" = {}", java_format_number(n))),
            V::Opt(inner) => {
                sb.push('.');
                sb.push_str(&self.option(inner));
            }
            V::Map(m) => {
                sb.push_str(" = {\n");
                self.option_map(&mut sb, m);
                sb.push('}');
            }
            V::List(l) => {
                sb.push_str(" = ");
                sb.push_str(&self.value_list(l));
            }
        }
        sb
    }

    /// Confluent's `formatOptionMap` when normalizing: empty lists dropped, extension keys qualified, keys sorted.
    fn option_map(&self, sb: &mut String, m: &[(String, V)]) {
        let mut entries: Vec<(String, &V)> = Vec::new();
        for (k, v) in m {
            if matches!(v, V::List(l) if l.is_empty()) {
                continue;
            }
            let key = match k.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
                Some(inner) => self
                    .names
                    .resolve_ext(inner, &self.scope)
                    .map_or_else(|| k.clone(), |r| format!("[{r}]")),
                None => k.clone(),
            };
            if !entries.iter().any(|(x, _)| *x == key) {
                entries.push((key, v));
            }
        }
        entries.sort_by(|a, b| java_cmp(&a.0, &b.0));
        let last = entries.len().saturating_sub(1);
        for (i, (k, v)) in entries.iter().enumerate() {
            let end = if i == last { "" } else { "," };
            indented(sb, &format!("{k}: {}{end}", self.value(v)));
        }
    }

    /// Confluent's `formatOptionMapOrListValue`.
    fn value(&self, v: &V) -> String {
        match v {
            V::Str(s) => format!("\"{}\"", escape(s)),
            V::Map(m) => {
                let mut sb = "{\n".to_owned();
                self.option_map(&mut sb, m);
                sb.push('}');
                sb
            }
            V::List(l) if l.len() == 1 => self.value(&l[0]),
            V::List(l) => {
                let mut sb = "[\n".to_owned();
                let last = l.len().saturating_sub(1);
                for (i, x) in l.iter().enumerate() {
                    let end = if i == last { "" } else { "," };
                    indented(&mut sb, &format!("{}{end}", self.value(x)));
                }
                sb.push(']');
                sb
            }
            V::Bool(w) | V::Enum(w) => w.clone(),
            V::Num(n) => java_format_number(n),
            V::Opt(o) => self.option(o),
        }
    }

    /// Confluent's `appendOptions` of a list of values.
    fn value_list(&self, l: &[V]) -> String {
        if let [one] = l {
            return format!("[{}]", self.value(one));
        }
        let mut sb = "[\n".to_owned();
        let last = l.len().saturating_sub(1);
        for (i, x) in l.iter().enumerate() {
            let end = if i == last { "" } else { "," };
            indented(&mut sb, &format!("{}{end}", self.value(x)));
        }
        sb.push(']');
        sb
    }

    /// A field's or an enum value's options, as `appendOptions` writes them.
    fn option_list(&self, opts: &[Named]) -> String {
        if let [one] = opts {
            return format!("[{}]", self.option(one));
        }
        let mut sb = "[\n".to_owned();
        let last = opts.len().saturating_sub(1);
        for (i, o) in opts.iter().enumerate() {
            let end = if i == last { "" } else { "," };
            indented(&mut sb, &format!("{}{end}", self.option(o)));
        }
        sb.push(']');
        sb
    }
}

/// The normalized text of `file` (read from `src`), with the files `deps` (its references, and the well-known files)
/// giving the names it may use.
///
/// # Errors
/// A type name Confluent's resolution does not find: `Could not resolve type: <name>`.
pub(super) fn normalize(src: &str, file: &File, deps: &[File]) -> Result<String, String> {
    let mut names = Names::default();
    for d in deps {
        names.collect(d);
    }
    names.collect(file);
    let mut p = Printer {
        src,
        names: &names,
        scope: Vec::new(),
    };
    p.file(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_as_javas_decimal_format_writes_what_commons_lang_reads() {
        for (raw, want) in [
            ("1", "1"),
            ("-0", "0"),
            ("0x10", "16"),
            ("-0x10", "-16"),
            ("010", "8"),
            ("2.50", "2.5"),
            ("1e3", "1000"),
            ("1.23456", "1.235"),
            ("0.0625", "0.062"),
            ("0.0005", "0.001"),
            ("-0.0001", "-0"),
            ("1.1", "1.1"),
            ("inf", "inf"),
            ("-inf", "-inf"),
            ("nan", "nan"),
            (
                "123456789012345678901234567890",
                "123456789012345678901234567890",
            ),
            ("0.1234567890123456789", "0.123"),
            // commons-lang reads it as a float, which DecimalFormat writes as its double.
            ("1e20", "100000002004087730000"),
        ] {
            assert_eq!(java_format_number(raw), want, "{raw}");
        }
    }

    #[test]
    fn indenting_drops_trailing_empty_lines_as_java_splits() {
        let mut s = String::new();
        indented(&mut s, "a\n\nb\n");
        assert_eq!(s, "  a\n  \n  b\n");
        let mut s = String::new();
        indented(&mut s, "x");
        assert_eq!(s, "  x\n");
    }

    #[test]
    fn strings_are_read_as_wire_reads_them() {
        assert_eq!(wire_unquote(r#""a\x41\101\?\q" 'b'"#), "aAA?qb");
        assert_eq!(escape("a\"'\\\n\u{1}"), "a\\\"\\'\\\\\\n\u{1}");
    }
}
