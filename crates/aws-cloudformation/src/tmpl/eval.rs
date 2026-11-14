//! The intrinsic functions, evaluated against what is known: parameters, pseudo parameters, conditions, mappings, exports and the
//! resources made so far. A reference to something not made yet is [`Fault::Pending`] (a change set calls such a value dynamic);
//! anything else that cannot be resolved is an error, never an empty string.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// Why a value could not be worked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    /// It depends on a resource that has no value yet.
    Pending(String),
    /// It cannot be resolved: the message says why, in AWS's words where known.
    Error(String),
}

pub(crate) type Out = Result<Option<Value>, Fault>;

/// What a lookup of a resource found.
#[derive(Clone, Debug)]
pub(crate) enum Lookup<T> {
    Found(T),
    /// Declared, but not made (yet).
    Pending,
    /// Not declared, or its condition is false.
    Absent,
    /// Made, but it has no such attribute.
    Missing,
}

/// The resources as the evaluator sees them.
pub(crate) trait Known {
    /// The resource's `Ref` (its physical id).
    fn reference(&self, logical: &str) -> Lookup<Value>;
    /// One of its attributes.
    fn attribute(&self, logical: &str, attr: &str) -> Lookup<Value>;
}

/// No resource is known.
#[derive(Debug)]
pub(crate) struct Nothing;

impl Known for Nothing {
    fn reference(&self, _: &str) -> Lookup<Value> {
        Lookup::Absent
    }
    fn attribute(&self, _: &str, _: &str) -> Lookup<Value> {
        Lookup::Absent
    }
}

/// The pseudo parameters of a stack.
#[derive(Clone, Debug, Default)]
pub(crate) struct Pseudo {
    pub(crate) account: String,
    pub(crate) region: String,
    pub(crate) stack_id: String,
    pub(crate) stack_name: String,
    pub(crate) notification_arns: Vec<String>,
}

impl Pseudo {
    pub(crate) fn partition(&self) -> &'static str {
        partition(&self.region)
    }
}

/// The partition a region belongs to.
pub(crate) fn partition(region: &str) -> &'static str {
    if region.starts_with("cn-") {
        "aws-cn"
    } else if region.starts_with("us-gov-") {
        "aws-us-gov"
    } else {
        "aws"
    }
}

/// Everything an expression may refer to.
pub(crate) struct Scope<'a> {
    pub(crate) params: &'a BTreeMap<String, Value>,
    pub(crate) pseudo: &'a Pseudo,
    pub(crate) conditions: &'a BTreeMap<String, bool>,
    pub(crate) mappings: Option<&'a Value>,
    pub(crate) known: &'a dyn Known,
    pub(crate) exports: &'a BTreeMap<String, String>,
}

pub(crate) fn err<T>(message: impl Into<String>) -> Result<T, Fault> {
    Err(Fault::Error(message.into()))
}

/// The name and argument of an intrinsic, when `v` is one: an object of one key, `Ref`, `Condition` (naming a condition) or
/// `Fn::*`.
pub(crate) fn intrinsic(v: &Value) -> Option<(&str, &Value)> {
    let o = v.as_object()?;
    if o.len() != 1 {
        return None;
    }
    let (k, arg) = o.iter().next()?;
    match k.as_str() {
        "Ref" | "Condition" if arg.is_string() => Some((k, arg)),
        k if k.starts_with("Fn::") => Some((k, arg)),
        _ => None,
    }
}

impl Scope<'_> {
    /// Evaluates `v`. `None` is `AWS::NoValue`: a property that is to be left out.
    pub(crate) fn eval(&self, v: &Value) -> Out {
        if let Some((name, arg)) = intrinsic(v) {
            return self.call(name, arg);
        }
        Ok(Some(match v {
            Value::Object(o) => {
                let mut out = Map::new();
                for (k, x) in o {
                    if let Some(y) = self.eval(x)? {
                        out.insert(k.clone(), y);
                    }
                }
                Value::Object(out)
            }
            Value::Array(a) => {
                let mut out = Vec::with_capacity(a.len());
                for x in a {
                    if let Some(y) = self.eval(x)? {
                        out.push(y);
                    }
                }
                Value::Array(out)
            }
            other => other.clone(),
        }))
    }

    /// Evaluates `v`, which must give a value (not `AWS::NoValue`).
    pub(crate) fn value(&self, v: &Value, what: &str) -> Result<Value, Fault> {
        match self.eval(v)? {
            Some(x) => Ok(x),
            None => err(format!("Template error: {what} cannot be AWS::NoValue")), // (memory)
        }
    }

    /// Evaluates `v` to text (a number or boolean is written out).
    pub(crate) fn text(&self, v: &Value, what: &str) -> Result<String, Fault> {
        match self.value(v, what)? {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            _ => err(format!("Template error: {what} must be a string")), // (memory)
        }
    }

    /// Evaluates `v` to a list.
    pub(crate) fn list(&self, v: &Value, what: &str) -> Result<Vec<Value>, Fault> {
        match self.value(v, what)? {
            Value::Array(a) => Ok(a),
            _ => err(format!("Template error: {what} must be a list")), // (memory)
        }
    }

    fn call(&self, name: &str, arg: &Value) -> Out {
        match name {
            "Ref" => self.reference(arg.as_str().unwrap_or_default()),
            "Condition" => {
                let c = arg.as_str().unwrap_or_default();
                Ok(Some(Value::Bool(self.condition(c)?)))
            }
            "Fn::GetAtt" => self.get_att(arg).map(Some),
            "Fn::If" => self.fn_if(arg),
            "Fn::Equals" | "Fn::And" | "Fn::Or" | "Fn::Not" => {
                Ok(Some(Value::Bool(self.boolean(name, arg)?)))
            }
            "Fn::Sub" => self.sub(arg).map(|s| Some(Value::String(s))),
            other => self.function(other, arg),
        }
    }

    /// A named condition's value.
    pub(crate) fn condition(&self, name: &str) -> Result<bool, Fault> {
        match self.conditions.get(name) {
            Some(b) => Ok(*b),
            // (memory)
            None => err(format!(
                "Template format error: Unresolved condition dependencies [{name}] in the Resources block of the template"
            )),
        }
    }

    fn reference(&self, name: &str) -> Out {
        if let Some(v) = self.params.get(name) {
            return Ok(Some(v.clone()));
        }
        let p = self.pseudo;
        let text = |s: &str| Ok(Some(Value::String(s.to_owned())));
        match name {
            "AWS::NoValue" => Ok(None),
            "AWS::AccountId" => text(&p.account),
            "AWS::Region" => text(&p.region),
            "AWS::StackId" => text(&p.stack_id),
            "AWS::StackName" => text(&p.stack_name),
            "AWS::Partition" => text(p.partition()),
            "AWS::URLSuffix" => text(if p.region.starts_with("cn-") {
                "amazonaws.com.cn"
            } else {
                "amazonaws.com"
            }),
            "AWS::NotificationARNs" => Ok(Some(Value::Array(
                p.notification_arns
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ))),
            _ => match self.known.reference(name) {
                Lookup::Found(v) => Ok(Some(v)),
                Lookup::Pending => Err(Fault::Pending(name.to_owned())),
                Lookup::Absent | Lookup::Missing => err(format!(
                    "Template format error: Unresolved resource dependencies [{name}] in the Resources block of the template"
                )),
            },
        }
    }

    fn get_att(&self, arg: &Value) -> Result<Value, Fault> {
        let (res, attr) = match arg {
            Value::String(s) => match s.split_once('.') {
                Some((a, b)) => (a.to_owned(), b.to_owned()),
                None => {
                    return err(
                        "Template error: every Fn::GetAtt object requires two non-empty parameters",
                    );
                }
            },
            Value::Array(a) if a.len() == 2 => (
                a.first()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                self.text(a.get(1).unwrap_or(&Value::Null), "Fn::GetAtt")?,
            ),
            // (memory)
            _ => {
                return err(
                    "Template error: every Fn::GetAtt object requires two non-empty parameters",
                );
            }
        };
        self.attribute(&res, &attr)
    }

    /// `Fn::GetAtt` of a resource.
    pub(crate) fn attribute(&self, res: &str, attr: &str) -> Result<Value, Fault> {
        match self.known.attribute(res, attr) {
            Lookup::Found(v) => Ok(v),
            Lookup::Pending => Err(Fault::Pending(res.to_owned())),
            Lookup::Absent => err(format!(
                "Template error: instance of Fn::GetAtt references undefined resource {res}"
            )),
            // (memory)
            Lookup::Missing => err(format!(
                "Template error: resource {res} does not support attribute type {attr} in Fn::GetAtt"
            )),
        }
    }

    fn fn_if(&self, arg: &Value) -> Out {
        let Some([c, yes, no]) = arg.as_array().map(Vec::as_slice) else {
            // (memory)
            return err("Template error: Fn::If requires a list argument with three elements");
        };
        let name = c.as_str().unwrap_or_default();
        if self.condition(name)? {
            self.eval(yes)
        } else {
            self.eval(no)
        }
    }

    /// `Fn::Equals`, `Fn::And`, `Fn::Or`, `Fn::Not`, or a `Condition` reference, to a boolean.
    pub(crate) fn boolean(&self, name: &str, arg: &Value) -> Result<bool, Fault> {
        let operands = || arg.as_array().cloned().unwrap_or_default();
        let truth = |v: &Value| -> Result<bool, Fault> {
            if let Some((n, a)) = intrinsic(v) {
                return match n {
                    "Condition" => self.condition(a.as_str().unwrap_or_default()),
                    "Fn::Equals" | "Fn::And" | "Fn::Or" | "Fn::Not" => self.boolean(n, a),
                    _ => match self.value(v, n)? {
                        Value::Bool(b) => Ok(b),
                        Value::String(s) => Ok(s == "true"),
                        _ => err(format!("Template error: {n} does not give a boolean")),
                    },
                };
            }
            match v {
                Value::Bool(b) => Ok(*b),
                Value::String(s) if s == "true" || s == "false" => Ok(s == "true"),
                // (memory)
                _ => err("Template error: every condition must be a boolean"),
            }
        };
        match name {
            "Fn::Equals" => {
                let ops = operands();
                let [a, b] = ops.as_slice() else {
                    return err(
                        "Template error: Fn::Equals requires a list argument with two elements",
                    ); // (memory)
                };
                let (a, b) = (self.value(a, name)?, self.value(b, name)?);
                Ok(loose_text(&a) == loose_text(&b))
            }
            "Fn::Not" => match operands().as_slice() {
                [x] => Ok(!truth(x)?),
                _ => err("Template error: Fn::Not requires a list argument with one element"), // (memory)
            },
            "Fn::And" | "Fn::Or" => {
                let ops = operands();
                if !(2..=10).contains(&ops.len()) {
                    // (memory)
                    return err(format!(
                        "Template error: {name} requires a list argument with between 2 and 10 elements"
                    ));
                }
                let mut acc = name == "Fn::And";
                for x in &ops {
                    let t = truth(x)?;
                    acc = if name == "Fn::And" {
                        acc && t
                    } else {
                        acc || t
                    };
                }
                Ok(acc)
            }
            _ => truth(&Value::Null),
        }
    }
}

/// A value compared as `Fn::Equals` compares: scalars as their text, lists and objects as written.
fn loose_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}
