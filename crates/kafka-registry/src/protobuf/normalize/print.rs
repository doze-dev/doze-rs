//! The file, its services, enums and messages, section by section.

use super::*;

impl Printer<'_> {
    pub(super) fn enter<T>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scope.push(name.to_owned());
        let out = f(self);
        self.scope.pop();
        out
    }

    pub(super) fn type_name(&self, name: &str) -> Res<String> {
        self.names
            .resolve_type(name, &self.scope)
            .map(|r| format!(".{r}"))
            .ok_or_else(|| unresolved(name))
    }

    pub(super) fn file(&mut self, f: &File) -> Res<String> {
        let mut sb = Self::head(f);
        let opts: Vec<Named> = f
            .decls
            .iter()
            .filter_map(|d| match d {
                Decl::Option(o) => Some(named(self.src, o)),
                _ => None,
            })
            .collect();
        let opts = self.options(opts);
        if !opts.is_empty() {
            sb.push('\n');
            for o in &opts {
                sb.push_str(&format!("option {};\n", self.option(o)));
            }
        }
        sb.push_str(&self.file_types(f)?);
        sb.push_str(&self.file_tail(f)?);
        Ok(sb)
    }

    /// The syntax (proto3 only), the package and the imports, sorted and each once.
    pub(super) fn head(f: &File) -> String {
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
        if imports.is_empty() && public.is_empty() {
            return sb;
        }
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
        sb
    }

    /// The file's messages, then its enums, in their order.
    pub(super) fn file_types(&mut self, f: &File) -> Res<String> {
        let mut sb = String::new();
        if !f
            .decls
            .iter()
            .any(|d| matches!(d, Decl::Message(_) | Decl::Enum(_)))
        {
            return Ok(sb);
        }
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
        Ok(sb)
    }

    /// The file's extends, then its services.
    pub(super) fn file_tail(&self, f: &File) -> Res<String> {
        let mut sb = String::new();
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

    pub(super) fn service(&self, s: &Service) -> Res<String> {
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

    pub(super) fn enumeration(&self, e: &Enum) -> String {
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
        let mut values: Vec<&crate::protobuf::ast::EnumValue> = e
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

    pub(super) fn message(&mut self, m: &Message) -> Res<String> {
        let mut sb = format!("message {} {{", m.name);
        sb.push_str(&self.message_head(m));
        sb.push_str(&self.message_members(m)?);
        sb.push_str(&self.message_ranges(m)?);
        sb.push_str(&self.message_nested(m)?);
        sb.push_str("}\n");
        Ok(sb)
    }

    /// A message's reserved ranges and names, then its options.
    pub(super) fn message_head(&self, m: &Message) -> String {
        let mut sb = String::new();
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
        sb
    }

    /// A message's fields, oneofs and groups, each by number.
    pub(super) fn message_members(&self, m: &Message) -> Res<String> {
        let mut sb = String::new();
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
        Ok(sb)
    }

    /// A message's extension ranges, then its extends.
    pub(super) fn message_ranges(&self, m: &Message) -> Res<String> {
        let mut sb = String::new();
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
        Ok(sb)
    }

    /// A message's nested messages (not a map's entry), then its nested enums.
    pub(super) fn message_nested(&mut self, m: &Message) -> Res<String> {
        let mut sb = String::new();
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
        Ok(sb)
    }
}
