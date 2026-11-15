//! What `TestState` shows of a state's data flow (`inspectionData`): the input after `InputPath` and `Parameters` (or JSONata's
//! `Arguments`), and a result after `ResultSelector` and `ResultPath`. Each is worked out on its own from the frame as it stands
//! before the state runs; a step that fails is left out (the run itself reports the failure).

use serde_json::Value;

use super::io::{View, parse};
use super::{Exec, sub};
use crate::def::{Definition, Kind, Lang};
use crate::env::Env;

impl Exec {
    /// `afterInputPath` and `afterParameters` (JSONPath), or `afterArguments` (JSONata), of frame `frame`'s current state.
    #[must_use]
    pub fn inspect_input(
        &self,
        def: &Definition,
        frame: u32,
        env: &Env,
    ) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        let Some(f) = self.frame(frame) else {
            return out;
        };
        let Some(d) = sub(def, &f.def) else {
            return out;
        };
        let name = if f.state.is_empty() {
            d.start_at.as_str()
        } else {
            f.state.as_str()
        };
        let Some(state) = d.state(name) else {
            return out;
        };
        let mut f = f.clone();
        f.state = name.to_owned();
        let view = View {
            ex: self,
            frame: &f,
            state,
            env,
        };
        let raw = parse(&f.input);
        if state.lang == Lang::Jsonata {
            if let Some(a) = &state.arguments
                && let Ok(Some(v)) = view.jsonata("Arguments", a, &view.states(&raw, &[]))
            {
                out.push(("afterArguments", v));
            }
            return out;
        }
        let Ok(eff) = view.effective(&raw) else {
            return out;
        };
        out.push(("afterInputPath", eff.clone()));
        let params = match &state.kind {
            Kind::Map(m) => m.item_selector.as_ref(),
            _ => state.parameters.as_ref(),
        };
        let after = match params {
            Some(p) => view.template(p, &eff, None).ok(),
            None => Some(eff),
        };
        if let Some(a) = after {
            out.push(("afterParameters", a));
        }
        out
    }

    /// `afterResultSelector` and `afterResultPath` of `result` in frame `frame`'s current state (JSONPath only).
    #[must_use]
    pub fn inspect_result(
        &self,
        def: &Definition,
        frame: u32,
        result: &Value,
        env: &Env,
    ) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        let Some(f) = self.frame(frame) else {
            return out;
        };
        let Some(state) = sub(def, &f.def).and_then(|d| d.state(&f.state)) else {
            return out;
        };
        if state.lang == Lang::Jsonata {
            return out;
        }
        let view = View {
            ex: self,
            frame: f,
            state,
            env,
        };
        let selected = match &state.result_selector {
            Some(s) => view.template(s, result, None).ok(),
            None => Some(result.clone()),
        };
        let Some(selected) = selected else { return out };
        out.push(("afterResultSelector", selected.clone()));
        if let Ok(merged) = view.merge(&state.result_path, &parse(&f.input), selected) {
            out.push(("afterResultPath", merged));
        }
        out
    }
}
