//! A list's parameters: filters and selectors the page offers and the server applies, `?p.<key>=<value>`.
//!
//! A kind names each [`Param`]: what it is called, how the page draws it, its choices (fixed, or listed by a call: the event
//! buses of a rule list), and how its value changes the list call (an EventBridge bus is the call's `EventBusName`; a
//! CloudFormation status is a group of statuses). A key the kind does not have is ignored. Nothing is kept: the value is in the
//! address.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value};

use super::bus::Bus;
use super::run::{self, Fill};
use super::{Descriptor, Kind, Step, Text};
pub use crate::model::ParamKind;
use crate::model::{ParamOption, ParamSpec};

/// What a parameter's choices are.
#[derive(Debug, Clone, Copy)]
pub enum Options {
    /// Free text.
    None,
    /// `(value, label)`.
    Fixed(&'static [(&'static str, &'static str)]),
    /// Listed by a call each time the list is drawn (every bus there is).
    Listed {
        step: Step,
        items: &'static str,
        item_as: &'static str,
        value: Text,
    },
}

/// How a value changes the list call.
#[derive(Debug, Clone, Copy)]
pub enum Apply {
    /// It is this member of the call's input.
    Member(&'static str),
    /// A JSON object merged over the input, `"{value}"` standing for the value (`{"AlarmTypes":["{value}"]}`).
    Template(&'static str),
    /// Worked out from the value.
    With(fn(&str) -> Value),
    /// No member of the call: the rows of the page it answered are narrowed to those this holds of (`row`, `value`). For a list
    /// whose call has nothing to filter by (CloudFormation's `DescribeStacks`).
    Filter(fn(&Value, &str) -> bool),
}

/// One filter or selector.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    /// In the address: `p.<key>`.
    pub key: &'static str,
    pub label: &'static str,
    pub input: ParamKind,
    pub options: Options,
    /// What it holds when the address gives none; empty for no value.
    pub default: &'static str,
    pub apply: Apply,
}

impl Param {
    /// A parameter that is one member of the list call.
    #[must_use]
    pub const fn member(
        key: &'static str,
        label: &'static str,
        input: ParamKind,
        options: Options,
        member: &'static str,
    ) -> Self {
        Self {
            key,
            label,
            input,
            options,
            default: "",
            apply: Apply::Member(member),
        }
    }

    /// A filter of the rows the page never draws: a picker asks for it in the address (`?p.trusts=sns.amazonaws.com`).
    #[must_use]
    pub const fn hidden(
        key: &'static str,
        label: &'static str,
        keep: fn(&Value, &str) -> bool,
    ) -> Self {
        Self {
            key,
            label,
            input: ParamKind::Hidden,
            options: Options::None,
            default: "",
            apply: Apply::Filter(keep),
        }
    }

    /// The same, holding `default` until the address says otherwise.
    #[must_use]
    pub const fn or(mut self, default: &'static str) -> Self {
        self.default = default;
        self
    }

    /// What it holds: the address's value, else its default; `None` for none.
    fn value<'a>(&'a self, given: &'a BTreeMap<String, String>) -> Option<&'a str> {
        given
            .get(self.key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .or_else(|| (!self.default.is_empty()).then_some(self.default))
    }

    /// The members this parameter sets over the list call, for `value`.
    fn members(&self, value: &str) -> Map<String, Value> {
        match &self.apply {
            Apply::Member(m) => {
                Map::from_iter([((*m).to_owned(), Value::String(value.to_owned()))])
            }
            Apply::Template(t) => {
                let escaped = serde_json::to_string(value).unwrap_or_default();
                let inner = escaped.trim_matches('"');
                match serde_json::from_str(&t.replace("{value}", inner)) {
                    Ok(Value::Object(m)) => m,
                    _ => Map::new(),
                }
            }
            Apply::With(f) => match f(value) {
                Value::Object(m) => m,
                _ => Map::new(),
            },
            Apply::Filter(_) => Map::new(),
        }
    }
}

/// The `items` of a page, narrowed by the parameters that filter rows.
#[must_use]
pub fn filtered(kind: &Kind, given: &BTreeMap<String, String>, items: Vec<Value>) -> Vec<Value> {
    let mut items = items;
    for p in kind.params {
        if let (Apply::Filter(keep), Some(v)) = (&p.apply, p.value(given)) {
            items.retain(|i| keep(i, v));
        }
    }
    items
}

/// The values the address gives for `kind`'s parameters (`p.<key>`), decoded; keys the kind does not have are left out.
#[must_use]
pub fn given(kind: &Kind, query: Option<&str>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in query.unwrap_or_default().split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let Some(key) = key.strip_prefix("p.") else {
            continue;
        };
        if kind.params.iter().any(|p| p.key == key) {
            out.insert(
                key.to_owned(),
                crate::api::resources::decode(&value.replace('+', " ")),
            );
        }
    }
    out
}

/// The members the parameters set over the list call, given what the address says.
#[must_use]
pub fn applied(kind: &Kind, given: &BTreeMap<String, String>) -> Map<String, Value> {
    let mut out = Map::new();
    for p in kind.params {
        if let Some(v) = p.value(given) {
            out.extend(p.members(v));
        }
    }
    out
}

/// How the page draws the parameters of `kind`, with their choices.
///
/// # Errors
/// The service's refusal of a call that lists a parameter's choices.
pub async fn specs(
    bus: &Bus,
    d: &Descriptor,
    kind: &Kind,
    given: &BTreeMap<String, String>,
    page: &[Value],
) -> Result<Vec<ParamSpec>, ApiError> {
    let mut out = Vec::new();
    for p in kind.params {
        if p.input == ParamKind::Hidden {
            continue;
        }
        // A row filter says how many rows of the page each of its choices keeps.
        let choice = |value: &str, label: &str| ParamOption {
            value: value.to_owned(),
            label: label.to_owned(),
            count: match p.apply {
                Apply::Filter(keep) => {
                    u32::try_from(page.iter().filter(|i| keep(i, value)).count()).ok()
                }
                _ => None,
            },
        };
        let options = match &p.options {
            Options::None => Vec::new(),
            Options::Fixed(list) => list.iter().map(|(v, l)| choice(v, l)).collect(),
            Options::Listed {
                step,
                items,
                item_as,
                value,
            } => {
                let (out, form) = (Value::Null, Map::new());
                let answer = run::call(d, step, &Fill::bare(bus, &out, &form)).await?;
                run::items_of(&answer, items, item_as)
                    .iter()
                    .filter_map(|i| value.read(i))
                    .map(|v| choice(&v, &v))
                    .collect()
            }
        };
        out.push(ParamSpec {
            key: p.key.to_owned(),
            label: p.label.to_owned(),
            input: p.input,
            options,
            value: p.value(given).map(str::to_owned),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Apply, Options, Param, ParamKind};

    const BUS: Param = Param::member(
        "bus",
        "Event bus",
        ParamKind::Select,
        Options::None,
        "EventBusName",
    );

    #[test]
    fn a_member_parameter_sets_its_member_and_a_default_stands_in() {
        let mut given = std::collections::BTreeMap::new();
        assert_eq!(BUS.value(&given), None);
        let with_default = BUS.or("default");
        assert_eq!(with_default.value(&given), Some("default"));
        given.insert("bus".to_owned(), "orders".to_owned());
        assert_eq!(with_default.value(&given), Some("orders"));
        assert_eq!(
            Value::Object(with_default.members("orders")),
            json!({"EventBusName": "orders"})
        );
    }

    #[test]
    fn a_template_is_filled_with_the_value_written_as_json() {
        let p = Param {
            apply: Apply::Template(r#"{"ParameterFilters":[{"Key":"Name","Values":["{value}"]}]}"#),
            ..BUS
        };
        assert_eq!(
            Value::Object(p.members(r#"/a"b"#)),
            json!({"ParameterFilters": [{"Key": "Name", "Values": ["/a\"b"]}]})
        );
    }

    #[test]
    fn the_address_gives_only_the_parameters_a_kind_has_decoded() {
        let ssm = crate::resources::catalog::descriptor("ssm").unwrap();
        let kind = &ssm.kinds[0];
        let given = super::given(
            kind,
            Some("token=x&p.filter=Name%3ABeginsWith%3A%2Fapp%2Fa+b&p.nope=1&p.filter.x=2"),
        );
        assert_eq!(given.len(), 1);
        assert_eq!(given["filter"], "Name:BeginsWith:/app/a b");
        assert!(super::given(kind, None).is_empty());
        let set = super::applied(kind, &given);
        assert_eq!(
            Value::Object(set),
            json!({"ParameterFilters": [{"Key": "Name", "Option": "BeginsWith", "Values": ["/app/a b"]}]})
        );
        assert!(super::applied(kind, &super::given(kind, Some("p.filter="))).is_empty());
    }

    #[test]
    fn a_row_filter_narrows_the_rows_and_counts_what_each_choice_keeps() {
        let cfn = crate::resources::catalog::descriptor("cloudformation").unwrap();
        let kind = &cfn.kinds[0];
        let rows = vec![
            json!({"StackStatus": "CREATE_COMPLETE"}),
            json!({"StackStatus": "UPDATE_COMPLETE"}),
            json!({"StackStatus": "CREATE_FAILED"}),
            json!({"StackStatus": "UPDATE_IN_PROGRESS"}),
            json!({"StackStatus": "UPDATE_ROLLBACK_COMPLETE"}),
        ];
        let pick =
            |q: &str| super::filtered(kind, &super::given(kind, Some(q)), rows.clone()).len();
        assert_eq!(pick("p.status=COMPLETE"), 2);
        assert_eq!(pick("p.status=FAILED"), 1);
        assert_eq!(pick("p.status=IN_PROGRESS"), 1);
        assert_eq!(pick("p.status=ROLLBACK"), 1);
        assert_eq!(pick("p.status="), 5);
        assert_eq!(pick(""), 5);
    }
}
