//! Running a template tree. Loops and output are bounded, so a template cannot run away.

use serde_json::{Map, Value};

use super::parse::Error;
use super::{Access, Env, Expr, Node, Op, Ref};

/// The most loop iterations one run may make.
const MAX_STEPS: usize = 100_000;
/// The most text one run may write.
const MAX_OUT: usize = 10 * 1024 * 1024;

type R<T> = Result<T, Error>;

fn fail<T>(why: impl Into<String>) -> R<T> {
    Err(Error(why.into()))
}

/// One run of a template.
pub(super) struct Run<'e> {
    pub(super) env: &'e mut Env,
    steps: usize,
}

impl<'e> Run<'e> {
    pub(super) fn new(env: &'e mut Env) -> Self {
        Self { env, steps: 0 }
    }

    pub(super) fn nodes(&mut self, nodes: &[Node], out: &mut String) -> R<()> {
        for n in nodes {
            self.node(n, out)?;
            if out.len() > MAX_OUT {
                return fail("the mapping template wrote more than 10 MB");
            }
        }
        Ok(())
    }

    fn node(&mut self, n: &Node, out: &mut String) -> R<()> {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::Ref(r, quiet) => match self.reference(r)? {
                Some(v) if !v.is_null() => write(&v, out),
                Some(_) => {}
                None if *quiet => {}
                None => out.push_str(&r.text),
            },
            Node::Set(target, e) => {
                let v = self.expr(e)?;
                self.set(target, v)?;
            }
            Node::If(arms, otherwise) => {
                for (cond, body) in arms {
                    if truthy(&self.expr(cond)?) {
                        return self.nodes(body, out);
                    }
                }
                if let Some(body) = otherwise {
                    self.nodes(body, out)?;
                }
            }
            Node::Foreach(var, over, body) => self.foreach(var, over, body, out)?,
        }
        Ok(())
    }

    fn foreach(&mut self, var: &str, over: &Expr, body: &[Node], out: &mut String) -> R<()> {
        let items = match self.expr(over)? {
            Value::Array(a) => a,
            Value::Object(o) => o.into_values().collect(),
            Value::Null => Vec::new(),
            other => vec![other],
        };
        let n = items.len();
        let saved = self.env.vars.remove("foreach");
        for (i, item) in items.into_iter().enumerate() {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return fail("the mapping template looped more than 100000 times");
            }
            self.env.vars.insert(var.to_owned(), item);
            let state = serde_json::json!({"index": i, "count": i + 1, "hasNext": i + 1 < n, "first": i == 0, "last": i + 1 == n});
            self.env.vars.insert("foreach".into(), state);
            self.env
                .vars
                .insert("velocityCount".into(), Value::from(i + 1));
            self.nodes(body, out)?;
        }
        if let Some(s) = saved {
            self.env.vars.insert("foreach".into(), s);
        }
        Ok(())
    }

    pub(super) fn expr(&mut self, e: &Expr) -> R<Value> {
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Str(nodes) => {
                let mut s = String::new();
                self.nodes(nodes, &mut s)?;
                Value::String(s)
            }
            Expr::Ref(r) => self.reference(r)?.unwrap_or(Value::Null),
            Expr::List(items) => {
                Value::Array(items.iter().map(|x| self.expr(x)).collect::<R<_>>()?)
            }
            Expr::Map(pairs) => {
                let mut m = Map::new();
                for (k, v) in pairs {
                    let k = text_of(&self.expr(k)?);
                    m.insert(k, self.expr(v)?);
                }
                Value::Object(m)
            }
            Expr::Range(a, b) => {
                let (a, b) = (num(&self.expr(a)?) as i64, num(&self.expr(b)?) as i64);
                if (b - a).abs() > MAX_STEPS as i64 {
                    return fail("a range is too long");
                }
                let r: Vec<Value> = if a <= b {
                    (a..=b).map(Value::from).collect()
                } else {
                    (b..=a).rev().map(Value::from).collect()
                };
                Value::Array(r)
            }
            Expr::Not(x) => Value::Bool(!truthy(&self.expr(x)?)),
            Expr::Neg(x) => number(-num(&self.expr(x)?)),
            Expr::Bin(op, a, b) => self.binary(*op, a, b)?,
        })
    }

    fn binary(&mut self, op: Op, a: &Expr, b: &Expr) -> R<Value> {
        let left = self.expr(a)?;
        match op {
            Op::Or if truthy(&left) => return Ok(Value::Bool(true)),
            Op::And if !truthy(&left) => return Ok(Value::Bool(false)),
            _ => {}
        }
        let right = self.expr(b)?;
        let both_numbers = left.is_number() && right.is_number();
        Ok(match op {
            Op::Or | Op::And => Value::Bool(truthy(&right)),
            Op::Eq => Value::Bool(same(&left, &right)),
            Op::Ne => Value::Bool(!same(&left, &right)),
            Op::Lt => Value::Bool(order(&left, &right) == Some(std::cmp::Ordering::Less)),
            Op::Le => {
                Value::Bool(order(&left, &right).is_some_and(|o| o != std::cmp::Ordering::Greater))
            }
            Op::Gt => Value::Bool(order(&left, &right) == Some(std::cmp::Ordering::Greater)),
            Op::Ge => {
                Value::Bool(order(&left, &right).is_some_and(|o| o != std::cmp::Ordering::Less))
            }
            Op::Add if !both_numbers => Value::String(text_of(&left) + &text_of(&right)),
            Op::Add => number(num(&left) + num(&right)),
            Op::Sub => number(num(&left) - num(&right)),
            Op::Mul => number(num(&left) * num(&right)),
            Op::Div if num(&right) == 0.0 => Value::Null,
            Op::Div if left.is_i64() && right.is_i64() => {
                number((num(&left) / num(&right)).trunc())
            }
            Op::Div => number(num(&left) / num(&right)),
            Op::Mod if num(&right) == 0.0 => Value::Null,
            Op::Mod => number(num(&left) % num(&right)),
        })
    }

    /// The value a reference names; `None` when its root is not defined.
    fn reference(&mut self, r: &Ref) -> R<Option<Value>> {
        let mut cur = match r.root.as_str() {
            "input" | "util" => {
                let Some((Access::Call(name, args), rest)) = r.path.split_first() else {
                    return Ok(None);
                };
                let args = args.iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?;
                let v = super::methods::builtin(&r.root, name, &args, &self.env.input)?;
                return self.walk(v, rest).map(Some);
            }
            root => match self.env.vars.get(root) {
                Some(v) => v.clone(),
                None => return Ok(None),
            },
        };
        // A mutating call on a variable (`$list.add(x)`, `$map.put(k, v)`) changes the variable.
        if let Some((Access::Call(name, args), parents)) = r.path.split_last()
            && matches!(
                name.as_str(),
                "add" | "put" | "remove" | "addAll" | "putAll"
            )
        {
            let args = args.iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?;
            let target = Ref {
                root: r.root.clone(),
                path: parents.to_vec(),
                text: String::new(),
            };
            let mut holder = self.reference(&target)?.unwrap_or(Value::Null);
            let answer = super::methods::mutate(&mut holder, name, &args)?;
            self.set(&target, holder)?;
            return Ok(Some(answer));
        }
        cur = self.walk(cur, &r.path)?;
        Ok(Some(cur))
    }

    fn walk(&mut self, mut cur: Value, path: &[Access]) -> R<Value> {
        for a in path {
            cur = match a {
                Access::Prop(p) => super::methods::property(&cur, p),
                Access::Index(e) => {
                    let i = self.expr(e)?;
                    match (&cur, &i) {
                        (Value::Array(a), _) => {
                            a.get(num(&i) as usize).cloned().unwrap_or(Value::Null)
                        }
                        (Value::Object(o), _) => {
                            o.get(&text_of(&i)).cloned().unwrap_or(Value::Null)
                        }
                        _ => Value::Null,
                    }
                }
                Access::Call(name, args) => {
                    let args = args.iter().map(|a| self.expr(a)).collect::<R<Vec<_>>>()?;
                    super::methods::call(&cur, name, &args)?
                }
            };
        }
        Ok(cur)
    }

    /// `#set`: a variable, or a member below one (made as maps where missing).
    fn set(&mut self, target: &Ref, v: Value) -> R<()> {
        if matches!(target.root.as_str(), "input" | "util") {
            return fail(format!("${} cannot be set", target.root));
        }
        let mut keys = Vec::with_capacity(target.path.len());
        for a in &target.path {
            keys.push(match a {
                Access::Prop(p) => p.clone(),
                Access::Index(e) => text_of(&self.expr(e)?),
                Access::Call(..) => return fail("a method call cannot be set"),
            });
        }
        let mut slot = self
            .env
            .vars
            .entry(target.root.clone())
            .or_insert(Value::Null);
        for key in keys {
            if !slot.is_object() {
                *slot = Value::Object(Map::new());
            }
            let Some(o) = slot.as_object_mut() else {
                return fail("a member cannot be set here");
            };
            slot = o.entry(key).or_insert(Value::Null);
        }
        *slot = v;
        Ok(())
    }
}

pub(super) fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
}

pub(super) fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// A number as JSON: whole numbers as integers.
pub(super) fn number(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9.0e15 {
        Value::from(f as i64)
    } else {
        serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
    }
}

fn same(a: &Value, b: &Value) -> bool {
    if a.is_number() && b.is_number() {
        return num(a) == num(b);
    }
    a == b || (!a.is_null() && !b.is_null() && text_of(a) == text_of(b))
}

fn order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if a.is_number() && b.is_number() {
        return num(a).partial_cmp(&num(b));
    }
    match (a, b) {
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

/// A value as text, as Velocity writes it.
pub(super) fn text_of(v: &Value) -> String {
    let mut s = String::new();
    write(v, &mut s);
    s
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => {}
        Value::String(s) => out.push_str(s),
        Value::Number(_) | Value::Bool(_) => out.push_str(&v.to_string()),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(k);
                out.push('=');
                write(x, out);
            }
            out.push('}');
        }
    }
}
