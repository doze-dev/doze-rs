//! `meta.yaml`, the docs prose the registry site renders, from the module's own manifest. Keys and type spellings as the
//! Go `modtool` writes them (the site's ArgTable reads them). No versions field: engine support lives in the index.

use doze_module_proto::msg::hello::{Attr, Manifest, Schema, Type};
use doze_module_proto::msg::value::Value;
use yaml_rust2::YamlEmitter;
use yaml_rust2::yaml::{Hash, Yaml};

fn key(k: &str) -> Yaml {
    Yaml::String(k.to_string())
}

/// The type as the Go descriptions spell it.
pub(crate) fn type_name(t: &Type) -> String {
    match t {
        Type::String => "string".into(),
        Type::Number => "number".into(),
        Type::Bool => "bool".into(),
        Type::Any => "any".into(),
        Type::List(inner) => format!("list({})", type_name(inner)),
        Type::Map(inner) => format!("map({})", type_name(inner)),
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) | Value::String(n) => n.clone(),
        Value::List(_) | Value::Map(_) => String::from("…"),
    }
}

fn opt(h: &mut Hash, k: &str, v: Option<&str>) {
    if let Some(v) = v.filter(|v| !v.is_empty()) {
        h.insert(key(k), key(v));
    }
}

fn arg(a: &Attr) -> Yaml {
    let mut h = Hash::new();
    h.insert(key("name"), key(&a.name));
    h.insert(key("type"), key(&type_name(&a.ty)));
    let default = a.default.as_ref().map(value_text);
    opt(&mut h, "default", default.as_deref());
    opt(&mut h, "desc", Some(&a.doc));
    if a.required {
        h.insert(key("required"), Yaml::Boolean(true));
    }
    opt(&mut h, "since", a.since.as_deref());
    opt(&mut h, "until", a.until.as_deref());
    Yaml::Hash(h)
}

fn config(s: &Schema) -> Yaml {
    let mut h = Hash::new();
    h.insert(
        key("arguments"),
        Yaml::Array(s.attrs.iter().map(arg).collect()),
    );
    if !s.blocks.is_empty() {
        let blocks = s.blocks.iter().map(|b| {
            let mut bh = Hash::new();
            bh.insert(key("name"), key(&b.name));
            opt(&mut bh, "label", b.labels.first().map(String::as_str));
            opt(&mut bh, "desc", Some(&b.doc));
            bh.insert(
                key("arguments"),
                Yaml::Array(b.schema.attrs.iter().map(arg).collect()),
            );
            Yaml::Hash(bh)
        });
        h.insert(key("blocks"), Yaml::Array(blocks.collect()));
    }
    Yaml::Hash(h)
}

/// The meta.yaml text for a module.
pub(crate) fn render(m: &Manifest) -> Result<String, String> {
    let mut h = Hash::new();
    h.insert(key("title"), key(&m.title));
    h.insert(key("tagline"), key(&m.tagline));
    h.insert(key("category"), key(&m.category));
    h.insert(key("engine"), key(&m.engine));
    if m.default_port != 0 {
        h.insert(key("port"), Yaml::Integer(i64::from(m.default_port)));
    }
    opt(&mut h, "example", Some(&m.example));
    opt(&mut h, "exampleLabel", m.example_label.as_deref());
    opt(&mut h, "description", m.description.as_deref());
    opt(&mut h, "homepage", m.homepage.as_deref());
    opt(&mut h, "source", m.source.as_deref());
    h.insert(key("config"), config(&m.schema));
    let mut out = String::new();
    YamlEmitter::new(&mut out)
        .dump(&Yaml::Hash(h))
        .map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}
