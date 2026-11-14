//! A request read as JSON (the gateway turned the Query form into JSON before it was checked against the model).

use aws_wire::service::Call;
use serde_json::{Map, Value};

use crate::tmpl::params::Given;

/// A decoded request.
#[derive(Debug)]
pub(crate) struct Req<'a> {
    pub(crate) call: &'a Call,
    pub(crate) v: Value,
}

impl<'a> Req<'a> {
    pub(crate) fn new(call: &'a Call) -> Self {
        let v = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        Self {
            call,
            v: if v.is_object() {
                v
            } else {
                Value::Object(Map::new())
            },
        }
    }

    pub(crate) fn opt(&self, name: &str) -> Option<&str> {
        self.v.get(name).and_then(Value::as_str)
    }

    pub(crate) fn s(&self, name: &str) -> &str {
        self.opt(name).unwrap_or_default()
    }

    pub(crate) fn flag(&self, name: &str) -> Option<bool> {
        self.v.get(name).and_then(Value::as_bool)
    }

    pub(crate) fn int(&self, name: &str) -> Option<i64> {
        self.v.get(name).and_then(Value::as_i64)
    }

    pub(crate) fn strings(&self, name: &str) -> Vec<String> {
        self.v
            .get(name)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `Tags` as pairs, in the order given.
    pub(crate) fn tags(&self) -> Option<Vec<(String, String)>> {
        let a = self.v.get("Tags")?.as_array()?;
        Some(
            a.iter()
                .map(|t| {
                    let get = |k: &str| {
                        t.get(k)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (get("Key"), get("Value"))
                })
                .collect(),
        )
    }

    /// `Parameters` as given.
    pub(crate) fn params(&self) -> Vec<Given> {
        self.v
            .get("Parameters")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|p| Given {
                        key: p
                            .get("ParameterKey")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        value: p
                            .get("ParameterValue")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        use_previous: p.get("UsePreviousValue").and_then(Value::as_bool)
                            == Some(true),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn account(&self) -> &str {
        &self.call.caller.account_id
    }

    pub(crate) fn region(&self) -> &str {
        &self.call.caller.region
    }
}
