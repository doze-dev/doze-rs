//! The first pass: every name a file declares (messages, enums and their values, fields, oneofs, groups and map entries
//! as messages, services, methods, extensions), each once.

use super::super::Error;
use super::super::ast::{Enum, EnumItem, Item, Message, TypeRef};
use super::{EnumInfo, Kind, Linker, Msg, Sym, Work, entry_name, join};

impl Linker<'_> {
    pub(super) fn declare(
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

    pub(super) fn new_msg(&mut self, full: String, file: usize, at: u32) -> Result<usize, Error> {
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

    pub(super) fn message<'a>(
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
    pub(super) fn body<'a>(
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

    pub(super) fn body_item<'a>(
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

    pub(super) fn enumeration<'a>(
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
    pub(super) fn declare_extensions(
        &mut self,
        file: usize,
        work: &[Work<'_>],
    ) -> Result<(), Error> {
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
}
