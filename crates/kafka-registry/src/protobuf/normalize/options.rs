//! Options: sorted, named by their extension, merged, and written as Confluent writes them.

use super::*;

impl Printer<'_> {
    /// Confluent's `filterOptions` when normalizing.
    pub(super) fn options(&self, opts: Vec<Named>) -> Vec<Named> {
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
    pub(super) fn merge(&self, field: &FieldInfo, existing: Named, replacement: Named) -> Named {
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

    pub(super) fn message_fields(&self, field: Option<&FieldInfo>) -> Option<&Vec<FieldInfo>> {
        let f = field?;
        let full = self.names.resolve_type(&f.ty, &self.scope)?;
        self.names.types.get(&full)?.fields.as_ref()
    }

    pub(super) fn merge_maps(
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

    pub(super) fn merge_values(&self, field: Option<&FieldInfo>, v1: V, v2: V) -> V {
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
    pub(super) fn option(&self, o: &Named) -> String {
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
    pub(super) fn option_map(&self, sb: &mut String, m: &[(String, V)]) {
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
    pub(super) fn value(&self, v: &V) -> String {
        match v {
            V::Str(s) => format!("\"{}\"", escape(s)),
            V::Map(m) => {
                let mut sb = "{\n".to_owned();
                self.option_map(&mut sb, m);
                sb.push('}');
                sb
            }
            V::List(l) if l.len() == 1 => l.first().map_or_else(String::new, |x| self.value(x)),
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
    pub(super) fn value_list(&self, l: &[V]) -> String {
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
    pub(super) fn option_list(&self, opts: &[Named]) -> String {
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
