//! From `.proto` files to the descriptors the comparison reads: imports followed (registry references, and the
//! well-known types Confluent knows without one), names declared and resolved by Protobuf's scoping rules, maps and
//! groups made into their messages, and the rules of the language checked, as `protoc` (and the compiler the Go
//! edition used, `protocompile`) check them.

use std::collections::BTreeMap;

use super::Error;
use super::ast::{Decl, Enum, Extend, File, Item, Label, MAX_WORD, Reserved, TypeRef, parse};
use super::wkt;

/// A field's type: a scalar (by its descriptor type number), or a message, enum or group by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ty {
    Scalar(u8),
    Message(usize),
    Enum(usize),
    Group(usize),
}

/// The scalar types, at their descriptor type numbers (10, 11 and 14 are group, message and enum).
pub(super) const SCALARS: [&str; 19] = [
    "", "double", "float", "int64", "uint64", "int32", "fixed64", "fixed32", "bool", "string", "",
    "", "bytes", "uint32", "", "sfixed32", "sfixed64", "sint32", "sint64",
];

fn scalar(name: &str) -> Option<u8> {
    SCALARS
        .iter()
        .position(|s| !s.is_empty() && *s == name)
        .and_then(|p| u8::try_from(p).ok())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Card {
    Optional,
    Required,
    Repeated,
}

#[derive(Debug)]
pub(super) struct Fld {
    pub(super) name: String,
    pub(super) number: i32,
    pub(super) card: Card,
    /// `optional` written in proto3.
    pub(super) explicit: bool,
    pub(super) ty: Ty,
    pub(super) oneof: Option<usize>,
}

#[derive(Debug)]
pub(super) struct Msg {
    pub(super) full: String,
    pub(super) file: usize,
    pub(super) fields: Vec<Fld>,
    pub(super) oneofs: Vec<String>,
    pub(super) nested: Vec<usize>,
    pub(super) map_entry: bool,
    ext_ranges: Vec<(i64, i64)>,
}

#[derive(Debug)]
pub(super) struct EnumInfo {
    pub(super) file: usize,
    values: Vec<String>,
}

#[derive(Debug)]
pub(super) struct FileInfo {
    pub(super) package: String,
    pub(super) proto3: bool,
    /// Top-level messages, in order.
    pub(super) msgs: Vec<usize>,
    /// The files whose names this one sees: its imports and what they import publicly.
    visible: Vec<usize>,
    public: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Package,
    Message,
    Enum,
    Value,
    Field,
    Oneof,
    Service,
    Method,
    Extension,
}

#[derive(Debug, Clone, Copy)]
struct Sym {
    kind: Kind,
    file: usize,
    idx: usize,
}

/// Every file a schema reaches, linked.
#[derive(Debug, Default)]
pub(super) struct Pool {
    pub(super) files: Vec<FileInfo>,
    pub(super) msgs: Vec<Msg>,
    pub(super) enums: Vec<EnumInfo>,
    syms: BTreeMap<String, Sym>,
}

impl Pool {
    pub(super) fn msg(&self, i: usize) -> Option<&Msg> {
        self.msgs.get(i)
    }
}

/// What is left to link once every name is declared.
enum Work<'a> {
    Body(usize, &'a [Item], u32),
    Map(usize, &'a str, &'a str, u32),
    Extend(&'a Extend, String),
    Enum(&'a Enum, usize),
    Service(&'a super::ast::Service, String),
}

/// One file being linked.
struct Unit<'a> {
    ast: &'a File,
    file: usize,
}

/// Links the main file `ast` (read from `src`), whose imports are found in `sources` (path, text) or among the
/// well-known types. `descriptor` words an unresolved type as Confluent does for a descriptor.
///
/// # Errors
/// An import not found, or a rule of the language broken; the message says where.
pub(super) fn link(
    src: &str,
    ast: &File,
    sources: &[(&str, &str)],
    descriptor: bool,
) -> Result<Pool, String> {
    let mut l = Linker {
        pool: Pool::default(),
        sources,
        parsed: Vec::new(),
        loading: Vec::new(),
        descriptor,
        missing: None,
    };
    l.load_deps(ast)?;
    let main = l.pool.files.len();
    l.add_file(ast)?;
    let unit = Unit { ast, file: main };
    l.unit(&unit, true).map_err(|e| e.render(src))?;
    match l.missing {
        Some(path) => Err(format!("Could not resolve import: {path}")),
        None => Ok(l.pool),
    }
}

struct Linker<'a> {
    pool: Pool,
    sources: &'a [(&'a str, &'a str)],
    /// Dependencies read so far: path, source, its file.
    parsed: Vec<(String, usize)>,
    loading: Vec<String>,
    descriptor: bool,
    /// An import a descriptor names that no reference gives.
    missing: Option<String>,
}

fn join(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_owned()
    } else {
        format!("{scope}.{name}")
    }
}

impl Linker<'_> {
    /// Reads, declares and links each file `ast` imports, dependencies first.
    fn load_deps(&mut self, ast: &File) -> Result<(), String> {
        for d in &ast.decls {
            if let Decl::Import(_, path, _) = d {
                self.load(path)?;
            }
        }
        Ok(())
    }

    fn load(&mut self, path: &str) -> Result<usize, String> {
        if let Some((_, f)) = self.parsed.iter().find(|(p, _)| p == path) {
            return Ok(*f);
        }
        if self.loading.iter().any(|p| p == path) {
            return Err(format!("import cycle through \"{path}\""));
        }
        let src = self
            .sources
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, s)| *s)
            .or_else(|| wkt::source(path))
            // A descriptor's missing import shows as the types it should have given, as Confluent words it.
            .or_else(|| {
                self.missing.get_or_insert(path.to_owned());
                self.descriptor.then_some("")
            })
            .ok_or_else(|| {
                format!("import \"{path}\" was not found: no reference has that name")
            })?;
        self.loading.push(path.to_owned());
        let ast = parse(src).map_err(|e| format!("{path}: {}", e.render(src)))?;
        self.load_deps(&ast)?;
        let file = self.pool.files.len();
        self.add_file(&ast)?;
        let unit = Unit { ast: &ast, file };
        self.unit(&unit, false)
            .map_err(|e| format!("{path}: {}", e.render(src)))?;
        self.loading.pop();
        self.parsed.push((path.to_owned(), file));
        Ok(file)
    }

    /// The file's entry: package, syntax, which files it sees.
    fn add_file(&mut self, ast: &File) -> Result<(), String> {
        let mut info = FileInfo {
            package: String::new(),
            proto3: ast.syntax.as_ref().is_some_and(|(_, s)| s == "proto3"),
            msgs: Vec::new(),
            visible: Vec::new(),
            public: Vec::new(),
        };
        for d in &ast.decls {
            match d {
                Decl::Package(p) => info.package.clone_from(p),
                Decl::Import(kind, p, _) => {
                    let Some(&(_, f)) = self.parsed.iter().find(|(q, _)| q == p) else {
                        return Err(format!("import \"{p}\" was not found"));
                    };
                    let reach = self
                        .pool
                        .files
                        .get(f)
                        .map(|x| x.public.clone())
                        .unwrap_or_default();
                    info.visible.push(f);
                    info.visible.extend(&reach);
                    if *kind == "public" {
                        info.public.push(f);
                        info.public.extend(reach);
                    }
                }
                _ => {}
            }
        }
        self.pool.files.push(info);
        Ok(())
    }

    /// Declares every name of a file, then links what refers to names.
    fn unit(&mut self, u: &Unit<'_>, check: bool) -> Result<(), Error> {
        let package = self
            .pool
            .files
            .get(u.file)
            .map(|f| f.package.clone())
            .unwrap_or_default();
        let mut scope = String::new();
        for part in package.split('.').filter(|p| !p.is_empty()) {
            scope = join(&scope, part);
            if let Some(s) = self
                .pool
                .syms
                .get(&scope)
                .filter(|s| s.kind != Kind::Package)
            {
                let _ = s;
                return Err(Error::at(
                    0,
                    format!("package {scope} clashes with a type of that name"),
                ));
            }
            self.pool.syms.insert(
                scope.clone(),
                Sym {
                    kind: Kind::Package,
                    file: u.file,
                    idx: 0,
                },
            );
        }
        let mut work = Vec::new();
        for d in &u.ast.decls {
            match d {
                Decl::Message(m) => {
                    let i = self.message(u.file, &package, m, &mut work)?;
                    if let Some(f) = self.pool.files.get_mut(u.file) {
                        f.msgs.push(i);
                    }
                }
                Decl::Enum(e) => self.enumeration(u.file, &package, e, &mut work)?,
                Decl::Extend(e) => work.push(Work::Extend(e, package.clone())),
                Decl::Service(s) => {
                    let full = join(&package, &s.name);
                    self.declare(&full, Kind::Service, u.file, 0, s.at)?;
                    for r in &s.rpcs {
                        self.declare(&join(&full, &r.name), Kind::Method, u.file, 0, r.at)?;
                    }
                    work.push(Work::Service(s, package.clone()));
                }
                _ => {}
            }
        }
        self.declare_extensions(u.file, &work)?;
        for w in &work {
            self.work(u, w, check)?;
        }
        if check {
            self.check_file_options(u, &package)?;
        }
        Ok(())
    }
}

fn card(l: Option<Label>) -> Card {
    match l {
        Some(Label::Required) => Card::Required,
        Some(Label::Repeated) => Card::Repeated,
        _ => Card::Optional,
    }
}

/// The message a map field's entries are: its name in camel case, then `Entry`.
fn entry_name(field: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in field.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out + "Entry"
}

/// A field's JSON name by default: its name in lower camel case.
pub(super) fn json_name(field: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in field.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The rules of the language, checked on the schema being registered.
mod check;

/// A reserved statement's ranges, `max` made `max`.
fn ranges_of(r: &Reserved, max: i64) -> impl Iterator<Item = (i64, i64)> + '_ {
    r.ranges
        .iter()
        .map(move |&(a, b)| (a, if b == MAX_WORD { max } else { b }))
}

/// The first pass: declarations.
mod declare;

/// The second pass: resolution.
mod resolve;
