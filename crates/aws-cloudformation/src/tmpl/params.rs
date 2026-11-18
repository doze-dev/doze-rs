//! Parameters: their declarations, the values a call binds to them (given, previous or default), the constraints CloudFormation
//! checks, and what `Ref` yields for each type. SSM-typed parameters are resolved from the local SSM at create or update.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use aws_wire::ApiError;

use crate::errors::{format, validation};
use crate::j::J;

/// One declared parameter.
#[derive(Clone, Debug, Default)]
pub(crate) struct Decl {
    pub(crate) key: String,
    pub(crate) ty: String,
    pub(crate) default: Option<String>,
    pub(crate) no_echo: bool,
    pub(crate) description: Option<String>,
    allowed_values: Vec<String>,
    allowed_pattern: Option<String>,
    min_length: Option<u64>,
    max_length: Option<u64>,
    min_value: Option<f64>,
    max_value: Option<f64>,
    constraint: Option<String>,
}

/// A value bound to a parameter, kept with the stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Bound {
    pub(crate) key: String,
    /// The value given (or defaulted), as text.
    pub(crate) value: String,
    /// What an SSM-typed parameter resolved to.
    pub(crate) resolved: Option<String>,
    pub(crate) no_echo: bool,
}

/// A parameter as a call gives it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Given {
    pub(crate) key: String,
    pub(crate) value: Option<String>,
    pub(crate) use_previous: bool,
}

/// Text of a scalar written in a template (`Default: 30`).
pub(crate) fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(a) => Some(a.iter().filter_map(text_of).collect::<Vec<_>>().join(",")),
        _ => None,
    }
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Whether CloudFormation knows a parameter type.
fn known_type(ty: &str) -> bool {
    let inner = ty
        .strip_prefix("List<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(ty);
    matches!(
        ty,
        "String" | "Number" | "List<Number>" | "CommaDelimitedList" | "AWS::SSM::Parameter::Name"
    ) || ty.starts_with("AWS::SSM::Parameter::Value<") && ty.ends_with('>')
        || inner.starts_with("AWS::EC2::")
        || inner == "AWS::Route53::HostedZone::Id"
}

/// The declared parameters, in the order written.
///
/// # Errors
/// A declaration CloudFormation refuses.
pub(crate) fn declarations(t: &Value) -> Result<Vec<Decl>, ApiError> {
    let Some(ps) = t.get("Parameters") else {
        return Ok(Vec::new());
    };
    let Some(ps) = ps.as_object() else {
        return Err(format("Parameters must be an object")); // (memory)
    };
    let mut out = Vec::new();
    for (key, p) in ps {
        let ty = p.get("Type").and_then(Value::as_str).unwrap_or_default();
        if ty.is_empty() {
            // (memory)
            return Err(format(format!("[/Parameters/{key}] 'Type' is required")));
        }
        if !known_type(ty) {
            // (memory)
            return Err(format(format!("Unrecognized parameter type: {ty}")));
        }
        let s = |k: &str| p.get(k).and_then(text_of);
        out.push(Decl {
            key: key.clone(),
            ty: ty.to_owned(),
            default: s("Default"),
            no_echo: p
                .get("NoEcho")
                .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true")),
            description: s("Description"),
            allowed_values: p
                .get("AllowedValues")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(text_of).collect())
                .unwrap_or_default(),
            allowed_pattern: s("AllowedPattern"),
            min_length: number(p.get("MinLength")).map(|n| n as u64),
            max_length: number(p.get("MaxLength")).map(|n| n as u64),
            min_value: number(p.get("MinValue")),
            max_value: number(p.get("MaxValue")),
            constraint: s("ConstraintDescription"),
        });
    }
    Ok(out)
}

/// Checks the declarations alone (for `ValidateTemplate`).
///
/// # Errors
/// As [`declarations`].
pub(crate) fn check_declarations(t: &Value) -> Result<(), ApiError> {
    declarations(t).map(|_| ())
}

impl Decl {
    /// Whether `Ref` gives a list.
    pub(crate) fn is_list(&self) -> bool {
        self.ty.starts_with("List<")
            || self.ty == "CommaDelimitedList"
            || self.ty.contains("<List<")
            || self.ty.contains("<CommaDelimitedList>")
    }

    /// Whether the value names an SSM parameter whose value is used.
    pub(crate) fn reads_ssm(&self) -> bool {
        self.ty.starts_with("AWS::SSM::Parameter::Value<")
    }

    /// `GetTemplateSummary`'s `ParameterDeclaration`.
    pub(crate) fn summary(&self) -> Value {
        let mut v = json!({"ParameterKey": self.key, "ParameterType": self.ty, "NoEcho": self.no_echo,
            "ParameterConstraints": {}});
        if let Some(d) = &self.default {
            v.put("DefaultValue", d.clone().into());
        }
        if let Some(d) = &self.description {
            v.put("Description", d.clone().into());
        }
        if !self.allowed_values.is_empty() {
            v.put(
                "ParameterConstraints",
                json!({"AllowedValues": self.allowed_values}),
            );
        }
        v
    }

    /// Checks one value against the declaration's constraints, in AWS's words.
    fn admit(&self, value: &str) -> Result<(), ApiError> {
        let key = &self.key;
        let refuse = |plain: String| {
            Err(validation(match &self.constraint {
                // (memory) A ConstraintDescription replaces the generic sentence.
                Some(c) => format!("Parameter '{key}' failed to satisfy constraint: {c}"),
                None => plain,
            }))
        };
        let items: Vec<&str> = if self.is_list() {
            value.split(',').map(str::trim).collect()
        } else {
            vec![value]
        };
        for item in &items {
            if self.ty == "Number" || self.ty == "List<Number>" {
                let Ok(n) = item.parse::<f64>() else {
                    return refuse(format!("Parameter '{key}' must be a number")); // (memory)
                };
                if let Some(max) = self.max_value
                    && n > max
                {
                    // Recorded 2026-11-12.
                    return refuse(format!(
                        "Parameter '{key}' must be a number not greater than {}",
                        trim_float(max)
                    ));
                }
                if let Some(min) = self.min_value
                    && n < min
                {
                    // (memory)
                    return refuse(format!(
                        "Parameter '{key}' must be a number not less than {}",
                        trim_float(min)
                    ));
                }
            }
            if !self.allowed_values.is_empty() && !self.allowed_values.iter().any(|a| a == item) {
                return refuse(format!("Parameter '{key}' must be one of AllowedValues")); // (memory)
            }
        }
        let len = value.chars().count() as u64;
        if let Some(min) = self.min_length
            && len < min
        {
            return refuse(format!(
                "Parameter '{key}' must contain at least {min} characters"
            )); // (memory)
        }
        if let Some(max) = self.max_length
            && len > max
        {
            return refuse(format!(
                "Parameter '{key}' must contain at most {max} characters"
            )); // (memory)
        }
        if let Some(p) = &self.allowed_pattern {
            let anchored = format!("^(?:{p})$");
            let ok = regex_lite::Regex::new(&anchored)
                .is_ok_and(|re| items.iter().all(|i| re.is_match(i)));
            if !ok {
                return refuse(format!("Parameter '{key}' must match pattern {p}")); // (memory)
            }
        }
        Ok(())
    }
}

fn trim_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// Binds the values a call gives (or the previous ones, or the defaults) to the declared parameters, and checks them.
///
/// # Errors
/// `ValidationError` in AWS's words: unknown parameters first, then the ones with no value, then each constraint.
pub(crate) fn bind(
    decls: &[Decl],
    given: &[Given],
    previous: &[Bound],
) -> Result<Vec<Bound>, ApiError> {
    let unknown: Vec<&str> = given
        .iter()
        .map(|g| g.key.as_str())
        .filter(|k| !decls.iter().any(|d| d.key == *k))
        .collect();
    if !unknown.is_empty() {
        // Recorded 2026-11-12.
        return Err(validation(format!(
            "Parameters: [{}] do not exist in the template",
            unknown.join(", ")
        )));
    }
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for d in decls {
        let g = given.iter().find(|g| g.key == d.key);
        let value = match g {
            Some(g) if g.use_previous => match previous.iter().find(|p| p.key == d.key) {
                Some(p) => Some(p.value.clone()),
                // (memory)
                None => {
                    return Err(validation(format!(
                        "Invalid input for parameter key {}. Cannot specify usePreviousValue as true for a parameter key not in the previous template",
                        d.key
                    )));
                }
            },
            Some(g) => g.value.clone(),
            None => d.default.clone(),
        };
        let Some(value) = value else {
            missing.push(d.key.clone());
            continue;
        };
        out.push(Bound {
            key: d.key.clone(),
            value,
            resolved: None,
            no_echo: d.no_echo,
        });
    }
    if !missing.is_empty() {
        // Recorded 2026-11-12.
        return Err(validation(format!(
            "Parameters: [{}] must have values",
            missing.join(", ")
        )));
    }
    for (d, b) in decls.iter().zip(&out) {
        if !d.reads_ssm() {
            d.admit(&b.value)?;
        }
    }
    Ok(out)
}

/// What `Ref` gives for a bound parameter: text, or a list for the list types.
pub(crate) fn ref_value(d: &Decl, b: &Bound) -> Value {
    let text = b.resolved.as_deref().unwrap_or(&b.value);
    if d.is_list() {
        if text.is_empty() {
            return Value::Array(Vec::new());
        }
        Value::Array(
            text.split(',')
                .map(|s| Value::String(s.trim().to_owned()))
                .collect(),
        )
    } else {
        Value::String(text.to_owned())
    }
}

/// The parameters as `DescribeStacks` shows them (a `NoEcho` value is masked).
pub(crate) fn described(bound: &[Bound]) -> Vec<Value> {
    bound
        .iter()
        .map(|b| {
            let mut v = json!({"ParameterKey": b.key,
                "ParameterValue": if b.no_echo { "****".to_owned() } else { b.value.clone() }});
            if let Some(r) = &b.resolved {
                v.put(
                    "ResolvedValue",
                    if b.no_echo {
                        "****".into()
                    } else {
                        r.clone().into()
                    },
                );
            }
            v
        })
        .collect()
}
