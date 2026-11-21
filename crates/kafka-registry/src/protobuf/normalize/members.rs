//! A message's oneofs, groups, extends and fields.

use super::*;

impl Printer<'_> {
    pub(super) fn oneof(&self, o: &Oneof, fields: &[&Field]) -> Res<String> {
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

    pub(super) fn group(&self, g: &Group) -> Res<String> {
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
    pub(super) fn extends(&self, es: &[&Extend]) -> Res<Vec<String>> {
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

    pub(super) fn field(&self, f: &Field) -> Res<String> {
        let mut label = f.label;
        let (ty, simple) = match &f.ty {
            TypeRef::Named(n, _) if is_scalar(n) => (n.clone(), n.clone()),
            TypeRef::Named(n, _) => {
                let full = self.type_name(n)?;
                let info = self.names.types.get(full.get(1..).unwrap_or_default());
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
        let opts = self.options(self.field_options(f, &simple));
        if !opts.is_empty() {
            s.push(' ');
            s.push_str(&self.option_list(&opts));
        }
        s.push_str(";\n");
        Ok(s)
    }

    /// A field's options as Wire keeps them: `default` and `json_name` taken out, and added back at the end by Confluent
    /// (a default of the kind its type gives, `simple` being the type's last name).
    pub(super) fn field_options(&self, f: &Field, simple: &str) -> Vec<Named> {
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
            let val = match kind_of(simple, &text) {
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
        opts
    }

    /// A map's value type, qualified when it names a type.
    pub(super) fn map_value(&self, v: &str) -> String {
        self.names
            .resolve_type(v, &self.scope)
            .map_or_else(|| v.to_owned(), |r| format!(".{r}"))
    }
}
