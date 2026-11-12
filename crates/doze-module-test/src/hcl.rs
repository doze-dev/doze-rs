//! Literal HCL to the `Value` tree the core sends a module (PLAN.md D1), for tests and published examples. Literals
//! only (strings, numbers, bools, null, lists, objects, nested blocks): evaluation (variables, functions, templates)
//! is the core's job. Spans are kept, so diagnostics point at the right place in the test's source.
//!
//! The block encoding is the contract of sdk.md: an argument is a map entry; a nested block type is a map entry holding
//! a list of maps, one per block; a block's labels are entries of its map, named after the schema's label names.

use doze_module_proto::msg::hello::Schema;
use doze_module_proto::msg::value::{Node, Span, Value};
use hcl_edit::Span as _;
use hcl_edit::expr::{Expression, ObjectKey, UnaryOperator};
use hcl_edit::structure::{Block, BlockLabel, Body, Structure};
use std::collections::BTreeMap;

/// Arguments the core reads itself; a module never sees them (protocol 1's `engine.CoreArguments`).
pub const CORE_ARGUMENTS: &[&str] = &[
    "count",
    "for_each",
    "depends_on",
    "enabled",
    "after",
    "version",
    "listen",
    "port",
    "sleep",
];

/// One engine block, as the core would hand it to the module.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub engine: String,
    pub name: String,
    /// The declared `version`, if any.
    pub version: String,
    /// The block without the core's arguments.
    pub spec: Node,
}

/// Why a test's HCL cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HclError(pub String);

impl std::fmt::Display for HclError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn span_of(s: Option<std::ops::Range<usize>>) -> Span {
    let r = s.unwrap_or_default();
    Span {
        file: 0,
        start: u32::try_from(r.start).unwrap_or(u32::MAX),
        end: u32::try_from(r.end).unwrap_or(u32::MAX),
    }
}

fn label(l: &BlockLabel) -> String {
    match l {
        BlockLabel::Ident(i) => i.as_str().to_string(),
        BlockLabel::String(s) => s.value().clone(),
    }
}

/// Parses `src`, which must hold exactly one block `<engine> "<name>" { … }`, against the module's schema.
pub fn parse_block(src: &str, schema: &Schema) -> Result<Parsed, HclError> {
    let body =
        hcl_edit::parser::parse_body(src).map_err(|e| HclError(format!("not valid HCL: {e}")))?;
    let blocks: Vec<&Block> = body.iter().filter_map(|s| s.as_block()).collect();
    let [block] = blocks.as_slice() else {
        return Err(HclError(format!(
            "expected one block, found {}",
            blocks.len()
        )));
    };
    let [name] = block.labels.as_slice() else {
        return Err(HclError("an engine block has one label, its name".into()));
    };
    let spec = body_node(&block.body, schema, &[], span_of(block.span()))?;
    let version = match spec.get("version").map(|n| &n.value) {
        Some(Value::String(v) | Value::Number(v)) => v.clone(),
        _ => String::new(),
    };
    let Value::Map(mut entries) = spec.value else {
        return Err(HclError("a block body".into()));
    };
    entries.retain(|k, _| !CORE_ARGUMENTS.contains(&k.as_str()));
    Ok(Parsed {
        engine: block.ident.as_str().to_string(),
        name: label(name),
        version,
        spec: Node {
            span: spec.span,
            value: Value::Map(entries),
        },
    })
}

fn body_node(
    body: &Body,
    schema: &Schema,
    labels: &[(String, String)],
    span: Span,
) -> Result<Node, HclError> {
    let mut map: BTreeMap<String, Node> = labels
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                Node {
                    span,
                    value: Value::String(v.clone()),
                },
            )
        })
        .collect();
    let mut nested: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for s in body.iter() {
        match s {
            Structure::Attribute(a) => {
                map.insert(a.key.as_str().to_string(), expr(&a.value)?);
            }
            Structure::Block(b) => {
                let kind = b.ident.as_str();
                let Some(bs) = schema.blocks.iter().find(|bs| bs.name == kind) else {
                    return Err(HclError(format!("unknown block type `{kind}`")));
                };
                if b.labels.len() != bs.labels.len() {
                    return Err(HclError(format!(
                        "`{kind}` takes {} label(s)",
                        bs.labels.len()
                    )));
                }
                let named: Vec<(String, String)> = bs
                    .labels
                    .iter()
                    .cloned()
                    .zip(b.labels.iter().map(label))
                    .collect();
                nested.entry(kind.to_string()).or_default().push(body_node(
                    &b.body,
                    &bs.schema,
                    &named,
                    span_of(b.span()),
                )?);
            }
        }
    }
    for (k, items) in nested {
        map.insert(
            k,
            Node {
                span,
                value: Value::List(items),
            },
        );
    }
    Ok(Node {
        span,
        value: Value::Map(map),
    })
}

fn expr(e: &Expression) -> Result<Node, HclError> {
    let span = span_of(e.span());
    let value = match e {
        Expression::Null(_) => Value::Null,
        Expression::Bool(b) => Value::Bool(*b.value()),
        Expression::Number(n) => Value::Number(n.value().to_string()),
        Expression::String(s) => Value::String(s.value().clone()),
        Expression::Array(a) => Value::List(a.iter().map(expr).collect::<Result<_, _>>()?),
        Expression::Object(o) => {
            let mut m = BTreeMap::new();
            for (k, v) in o.iter() {
                let key = match k {
                    ObjectKey::Ident(i) => i.as_str().to_string(),
                    ObjectKey::Expression(Expression::String(s)) => s.value().clone(),
                    _ => return Err(HclError("object keys must be names or strings".into())),
                };
                m.insert(key, expr(v.expr())?);
            }
            Value::Map(m)
        }
        Expression::UnaryOp(u) if *u.operator.value() == UnaryOperator::Neg => match &u.expr {
            Expression::Number(n) => Value::Number(format!("-{}", n.value())),
            _ => return Err(HclError("only a number can be negated in a literal".into())),
        },
        _ => {
            return Err(HclError(
                "only literals here: the core evaluates expressions, the harness does not".into(),
            ));
        }
    };
    Ok(Node { span, value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use doze_module_proto::msg::hello::{BlockSchema, Schema};

    fn schema() -> Schema {
        let channel = BlockSchema {
            name: "channel".into(),
            labels: vec!["name".into()],
            repeated: true,
            doc: String::new(),
            schema: Schema::default(),
        };
        Schema {
            attrs: vec![],
            blocks: vec![channel],
        }
    }

    #[test]
    fn literals_labels_spans_and_core_arguments() {
        let src = "echo \"e\" {\n  version = \"2\"\n  port = 7001\n  buffer = 12\n  ratio = -1.5\n  on = true\n  tags = [\"a\", \"b\"]\n  settings = { k = \"v\" }\n  channel \"a\" { size = 4 }\n  channel \"b\" { size = 1 }\n}\n";
        let p = parse_block(src, &schema()).unwrap();
        assert_eq!(
            (p.engine.as_str(), p.name.as_str(), p.version.as_str()),
            ("echo", "e", "2")
        );
        assert!(
            p.spec.get("version").is_none() && p.spec.get("port").is_none(),
            "core arguments must be stripped"
        );
        assert_eq!(
            p.spec.get("buffer").map(|n| &n.value),
            Some(&Value::Number("12".into()))
        );
        assert_eq!(
            p.spec.get("ratio").map(|n| &n.value),
            Some(&Value::Number("-1.5".into()))
        );
        let b = p.spec.get("buffer").unwrap().span;
        assert_eq!(&src[b.start as usize..b.end as usize], "12");
        let Some(Value::List(chans)) = p.spec.get("channel").map(|n| &n.value) else {
            panic!("channels")
        };
        assert_eq!(
            chans[1].get("name").map(|n| &n.value),
            Some(&Value::String("b".into()))
        );
    }

    #[test]
    fn expressions_and_unknown_blocks_are_refused() {
        assert!(parse_block("echo \"e\" { x = var.y }", &schema()).is_err());
        assert!(parse_block("echo \"e\" { other \"z\" {} }", &schema()).is_err());
        assert!(parse_block("echo \"e\" {}\necho \"f\" {}", &schema()).is_err());
    }
}
