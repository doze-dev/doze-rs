//! The context keys a policy asks about: those named in its conditions, and the variables (`${aws:username}`) in its resources and
//! condition values. It is what `GetContextKeysForCustomPolicy` answers.

use crate::document::{Document, Statement};

/// Every key `docs` refer to, as first written and without repeats (keys are case-insensitive), in order of appearance.
#[must_use]
pub fn context_keys(docs: &[&Document]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |key: &str| {
        if !out.iter().any(|k| k.eq_ignore_ascii_case(key)) {
            out.push(key.to_owned());
        }
    };
    for st in docs.iter().flat_map(|d| &d.statements) {
        statement_keys(st, &mut add);
    }
    out
}

fn statement_keys(st: &Statement, add: &mut impl FnMut(&str)) {
    for c in &st.conditions {
        add(&c.key);
        c.values.iter().for_each(|v| variables(v, add));
    }
    for m in [&st.resources, &st.actions].into_iter().flatten() {
        m.patterns.iter().for_each(|p| variables(p, add));
    }
}

/// The names inside `${...}`, except the escapes `${*}`, `${?}` and `${$}`.
fn variables(s: &str, add: &mut impl FnMut(&str)) {
    let mut rest = s;
    while let Some(at) = rest.find("${") {
        let after = &rest[at + 2..];
        let Some(end) = after.find('}') else { return };
        let name = &after[..end];
        if !matches!(name, "*" | "?" | "$") && !name.is_empty() {
            add(name);
        }
        rest = &after[end + 1..];
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn keys_come_from_conditions_and_variables_once_each() {
        let d = crate::parse(
            r#"{"Statement":[{"Effect":"Allow","Action":"s3:*","Resource":"arn:aws:s3:::b/${aws:username}/*",
            "Condition":{"StringEquals":{"aws:PrincipalTag/team":"${aws:RequestTag/team}"},"Bool":{"AWS:USERNAME":"true"}}}]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            context_keys(&[&d]),
            [
                "AWS:USERNAME",
                "aws:PrincipalTag/team",
                "aws:RequestTag/team"
            ]
        );
    }
}
