//! From `.proto` files to the descriptors the comparison reads: imports followed (registry references, and the
//! well-known types Confluent knows without one), names declared and resolved by Protobuf's scoping rules, maps and
//! groups made into their messages, and the rules of the language checked, as `protoc` (and the compiler the Go
//! edition used, `protocompile`) check them.

use std::collections::BTreeMap;

use super::Error;
use super::ast::{
    Decl, Enum, EnumItem, Extend, Field, File, Item, Label, MAX_FIELD, MAX_WORD, Message, Reserved,
    TypeRef, parse,
};
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
                Decl::Package(p, _) => info.package.clone_from(p),
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

    fn declare(
        &mut self,
        full: &str,
        kind: Kind,
        file: usize,
        idx: usize,
        at: u32,
    ) -> Result<(), Error> {
        if let Some(old) = self.pool.syms.get(full)
            && !(old.kind == Kind::Package && kind == Kind::Package)
        {
            return Err(Error::at(at, format!("\"{full}\" is already defined")));
        }
        self.pool
            .syms
            .insert(full.to_owned(), Sym { kind, file, idx });
        Ok(())
    }

    fn new_msg(&mut self, full: String, file: usize, at: u32) -> Result<usize, Error> {
        let i = self.pool.msgs.len();
        self.declare(&full, Kind::Message, file, i, at)?;
        self.pool.msgs.push(Msg {
            full,
            file,
            fields: Vec::new(),
            oneofs: Vec::new(),
            nested: Vec::new(),
            map_entry: false,
            ext_ranges: Vec::new(),
        });
        Ok(i)
    }

    fn message<'a>(
        &mut self,
        file: usize,
        scope: &str,
        m: &'a Message,
        work: &mut Vec<Work<'a>>,
    ) -> Result<usize, Error> {
        let i = self.new_msg(join(scope, &m.name), file, m.at)?;
        self.body(file, i, &m.body, m.at, work)?;
        Ok(i)
    }

    /// Declares what a message body declares: nested types, groups and map entries (as messages), fields and oneofs.
    fn body<'a>(
        &mut self,
        file: usize,
        i: usize,
        items: &'a [Item],
        at: u32,
        work: &mut Vec<Work<'a>>,
    ) -> Result<(), Error> {
        let full = self.pool.msg(i).map(|m| m.full.clone()).unwrap_or_default();
        work.push(Work::Body(i, items, at));
        for item in items {
            self.body_item(file, i, &full, item, work)?;
        }
        Ok(())
    }

    fn body_item<'a>(
        &mut self,
        file: usize,
        i: usize,
        full: &str,
        item: &'a Item,
        work: &mut Vec<Work<'a>>,
    ) -> Result<(), Error> {
        let nest = |l: &mut Self, n: usize| {
            if let Some(m) = l.pool.msgs.get_mut(i) {
                m.nested.push(n);
            }
        };
        match item {
            Item::Message(m) => {
                let n = self.message(file, full, m, work)?;
                nest(self, n);
            }
            Item::Enum(e) => self.enumeration(file, full, e, work)?,
            Item::Field(f) => {
                self.declare(&join(full, &f.name), Kind::Field, file, 0, f.at)?;
                if let TypeRef::Map(k, _, v, _) = &f.ty {
                    let n = self.new_msg(join(full, &entry_name(&f.name)), file, f.at)?;
                    if let Some(m) = self.pool.msgs.get_mut(n) {
                        m.map_entry = true;
                    }
                    nest(self, n);
                    work.push(Work::Map(n, k, v, f.at));
                }
            }
            Item::Group(g) => {
                if !g.name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    return Err(Error::at(
                        g.at,
                        format!("group {} must start with a capital letter", g.name),
                    ));
                }
                self.declare(
                    &join(full, &g.name.to_ascii_lowercase()),
                    Kind::Field,
                    file,
                    0,
                    g.at,
                )?;
                let n = self.new_msg(join(full, &g.name), file, g.at)?;
                nest(self, n);
                self.body(file, n, &g.body, g.at, work)?;
            }
            Item::Oneof(o) => {
                self.declare(&join(full, &o.name), Kind::Oneof, file, 0, o.at)?;
                for d in &o.body {
                    self.body_item(file, i, full, d, work)?;
                }
            }
            Item::Extend(e) => work.push(Work::Extend(e, full.to_owned())),
            Item::Option(_) | Item::Reserved(_) | Item::Extensions(..) => {}
        }
        Ok(())
    }

    fn enumeration<'a>(
        &mut self,
        file: usize,
        scope: &str,
        e: &'a Enum,
        work: &mut Vec<Work<'a>>,
    ) -> Result<(), Error> {
        let full = join(scope, &e.name);
        let i = self.pool.enums.len();
        self.declare(&full, Kind::Enum, file, i, e.at)?;
        let mut values = Vec::new();
        for d in &e.body {
            if let EnumItem::Value(v) = d {
                // Enum values are siblings of their enum, as in C++.
                self.declare(&join(scope, &v.name), Kind::Value, file, i, v.at)?;
                values.push(v.name.clone());
            }
        }
        self.pool.enums.push(EnumInfo { file, values });
        work.push(Work::Enum(e, i));
        Ok(())
    }

    /// Extension fields are names too, declared before any option may name them.
    fn declare_extensions(&mut self, file: usize, work: &[Work<'_>]) -> Result<(), Error> {
        for w in work {
            if let Work::Extend(e, scope) = w {
                for item in &e.body {
                    let (name, at) = match item {
                        Item::Field(f) => (f.name.clone(), f.at),
                        Item::Group(g) => (g.name.to_ascii_lowercase(), g.at),
                        _ => continue,
                    };
                    self.declare(&join(scope, &name), Kind::Extension, file, 0, at)?;
                }
            }
        }
        Ok(())
    }

    fn sees(&self, file: usize, s: &Sym) -> bool {
        s.kind == Kind::Package
            || s.file == file
            || self
                .pool
                .files
                .get(file)
                .is_some_and(|f| f.visible.contains(&s.file))
    }

    /// Resolves `name` from `scope` as Protobuf does: the innermost scope that has its first part decides.
    fn resolve(&self, name: &str, scope: &str, file: usize, want: fn(Kind) -> bool) -> Option<Sym> {
        let find = |full: &str| {
            self.pool
                .syms
                .get(full)
                .copied()
                .filter(|s| self.sees(file, s))
        };
        if let Some(abs) = name.strip_prefix('.') {
            return find(abs).filter(|s| want(s.kind));
        }
        let first = name.split('.').next().unwrap_or(name);
        let mut scope = scope.to_owned();
        loop {
            if let Some(s) = find(&join(&scope, first)) {
                if first.len() < name.len() {
                    if matches!(
                        s.kind,
                        Kind::Package | Kind::Message | Kind::Enum | Kind::Service
                    ) {
                        return find(&join(&scope, name)).filter(|s| want(s.kind));
                    }
                } else if want(s.kind) {
                    return Some(s);
                }
            }
            if scope.is_empty() {
                return None;
            }
            scope = scope.rfind('.').map_or(String::new(), |i| {
                scope.get(..i).unwrap_or_default().to_owned()
            });
        }
    }

    fn unresolved(&self, name: &str, at: u32) -> Error {
        if self.descriptor {
            Error::plain(format!("Could not resolve type: {name}"))
        } else {
            Error::at(at, format!("unknown type {name}"))
        }
    }

    /// A field's type from its name.
    fn ty(&self, name: &str, scope: &str, file: usize, at: u32) -> Result<Ty, Error> {
        if let Some(t) = scalar(name) {
            return Ok(Ty::Scalar(t));
        }
        let s = self
            .resolve(name, scope, file, |k| {
                matches!(k, Kind::Message | Kind::Enum)
            })
            .ok_or_else(|| self.unresolved(name, at))?;
        Ok(if s.kind == Kind::Enum {
            Ty::Enum(s.idx)
        } else {
            Ty::Message(s.idx)
        })
    }

    fn work(&mut self, u: &Unit<'_>, w: &Work<'_>, check: bool) -> Result<(), Error> {
        match w {
            Work::Body(i, items, at) => self.link_body(u, *i, items, *at, check),
            Work::Map(i, k, v, at) => {
                let scope = self
                    .pool
                    .msg(*i)
                    .map(|m| m.full.clone())
                    .unwrap_or_default();
                let key = self.ty(k, &scope, u.file, *at)?;
                if check && !matches!(key, Ty::Scalar(t) if !matches!(t, 1 | 2 | 12)) {
                    return Err(Error::at(
                        *at,
                        format!("map key type {k} is not an integer, bool or string"),
                    ));
                }
                let val = self.ty(v, &scope, u.file, *at)?;
                let field = |name: &str, number, ty| Fld {
                    name: name.to_owned(),
                    number,
                    card: Card::Optional,
                    explicit: false,
                    ty,
                    oneof: None,
                };
                if let Some(m) = self.pool.msgs.get_mut(*i) {
                    m.fields = vec![field("key", 1, key), field("value", 2, val)];
                }
                Ok(())
            }
            Work::Extend(e, scope) => self.link_extend(u, e, scope, check),
            Work::Enum(e, i) => {
                if check {
                    self.check_enum(u, e, *i)?;
                }
                Ok(())
            }
            Work::Service(s, scope) => {
                let full = join(scope, &s.name);
                for r in &s.rpcs {
                    for (_, t, span) in [&r.input, &r.output] {
                        if self
                            .resolve(t, &full, u.file, |k| k == Kind::Message)
                            .is_none()
                        {
                            return Err(self.unresolved(t, span.0));
                        }
                    }
                    if check {
                        self.check_options(u, &r.opts, "R", &full)?;
                    }
                }
                if check {
                    self.check_options(u, &s.opts, "S", &full)?;
                }
                Ok(())
            }
        }
    }

    fn proto3(&self, file: usize) -> bool {
        self.pool.files.get(file).is_some_and(|f| f.proto3)
    }

    /// The fields of a message (and of its oneofs), linked and checked.
    fn link_body(
        &mut self,
        u: &Unit<'_>,
        i: usize,
        items: &[Item],
        at: u32,
        check: bool,
    ) -> Result<(), Error> {
        let full = self.pool.msg(i).map(|m| m.full.clone()).unwrap_or_default();
        let mut fields = Vec::new();
        let mut oneofs = Vec::new();
        let mut ranges = Vec::new();
        for item in items {
            match item {
                Item::Field(f) => fields.push(self.field(u, f, &full, None, check)?),
                Item::Group(g) => fields.push(self.group(u, g, &full, None, check)?),
                Item::Oneof(o) => {
                    let n = oneofs.len();
                    oneofs.push(o.name.clone());
                    let before = fields.len();
                    for d in &o.body {
                        match d {
                            Item::Field(f) => {
                                fields.push(self.field(u, f, &full, Some(n), check)?)
                            }
                            Item::Group(g) => {
                                fields.push(self.group(u, g, &full, Some(n), check)?)
                            }
                            Item::Option(op) if check => {
                                self.check_options(u, std::slice::from_ref(op), "O", &full)?
                            }
                            _ => {}
                        }
                    }
                    if fields.len() == before {
                        return Err(Error::at(o.at, format!("oneof {} has no fields", o.name)));
                    }
                }
                Item::Extensions(r, opts) => {
                    if check && self.proto3(u.file) {
                        return Err(Error::at(
                            r.at,
                            "extension ranges are not allowed in proto3".into(),
                        ));
                    }
                    if check {
                        self.check_options(u, opts, "X", &full)?;
                    }
                    ranges.extend(
                        r.ranges
                            .iter()
                            .map(|&(a, b)| (a, if b == MAX_WORD { MAX_FIELD } else { b })),
                    );
                }
                Item::Option(o) if check => {
                    self.check_options(u, std::slice::from_ref(o), "M", &full)?
                }
                _ => {}
            }
        }
        if let Some(m) = self.pool.msgs.get_mut(i) {
            m.fields = fields;
            m.oneofs = oneofs;
            m.ext_ranges = ranges;
        }
        if check {
            self.check_message(u, i, items, at)?;
        }
        Ok(())
    }

    fn field(
        &self,
        u: &Unit<'_>,
        f: &Field,
        scope: &str,
        oneof: Option<usize>,
        check: bool,
    ) -> Result<Fld, Error> {
        let (ty, card) = match &f.ty {
            TypeRef::Named(n, _) => (self.ty(n, scope, u.file, f.at)?, card(f.label)),
            TypeRef::Map(..) => {
                let s = self
                    .pool
                    .syms
                    .get(&join(scope, &entry_name(&f.name)))
                    .copied()
                    .ok_or_else(|| Error::at(f.at, "map entry missing".into()))?;
                (Ty::Message(s.idx), Card::Repeated)
            }
        };
        let fld = Fld {
            name: f.name.clone(),
            number: i32::try_from(f.number).unwrap_or(i32::MAX),
            card,
            explicit: f.label == Some(Label::Optional) && self.proto3(u.file),
            ty,
            oneof,
        };
        if check {
            self.check_field(u, f, &fld, scope)?;
        }
        Ok(fld)
    }

    fn group(
        &self,
        u: &Unit<'_>,
        g: &super::ast::Group,
        scope: &str,
        oneof: Option<usize>,
        check: bool,
    ) -> Result<Fld, Error> {
        if check && self.proto3(u.file) {
            return Err(Error::at(g.at, "groups are not allowed in proto3".into()));
        }
        if check && g.label.is_none() && oneof.is_none() {
            return Err(Error::at(
                g.at,
                format!("group {}: a label is required in proto2", g.name),
            ));
        }
        let s = self
            .pool
            .syms
            .get(&join(scope, &g.name))
            .copied()
            .ok_or_else(|| Error::at(g.at, "group missing".into()))?;
        if check {
            self.check_number(g.number, g.at)?;
            self.check_options(u, &g.opts, "D", scope)?;
        }
        Ok(Fld {
            name: g.name.to_ascii_lowercase(),
            number: i32::try_from(g.number).unwrap_or(i32::MAX),
            card: card(g.label),
            explicit: false,
            ty: Ty::Group(s.idx),
            oneof,
        })
    }

    fn link_extend(&self, u: &Unit<'_>, e: &Extend, scope: &str, check: bool) -> Result<(), Error> {
        let target = self
            .resolve(&e.extendee, scope, u.file, |k| k == Kind::Message)
            .ok_or_else(|| self.unresolved(&e.extendee, e.at))?;
        let Some(m) = self.pool.msg(target.idx) else {
            return Ok(());
        };
        if check
            && self.proto3(u.file)
            && !(m.full.starts_with("google.protobuf.") && m.full.ends_with("Options"))
        {
            return Err(Error::at(
                e.at,
                "extensions in proto3 may only define custom options".into(),
            ));
        }
        for item in &e.body {
            let (number, at) = match item {
                Item::Field(f) => {
                    if matches!(f.ty, TypeRef::Map(..)) || f.label == Some(Label::Required) {
                        return Err(Error::at(
                            f.at,
                            "an extension may not be a map or required".into(),
                        ));
                    }
                    self.field(u, f, scope, None, check)?;
                    (f.number, f.at)
                }
                Item::Group(g) => (g.number, g.at),
                _ => continue,
            };
            if check
                && !m
                    .ext_ranges
                    .iter()
                    .any(|&(a, b)| a <= number && number <= b)
            {
                return Err(Error::at(
                    at,
                    format!("{} has no extension range for number {number}", m.full),
                ));
            }
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
