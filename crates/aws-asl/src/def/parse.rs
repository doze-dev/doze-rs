//! A definition's text to a [`Definition`], with every problem AWS reports.

use serde_json::Value;

use super::fields::{Obj, SCHEMA};
use super::{Definition, Diagnostic, Lang};

/// How deep branches and item processors may nest.
const DEEPEST: usize = 32;

// fuzz:parser fuzz_definition
/// Reads and checks a definition.
///
/// # Errors
/// Every problem found, in the order AWS lists them.
pub fn parse(text: &str) -> Result<Definition, Vec<Diagnostic>> {
    let value = crate::jackson::read(text).map_err(|e| {
        let located = e.located(text);
        let at = located
            .split_once("\n at ")
            .map_or_else(String::new, |(_, at)| at.to_owned());
        vec![Diagnostic {
            code: "INVALID_JSON_DESCRIPTION",
            message: located,
            location: Some(at),
            warning: false,
        }]
    })?;
    parse_value(&value)
}

/// Checks a definition already read as JSON.
///
/// # Errors
/// Every problem found.
pub fn parse_value(value: &Value) -> Result<Definition, Vec<Diagnostic>> {
    let mut out = Vec::new();
    let Value::Object(map) = value else {
        // (memory)
        return Err(vec![Diagnostic::error(
            SCHEMA,
            "The definition must be a JSON object",
            "",
        )]);
    };
    let obj = Obj::new(map, String::new());
    obj.only(
        &[
            "Comment",
            "StartAt",
            "States",
            "TimeoutSeconds",
            "Version",
            "QueryLanguage",
        ],
        &mut out,
    );
    let lang = language(&obj, Lang::JsonPath, &mut out);
    let def = machine(&obj, lang, 0, &mut out);
    if let Some(d) = &def {
        super::check::machine(d, "", true, &mut out);
    }
    match def {
        Some(d) if out.iter().all(|d| d.warning) => Ok(d),
        _ => Err(out),
    }
}

/// `QueryLanguage` of a machine or a state, defaulting to `inherited`.
pub(super) fn language(obj: &Obj<'_>, inherited: Lang, out: &mut Vec<Diagnostic>) -> Lang {
    match obj.text("QueryLanguage", out).as_deref() {
        None => inherited,
        Some("JSONata") => Lang::Jsonata,
        Some("JSONPath") if inherited == Lang::Jsonata => {
            // (memory)
            out.push(Diagnostic::error(
                SCHEMA,
                "The QueryLanguage of a state cannot be JSONPath when the state machine's is JSONata",
                &obj.here("QueryLanguage"),
            ));
            inherited
        }
        Some("JSONPath") => Lang::JsonPath,
        Some(_) => {
            out.push(Diagnostic::error(
                SCHEMA,
                "The field 'QueryLanguage' should have one of these values: [JSONPath, JSONata]",
                &obj.here("QueryLanguage"),
            ));
            inherited
        }
    }
}

/// A (sub-)machine's `StartAt` and `States`.
pub(super) fn machine(
    obj: &Obj<'_>,
    lang: Lang,
    depth: usize,
    out: &mut Vec<Diagnostic>,
) -> Option<Definition> {
    if depth > DEEPEST {
        out.push(Diagnostic::error(
            SCHEMA,
            "The definition is nested too deeply",
            &obj.at,
        ));
        return None;
    }
    let complete = obj.require(&["StartAt", "States"], out);
    let start_at = obj.text("StartAt", out).unwrap_or_default();
    let timeout_seconds = obj.number("TimeoutSeconds", out);
    let mut states = Vec::new();
    if let Some(map) = obj.object("States", out) {
        for (name, body) in map {
            let at = format!("{}/States/{}", obj.at, pointer_escape(name));
            if let Some(s) = super::state::state(name, body, lang, &at, depth, out) {
                states.push(s);
            }
        }
    }
    if !complete && states.is_empty() {
        return None;
    }
    Some(Definition {
        start_at,
        states,
        timeout_seconds,
        lang,
    })
}

/// A name as a JSON pointer writes it.
fn pointer_escape(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::super::refusal;
    use super::parse;

    fn refused(text: &str) -> String {
        refusal(&parse(text).unwrap_err())
    }

    #[test]
    fn the_recorded_refusals() {
        assert_eq!(
            refused(r#"{"States": {"A": {"Type": "Succeed"}}}"#),
            "Invalid State Machine Definition: 'SCHEMA_VALIDATION_FAILED: These fields are required: [StartAt] at /'"
        );
        assert_eq!(
            refused(r#"{"StartAt": "Z", "States": {"A": {"Type": "Succeed"}}}"#),
            "Invalid State Machine Definition: 'MISSING_TRANSITION_TARGET: Missing 'Next' target: Z at /StartAt, MISSING_TRANSITION_TARGET: State \"A\" is not reachable. at /States/A'"
        );
        assert_eq!(
            refused(r#"{"StartAt": "A", "States": {"A": {"Type": "Pass", "Next": "A"}}}"#),
            "Invalid State Machine Definition: 'MISSING_END_STATE: Workflow has no terminal state at null'"
        );
        assert_eq!(
            refused(r#"{"StartAt": "A", "States": {"A": {"Type": "Shrug"}}}"#),
            "Invalid State Machine Definition: 'SCHEMA_VALIDATION_FAILED: The field 'Type' should have one of these values: [Task, Wait, Pass, Succeed, Fail, Choice, Parallel, Map] at /States/A/Type, MISSING_END_STATE: Workflow has no terminal state at null'"
        );
        assert_eq!(
            refused(r#"{"StartAt": "A", "States": {"A": {"Type": "Pass", "Next": "Nowhere"}}}"#),
            "Invalid State Machine Definition: 'MISSING_TRANSITION_TARGET: Missing 'Next' target: Nowhere at /States/A/Next, MISSING_END_STATE: Workflow has no terminal state at null'"
        );
        assert_eq!(
            refused("{nope"),
            "Invalid State Machine Definition: 'INVALID_JSON_DESCRIPTION: Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: (String)\"{nope\"; line: 1, column: 2] at [Source: (String)\"{nope\"; line: 1, column: 2]'"
        );
    }

    #[test]
    fn a_good_machine_reads() {
        let d = parse(r#"{"StartAt": "A", "States": {"A": {"Type": "Pass", "Next": "B"}, "B": {"Type": "Succeed"}}}"#).unwrap();
        assert_eq!(d.states.len(), 2);
        assert!(d.state("B").unwrap().terminal());
    }
}
