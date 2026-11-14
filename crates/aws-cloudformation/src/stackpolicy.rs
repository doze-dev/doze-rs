//! Stack policies, enforced on updates as AWS enforces them: once a stack has a policy, an update to a resource needs a statement
//! that allows its action (`Update:Modify`, `Update:Replace`, `Update:Delete`, or `Update:*`) on `LogicalResourceId/<id>` (globs
//! allowed), optionally conditioned on `ResourceType`; an explicit deny wins. `StackPolicyDuringUpdateBody` replaces it for one update.

use serde_json::Value;

/// Whether `policy` lets `action` happen to resource `logical` of type `ty`; the reason when it does not (memory: AWS's wording).
pub(crate) fn check(policy: &str, action: &str, logical: &str, ty: &str) -> Result<(), String> {
    let doc: Value = serde_json::from_str(policy).unwrap_or(Value::Null);
    let statements: Vec<&Value> = match doc.get("Statement") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(s @ Value::Object(_)) => vec![s],
        _ => Vec::new(),
    };
    let resource = format!("LogicalResourceId/{logical}");
    let mut allowed = false;
    for (i, s) in statements.iter().enumerate() {
        if !applies(s, action, &resource, ty) {
            continue;
        }
        if s.get("Effect").and_then(Value::as_str) == Some("Deny") {
            return Err(format!(
                "Action denied by stack policy: Statement [#{}] denies [{action}] on resource [{resource}]",
                i + 1
            ));
        }
        allowed = true;
    }
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "Action denied by stack policy: no statement allows [{action}] on resource [{resource}]"
        ))
    }
}

fn applies(s: &Value, action: &str, resource: &str, ty: &str) -> bool {
    let list = |k: &str| -> Option<Vec<String>> {
        match s.get(k)? {
            Value::String(x) => Some(vec![x.clone()]),
            Value::Array(a) => Some(
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect(),
            ),
            _ => None,
        }
    };
    let action_ok = match (list("Action"), list("NotAction")) {
        (Some(a), _) => a.iter().any(|p| glob(p, action)),
        (None, Some(n)) => !n.iter().any(|p| glob(p, action)),
        (None, None) => false,
    };
    let resource_ok = match (list("Resource"), list("NotResource")) {
        (Some(r), _) => r.iter().any(|p| glob(p, resource)),
        (None, Some(n)) => !n.iter().any(|p| glob(p, resource)),
        (None, None) => false,
    };
    action_ok && resource_ok && condition(s.get("Condition"), ty)
}

/// `{"StringEquals": {"ResourceType": [...]}}` or `StringLike`; anything else is taken as met.
fn condition(c: Option<&Value>, ty: &str) -> bool {
    let Some(Value::Object(ops)) = c else {
        return true;
    };
    ops.iter().all(|(op, keys)| {
        let values: Vec<&str> = match keys.get("ResourceType") {
            Some(Value::String(x)) => vec![x.as_str()],
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            _ => return true,
        };
        match op.as_str() {
            "StringEquals" => values.contains(&ty),
            "StringLike" => values.iter().any(|p| glob(p, ty)),
            "StringNotEquals" => !values.contains(&ty),
            "StringNotLike" => !values.iter().any(|p| glob(p, ty)),
            _ => true,
        }
    })
}

/// `*` and `?` globbing.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None, 0usize);
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some(pi);
                mark = ti;
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some(s) => {
                    pi = s + 1;
                    mark += 1;
                    ti = mark;
                }
                None => return false,
            },
        }
    }
    p.get(pi..)
        .is_none_or(|rest| rest.iter().all(|c| *c == '*'))
}

#[cfg(test)]
mod tests {
    use super::{check, glob};

    #[test]
    fn a_policy_allows_what_it_names_and_a_deny_wins() {
        let p = r#"{"Statement":[{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"},
            {"Effect":"Deny","Action":"Update:Replace","Principal":"*","Resource":"LogicalResourceId/Prod*"}]}"#;
        assert!(check(p, "Update:Modify", "ProdDb", "AWS::SQS::Queue").is_ok());
        assert!(check(p, "Update:Replace", "ProdDb", "AWS::SQS::Queue").is_err());
        assert!(check(p, "Update:Replace", "Dev", "AWS::SQS::Queue").is_ok());
        let typed = r#"{"Statement":[{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*",
            "Condition":{"StringEquals":{"ResourceType":["AWS::SQS::Queue"]}}}]}"#;
        assert!(check(typed, "Update:Modify", "Q", "AWS::SNS::Topic").is_err());
        assert!(glob("a*c?e", "abbbcde"));
        assert!(!glob("a*c", "abd"));
    }
}
