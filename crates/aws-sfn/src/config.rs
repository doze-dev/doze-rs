//! What the host may set for local testing (neither is AWS behaviour, and both are off unless asked for):
//!
//! - **Wait scaling**: every `Wait` and every retry interval is multiplied by `wait_scale` (`0.01` makes a `Wait` of an hour 36
//!   seconds). The history and the context object still say the times the definition asked for.
//! - **Mocked tasks, in Step Functions Local's format**: a JSON file with `StateMachines.<machine>.TestCases.<case>.<state>` naming
//!   a response in `MockedResponses.<name>`, whose keys are call numbers (`"0"`, `"1-2"`) and whose values are `{"Return": ...}`
//!   or `{"Throw": {"Error", "Cause"}}`. An execution started on `<machine ARN>#<case>` runs that test case: a mocked state's task
//!   is answered from the file and never called.

use std::path::Path;

use serde_json::Value;

/// The local-testing settings.
#[derive(Clone, Debug)]
pub struct Config {
    /// What every Wait and retry interval is multiplied by (1 is AWS).
    pub wait_scale: f64,
    /// The mock configuration, already read.
    pub mocks: Option<Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wait_scale: 1.0,
            mocks: None,
        }
    }
}

impl Config {
    /// Reads Step Functions Local's mock configuration file.
    ///
    /// # Errors
    /// The file cannot be read or is not JSON.
    pub fn read_mocks(path: &Path) -> Result<Value, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The mocked answer for call `n` (from 0) of state `state` in test case `case` of machine `machine`.
    pub(crate) fn mocked(
        &self,
        machine: &str,
        case: &str,
        state: &str,
        n: u32,
    ) -> Option<aws_asl::Outcome> {
        let mocks = self.mocks.as_ref()?;
        let name = mocks
            .get("StateMachines")?
            .get(machine)?
            .get("TestCases")?
            .get(case)?
            .get(state)?
            .as_str()?;
        let responses = mocks.get("MockedResponses")?.get(name)?.as_object()?;
        let answer = responses
            .iter()
            .find(|(k, _)| covers(k, n))
            .map(|(_, v)| v)?;
        if let Some(r) = answer.get("Return") {
            return Some(aws_asl::Outcome::Done(r.clone()));
        }
        let t = answer.get("Throw")?;
        let text = |k: &str| {
            t.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Some(aws_asl::Outcome::Failed(aws_asl::Failure::new(
            text("Error"),
            text("Cause"),
        )))
    }
}

/// Whether a key (`"2"` or `"1-3"`) names call `n`.
fn covers(key: &str, n: u32) -> bool {
    match key.split_once('-') {
        Some((a, b)) => {
            a.trim().parse::<u32>().is_ok_and(|a| a <= n)
                && b.trim().parse::<u32>().is_ok_and(|b| n <= b)
        }
        None => key.trim().parse::<u32>() == Ok(n),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn mocks_answer_by_call_number() {
        let c = Config {
            wait_scale: 1.0,
            mocks: Some(json!({
                "StateMachines": {"m": {"TestCases": {"happy": {"Call": "ok"}, "sad": {"Call": "flaky"}}}},
                "MockedResponses": {
                    "ok": {"0": {"Return": {"v": 1}}},
                    "flaky": {"0-1": {"Throw": {"Error": "Boom", "Cause": "c"}}, "2": {"Return": 2}}
                }
            })),
        };
        assert_eq!(
            c.mocked("m", "happy", "Call", 0),
            Some(aws_asl::Outcome::Done(json!({"v": 1})))
        );
        assert!(
            matches!(c.mocked("m", "sad", "Call", 1), Some(aws_asl::Outcome::Failed(f)) if f.error == "Boom")
        );
        assert_eq!(
            c.mocked("m", "sad", "Call", 2),
            Some(aws_asl::Outcome::Done(json!(2)))
        );
        assert_eq!(c.mocked("m", "sad", "Other", 0), None);
    }
}
