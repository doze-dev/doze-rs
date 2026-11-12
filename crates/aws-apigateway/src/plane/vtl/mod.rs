//! Mapping templates: the Velocity subset API Gateway runs on a request before an integration (`requestTemplates`) and on an answer
//! after it (`responseTemplates`, gateway responses). [`parse`] reads a template into a tree; [`render`] runs it against an [`Env`]:
//! `$input` (the body, `json()`, `path()`, `params()`), `$util` (`escapeJavaScript`, `parseJson`, `urlEncode`, `urlDecode`,
//! `base64Encode`, `base64Decode`), `$context` (writable, for `requestOverride` and `responseOverride`) and `$stageVariables`.
//!
//! Values are JSON values; a map or list written into the text prints as Java prints it (`{a=1, b=2}`), as Velocity does. What the
//! subset does not have is refused with a message naming it, never guessed.

mod eval;
mod expr;
mod methods;
mod parse;

use std::collections::HashMap;

use serde_json::Value;

pub(crate) use parse::Error;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Text(String),
    /// A reference; `true` when quiet (`$!x`: nothing is written for null).
    Ref(Ref, bool),
    Set(Ref, Expr),
    If(Vec<(Expr, Vec<Node>)>, Option<Vec<Node>>),
    Foreach(String, Expr, Vec<Node>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ref {
    pub(crate) root: String,
    pub(crate) path: Vec<Access>,
    /// The reference as written, for writing it back when it names nothing.
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Access {
    Prop(String),
    Index(Expr),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    Lit(Value),
    /// A double-quoted string: interpolated.
    Str(Vec<Node>),
    Ref(Ref),
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Range(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

/// The request a template reads through `$input`.
#[derive(Debug, Default)]
pub(crate) struct Input {
    pub(crate) body: String,
    /// `{path: {...}, querystring: {...}, header: {...}}`.
    pub(crate) params: Value,
}

/// What a template runs against: its variables (`context`, `stageVariables` and whatever `#set` makes) and `$input`.
#[derive(Debug, Default)]
pub(crate) struct Env {
    pub(crate) vars: HashMap<String, Value>,
    pub(crate) input: Input,
}

/// Runs a template.
///
/// # Errors
/// A template that cannot be read, or one whose run goes wrong (a method that is not built, too much output, too many loops).
pub(crate) fn render(text: &str, env: &mut Env) -> Result<String, Error> {
    let nodes = parse::parse(text)?;
    let mut out = String::new();
    eval::Run::new(env).nodes(&nodes, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn env(body: &str) -> Env {
        let mut e = Env {
            input: Input {
                body: body.into(),
                params: json!({"path": {"id": "7"}, "querystring": {"q": "x"}, "header": {"h": "y"}}),
            },
            ..Env::default()
        };
        e.vars
            .insert("context".into(), json!({"requestId": "r1", "stage": "dev"}));
        e.vars
            .insert("stageVariables".into(), json!({"color": "blue"}));
        e
    }

    #[test]
    fn the_common_templates_render_as_api_gateway_renders_them() {
        let mut e = env(r#"{"name":"pat","items":[1,2,3],"nested":{"a":"b"}}"#);
        let t = r#"{"name": "$input.path('$.name')", "n": $input.path('$.items').size(), "raw": $input.json('$.nested'), "id": "$input.params('id')", "q": "$input.params().querystring.q", "ctx": "$context.requestId", "color": "$stageVariables.color"}"#;
        let out = render(t, &mut e).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v,
            json!({"name": "pat", "n": 3, "raw": {"a": "b"}, "id": "7", "q": "x", "ctx": "r1", "color": "blue"})
        );
    }

    #[test]
    fn loops_conditions_sets_and_utilities() {
        let mut e = env(r#"{"items":[{"v":1},{"v":2}]}"#);
        let t = "#set($sum = 0)#foreach($i in $input.path('$.items'))#set($sum = $sum + $i.v)$i.v#if($foreach.hasNext),#end#end=$sum|$util.escapeJavaScript('a\"b')|$util.base64Encode('hi')|#set($context.responseOverride.status = 418)";
        assert_eq!(render(t, &mut e).unwrap(), "1,2=3|a\\\"b|aGk=|");
        assert_eq!(e.vars["context"]["responseOverride"]["status"], 418);
    }

    #[test]
    fn what_names_nothing_is_written_back_or_left_out() {
        let mut e = env("{}");
        assert_eq!(
            render("[$nope][$!nope][$input.path('$.missing')]", &mut e).unwrap(),
            "[$nope][][]"
        );
        assert!(
            render("$input.nonesuch()", &mut e)
                .unwrap_err()
                .0
                .contains("nonesuch")
        );
    }
}
