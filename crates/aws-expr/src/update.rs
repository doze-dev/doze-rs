//! Applying an update expression to an item.
//!
//! Every right-hand side is read from the item as it was before the update, then the clauses are applied in the order SET, REMOVE,
//! ADD, DELETE. The refusals' words are `(memory)` except the key one, which is recorded.

use crate::Invalid;
use crate::ast::{Operand, Path, SetValue, Update};
use crate::cond::operand as plain;
use crate::number::Number;
use crate::path::{get, get_mut, remove, set};
use crate::value::{Item, Value};

fn wrong_type() -> Invalid {
    Invalid("An operand in the update expression has an incorrect data type".to_owned())
}

fn invalid_path() -> Invalid {
    Invalid("The document path provided in the update expression is invalid for update".to_owned())
}

fn missing_attribute() -> Invalid {
    Invalid(
        "The provided expression refers to an attribute that does not exist in the item".to_owned(),
    )
}

fn number_error(e: crate::number::NumberError) -> Invalid {
    Invalid(e.message(""))
}

impl Update {
    /// Every top-level attribute the update names.
    pub fn roots(&self) -> impl Iterator<Item = &str> {
        let sets = self.set.iter().map(|(p, _)| p);
        let adds = self.add.iter().map(|(p, _)| p);
        let deletes = self.delete.iter().map(|(p, _)| p);
        sets.chain(&self.remove)
            .chain(adds)
            .chain(deletes)
            .map(Path::root)
    }

    /// Changes `item` as the expression says. `keys` are the key attributes, which no update may name.
    ///
    /// # Errors
    /// AWS's refusal: a key attribute, a value of the wrong type, a path that cannot be written.
    pub fn apply(&self, item: &mut Item, keys: &[&str]) -> Result<(), Invalid> {
        if let Some(key) = self.roots().find(|r| keys.contains(r)) {
            return Err(Invalid(format!(
                "One or more parameter values were invalid: Cannot update attribute {key}. This attribute is part of the key"
            )));
        }
        let computed: Vec<(&Path, Value)> = self
            .set
            .iter()
            .map(|(path, rhs)| right_side(rhs, item).map(|v| (path, v)))
            .collect::<Result<_, _>>()?;
        for (path, value) in computed {
            if !set(item, path, value) {
                return Err(invalid_path());
            }
        }
        for path in &self.remove {
            remove(item, path);
        }
        for (path, value) in &self.add {
            add(item, path, value)?;
        }
        for (path, value) in &self.delete {
            delete(item, path, value)?;
        }
        Ok(())
    }
}

fn right_side(rhs: &SetValue, item: &Item) -> Result<Value, Invalid> {
    match rhs {
        SetValue::Operand(a) => value_of(a, item),
        SetValue::Plus(a, b) => arithmetic(a, b, item, Number::add),
        SetValue::Minus(a, b) => arithmetic(a, b, item, Number::sub),
    }
}

type Op = fn(&Number, &Number) -> Result<Number, crate::number::NumberError>;

fn arithmetic(a: &Operand, b: &Operand, item: &Item, op: Op) -> Result<Value, Invalid> {
    match (value_of(a, item)?, value_of(b, item)?) {
        (Value::N(a), Value::N(b)) => op(&a, &b).map(Value::N).map_err(number_error),
        _ => Err(wrong_type()),
    }
}

/// The value of an operand in an update, which may be `if_not_exists` or `list_append`.
fn value_of(op: &Operand, item: &Item) -> Result<Value, Invalid> {
    match op {
        Operand::IfNotExists(path, fallback) => match get(item, path) {
            Some(v) => Ok(v.clone()),
            None => value_of(fallback, item),
        },
        Operand::ListAppend(a, b) => match (value_of(a, item)?, value_of(b, item)?) {
            (Value::L(mut a), Value::L(b)) => {
                a.extend(b);
                Ok(Value::L(a))
            }
            _ => Err(wrong_type()),
        },
        other => plain(other, item)
            .map(std::borrow::Cow::into_owned)
            .ok_or_else(missing_attribute),
    }
}

fn add(item: &mut Item, path: &Path, value: &Value) -> Result<(), Invalid> {
    let sum = match (get(item, path), value) {
        (None, Value::N(_)) => value.clone(),
        (None, v) if v.is_set() => value.clone(),
        (Some(Value::N(a)), Value::N(b)) => Value::N(a.add(b).map_err(number_error)?),
        (Some(Value::Ss(a)), Value::Ss(b)) => Value::Ss(a.union(b).cloned().collect()),
        (Some(Value::Ns(a)), Value::Ns(b)) => Value::Ns(a.union(b).cloned().collect()),
        (Some(Value::Bs(a)), Value::Bs(b)) => Value::Bs(a.union(b).cloned().collect()),
        (Some(have), want) if have.is_set() && want.is_set() => {
            return Err(Invalid("Type mismatch for attribute to update".to_owned()));
        }
        _ => return Err(wrong_type()),
    };
    if set(item, path, sum) {
        Ok(())
    } else {
        Err(invalid_path())
    }
}

fn delete(item: &mut Item, path: &Path, value: &Value) -> Result<(), Invalid> {
    if !value.is_set() {
        return Err(wrong_type());
    }
    let Some(have) = get_mut(item, path) else {
        return Ok(());
    };
    let emptied = match (&mut *have, value) {
        (Value::Ss(a), Value::Ss(b)) => {
            a.retain(|x| !b.contains(x));
            a.is_empty()
        }
        (Value::Ns(a), Value::Ns(b)) => {
            a.retain(|x| !b.contains(x));
            a.is_empty()
        }
        (Value::Bs(a), Value::Bs(b)) => {
            a.retain(|x| !b.contains(x));
            a.is_empty()
        }
        (have, _) if have.is_set() => {
            return Err(Invalid("Type mismatch for attribute to update".to_owned()));
        }
        _ => return Err(wrong_type()),
    };
    if emptied {
        remove(item, path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ast::{NameMap, ValueMap};
    use crate::parse_update::parse_update;
    use crate::scope::Scope;
    use crate::value::{item_from_json, item_to_json};

    fn run(
        text: &str,
        values: serde_json::Value,
        item: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let values: ValueMap = values
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), Value::from_json(v).unwrap()))
            .collect();
        let names = NameMap::new();
        let update = parse_update(text, &mut Scope::new(&names, &values)).map_err(|e| e.0)?;
        let mut item = item_from_json(&item).unwrap();
        update.apply(&mut item, &["pk"]).map_err(|e| e.0)?;
        Ok(item_to_json(&item))
    }

    fn base() -> serde_json::Value {
        json!({"pk": {"S": "a"}, "n": {"N": "2"}, "gone": {"S": "x"}, "t": {"SS": ["a", "b"]}, "l": {"L": [{"N": "1"}]}, "m": {"M": {}}})
    }

    #[test]
    fn the_recorded_update() {
        let v = json!({":one": {"N": "1"}, ":t": {"L": []}, ":s": {"SS": ["x"]}});
        let out = run(
            "SET n = n + :one, tags = :t ADD seen :s REMOVE gone",
            v,
            base(),
        )
        .unwrap();
        assert_eq!(out["n"], json!({"N": "3"}));
        assert_eq!(out["tags"], json!({"L": []}));
        assert_eq!(out["seen"], json!({"SS": ["x"]}));
        assert!(out.get("gone").is_none());
    }

    #[test]
    fn functions_and_nested_paths() {
        let v = json!({":zero": {"N": "0"}, ":more": {"L": [{"N": "2"}]}, ":d": {"S": "d"}});
        let out = run("SET l = list_append(l, :more), z = if_not_exists(z, :zero), n = if_not_exists(n, :zero), m.k = :d", v, base()).unwrap();
        assert_eq!(out["l"], json!({"L": [{"N": "1"}, {"N": "2"}]}));
        assert_eq!(out["z"], json!({"N": "0"}));
        assert_eq!(out["n"], json!({"N": "2"}));
        assert_eq!(out["m"], json!({"M": {"k": {"S": "d"}}}));
    }

    #[test]
    fn set_arithmetic_and_delete() {
        let v = json!({":b": {"SS": ["b"]}, ":c": {"SS": ["c"]}, ":ns": {"NS": ["1"]}});
        let out = run("ADD t :c", v.clone(), base()).unwrap();
        assert_eq!(out["t"], json!({"SS": ["a", "b", "c"]}));
        let out = run("DELETE t :b", v.clone(), base()).unwrap();
        assert_eq!(out["t"], json!({"SS": ["a"]}));
        assert_eq!(
            run("ADD t :ns", v.clone(), base()).unwrap_err(),
            "Type mismatch for attribute to update"
        );
        let empty = run("DELETE t :ab", json!({":ab": {"SS": ["a", "b"]}}), base()).unwrap();
        assert!(empty.get("t").is_none());
    }

    #[test]
    fn refusals() {
        let v = json!({":v": {"S": "b"}, ":one": {"N": "1"}});
        assert_eq!(
            run("SET pk = :v", v.clone(), base()).unwrap_err(),
            "One or more parameter values were invalid: Cannot update attribute pk. This attribute is part of the key"
        );
        assert_eq!(
            run("SET n = gone + :one", v.clone(), base()).unwrap_err(),
            "An operand in the update expression has an incorrect data type"
        );
        assert!(
            run("SET n = nothere + :one", v.clone(), base())
                .unwrap_err()
                .contains("does not exist in the item")
        );
        assert!(
            run("SET nothere.x = :v", v, base())
                .unwrap_err()
                .contains("invalid for update")
        );
    }
}
