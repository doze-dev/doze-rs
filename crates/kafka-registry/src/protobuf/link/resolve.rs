//! The second pass: names resolved by Protobuf's scoping, fields given their types, and each declaration checked.

use super::super::Error;
use super::super::ast::{Extend, Field, Group, Item, Label, MAX_FIELD, MAX_WORD, TypeRef};
use super::{Card, Fld, Kind, Linker, Sym, Ty, Unit, Work, card, entry_name, join, scalar};

impl Linker<'_> {
    pub(super) fn sees(&self, file: usize, s: &Sym) -> bool {
        s.kind == Kind::Package
            || s.file == file
            || self
                .pool
                .files
                .get(file)
                .is_some_and(|f| f.visible.contains(&s.file))
    }

    /// Resolves `name` from `scope` as Protobuf does: the innermost scope that has its first part decides.
    pub(super) fn resolve(
        &self,
        name: &str,
        scope: &str,
        file: usize,
        want: fn(Kind) -> bool,
    ) -> Option<Sym> {
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

    pub(super) fn unresolved(&self, name: &str, at: u32) -> Error {
        if self.descriptor {
            Error::plain(format!("Could not resolve type: {name}"))
        } else {
            Error::at(at, format!("unknown type {name}"))
        }
    }

    /// A field's type from its name.
    pub(super) fn ty(&self, name: &str, scope: &str, file: usize, at: u32) -> Result<Ty, Error> {
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

    pub(super) fn work(&mut self, u: &Unit<'_>, w: &Work<'_>, check: bool) -> Result<(), Error> {
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

    pub(super) fn proto3(&self, file: usize) -> bool {
        self.pool.files.get(file).is_some_and(|f| f.proto3)
    }

    /// The fields of a message (and of its oneofs), linked and checked.
    pub(super) fn link_body(
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

    pub(super) fn field(
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

    pub(super) fn group(
        &self,
        u: &Unit<'_>,
        g: &Group,
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

    pub(super) fn link_extend(
        &self,
        u: &Unit<'_>,
        e: &Extend,
        scope: &str,
        check: bool,
    ) -> Result<(), Error> {
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
