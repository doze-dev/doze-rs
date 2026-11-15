//! A module block as the module receives it (PLAN.md D1, the contract of sdk.md): doze-config's evaluated body made the
//! protocol's value tree. An argument is a map entry; a nested block type is a map entry holding a list of maps, one per
//! block; a block's labels are entries of its map, named as the module's schema names them. The core's own arguments
//! (`port`, `version`, `after`…) were taken by doze-config and never reach the module.
//!
//! doze-config hands over values, not syntax, so the nodes carry no source span (a module's diagnostic is mapped to the
//! block by its path).

use doze_config::model::ModuleBody;
use doze_hcl::Value as Hcl;
use doze_module_proto::msg::hello::Schema;
use doze_module_proto::msg::value::{Node, Value};
use std::collections::BTreeMap;

/// The block `body`, read against the module's `schema` (for its blocks' label names).
pub fn tree(body: &ModuleBody, schema: &Schema) -> Node {
    Node::bare(Value::Map(map(body, schema, BTreeMap::new())))
}

fn map(
    body: &ModuleBody,
    schema: &Schema,
    labels: BTreeMap<String, Node>,
) -> BTreeMap<String, Node> {
    let mut out = labels;
    for (k, v) in &body.attrs {
        out.insert(k.clone(), Node::bare(value(v)));
    }
    let mut nested: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for b in &body.blocks {
        let inner = schema.blocks.iter().find(|s| s.name == b.ty);
        let (names, sub) = inner.map_or_else(
            || (Vec::new(), Schema::default()),
            |s| (s.labels.clone(), s.schema.clone()),
        );
        let labelled = b
            .labels
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let key = names.get(i).cloned().unwrap_or_else(|| format!("label{i}"));
                (key, Node::bare(Value::String(l.clone())))
            })
            .collect();
        let node = Node::bare(Value::Map(map(&b.body, &sub, labelled)));
        nested.entry(b.ty.clone()).or_default().push(node);
    }
    for (k, items) in nested {
        out.insert(k, Node::bare(Value::List(items)));
    }
    out
}

/// One evaluated value. An unknown (there are none after a full load) travels as null.
pub fn value(v: &Hcl) -> Value {
    match v {
        Hcl::Null(_) | Hcl::Unknown(..) => Value::Null,
        Hcl::Bool(b) => Value::Bool(*b),
        Hcl::Number(n) => Value::Number(n.to_string()),
        Hcl::String(s) => Value::String(s.clone()),
        Hcl::List(_, xs) | Hcl::Set(_, xs) | Hcl::Tuple(xs) => {
            Value::List(xs.iter().map(|x| Node::bare(value(x))).collect())
        }
        Hcl::Map(_, m) | Hcl::Object(m) => Value::Map(
            m.iter()
                .map(|(k, x)| (k.clone(), Node::bare(value(x))))
                .collect(),
        ),
    }
}

/// A value a module published (its `attributes`), as the config's references read it.
pub fn back(v: &Value) -> Hcl {
    match v {
        Value::Null => Hcl::Null(doze_hcl::Type::Dynamic),
        Value::Bool(b) => Hcl::Bool(*b),
        Value::Number(n) => doze_hcl::Number::parse(n).map_or_else(|| Hcl::string(n), Hcl::number),
        Value::String(s) => Hcl::string(s),
        Value::List(xs) => Hcl::Tuple(xs.iter().map(|n| back(&n.value)).collect()),
        Value::Map(m) => Hcl::Object(m.iter().map(|(k, n)| (k.clone(), back(&n.value))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use doze_config::model::ModuleBlock;
    use doze_module_proto::msg::hello::BlockSchema;

    fn schema() -> Schema {
        Schema {
            attrs: vec![],
            blocks: vec![BlockSchema {
                name: "database".into(),
                labels: vec!["name".into()],
                repeated: true,
                doc: String::new(),
                schema: Schema::default(),
            }],
        }
    }

    fn block(label: &str, owner: &str) -> ModuleBlock {
        ModuleBlock {
            ty: "database".into(),
            labels: vec![label.into()],
            body: ModuleBody {
                attrs: BTreeMap::from([("owner".into(), Hcl::string(owner))]),
                blocks: vec![],
            },
        }
    }

    #[test]
    fn arguments_blocks_and_labels_are_the_sdk_contract() {
        let n = doze_hcl::Number::parse("1.5").map(Hcl::number);
        let body = ModuleBody {
            attrs: BTreeMap::from([
                ("password".into(), Hcl::string("pw")),
                ("ratio".into(), n.unwrap_or(Hcl::Bool(false))),
                (
                    "tags".into(),
                    Hcl::Tuple(vec![Hcl::string("a"), Hcl::Bool(true)]),
                ),
            ]),
            blocks: vec![block("app", "me"), block("shop", "you")],
        };
        let t = tree(&body, &schema());
        assert_eq!(
            t.get("password").map(|n| &n.value),
            Some(&Value::String("pw".into()))
        );
        assert_eq!(
            t.get("ratio").map(|n| &n.value),
            Some(&Value::Number("1.5".into()))
        );
        let Some(Value::List(dbs)) = t.get("database").map(|n| &n.value) else {
            panic!("a block type is a list of maps: {t:?}");
        };
        let names: Vec<_> = dbs
            .iter()
            .filter_map(|d| d.get("name"))
            .map(|n| &n.value)
            .collect();
        assert_eq!(
            names,
            [&Value::String("app".into()), &Value::String("shop".into())],
            "each block's label is an entry named as the schema names it, in order"
        );
        assert_eq!(
            dbs.get(1).and_then(|d| d.get("owner")).map(|n| &n.value),
            Some(&Value::String("you".into()))
        );
    }

    #[test]
    fn a_block_the_schema_does_not_name_keeps_its_labels_by_position() {
        let mut b = block("x", "y");
        b.ty = "other".into();
        let t = tree(
            &ModuleBody {
                attrs: BTreeMap::new(),
                blocks: vec![b],
            },
            &schema(),
        );
        let first = t.get("other").and_then(|n| match &n.value {
            Value::List(l) => l
                .first()
                .and_then(|m| m.get("label0"))
                .map(|n| n.value.clone()),
            _ => None,
        });
        assert_eq!(first, Some(Value::String("x".into())));
    }

    #[test]
    fn published_values_come_back_as_the_config_reads_them() {
        let v = Value::Map(BTreeMap::from([
            ("port".into(), Node::bare(Value::Number("5432".into()))),
            ("tls".into(), Node::bare(Value::Bool(false))),
            (
                "hosts".into(),
                Node::bare(Value::List(vec![Node::bare(Value::String("a".into()))])),
            ),
        ]));
        let back = back(&v);
        assert_eq!(
            value(&back),
            v,
            "a published tree survives the trip into the config and out"
        );
    }
}
