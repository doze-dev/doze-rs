//! A Fail rule's "an error I choose": the errors the model lists for the operations a rule fits, and the check of the one that was chosen.
//!
//! - `GET /api/v1/lab/errors?services=<names>&operations=<names>` (names separated by commas): the errors every one of the operations
//!   lists, with the status the model gives each ([`crate::lab::listed`]). Nothing is invented: an operation whose model lists none says so.
//! - [`custom`] is what `POST /api/v1/lab/rules` and `/check` do with such an effect: the code must be one the model lists for every
//!   operation the rule fits, and its status is the model's. A code the model does not list is refused in words.

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::lab_build::{models_fitting, resolve};
use super::resources::decode;
use super::{json, param};
use crate::facts::Facts;
use crate::lab::listed::{self, Listed};
use crate::lab::rule::Fail;
use crate::lab::{AddError, Field, LONGEST_CODE, MOST_OPERATIONS, MOST_SERVICES};
use crate::model::{LabErrors, LabListedError};
use crate::state::Shared;

/// `SendMessage`, `SendMessage and ReceiveMessage`, `A, B and C`.
fn said(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The errors every one of `operations` lists in the APIs `services` (signing names or SDK ids) name; why not when there are none.
///
/// # Errors
/// What is missing from the rule for the list to be said, or that the operations share no error.
pub(super) fn offered(
    facts: &Facts,
    services: &[impl AsRef<str>],
    operations: &[impl AsRef<str>],
) -> Result<Vec<Listed>, AddError> {
    if services.is_empty() {
        return Err(AddError::CustomNeeds(Field::Services));
    }
    if operations.is_empty() {
        return Err(AddError::CustomNeeds(Field::Operations));
    }
    let models = models_fitting(facts, services);
    let ops: Vec<&str> = operations.iter().map(AsRef::as_ref).collect();
    let found = listed::common(&models, &ops);
    if found.is_empty() {
        return Err(AddError::NoCommonError {
            operations: said(&ops),
            several: ops.len() > 1,
        });
    }
    Ok(found)
}

/// The failure `code` stands for in a rule that fits `services` and `operations`: what the model gives it, or what is wrong. `status` is
/// what the form sent, which must agree with the model's when it is sent at all. The failure is built from what could be read so a
/// sentence can be said of a rule that cannot be made.
pub(super) fn custom(
    facts: &Facts,
    services: &[impl AsRef<str>],
    operations: &[impl AsRef<str>],
    status: Option<u16>,
    code: &str,
    problems: &mut Vec<AddError>,
) -> Fail {
    let code = code.trim();
    let stated = |shape| Fail::Custom {
        status: status.unwrap_or(500),
        code: code.into(),
        shape,
    };
    let list = match offered(facts, services, operations) {
        Ok(list) => list,
        Err(why) => {
            problems.push(why);
            return stated(None);
        }
    };
    if let Some(l) = listed::find(&list, code) {
        if let Some(given) = status.filter(|s| *s != l.status) {
            problems.push(AddError::WrongStatus {
                code: l.code.to_owned(),
                listed: l.status,
                given,
            });
        }
        return Fail::Custom {
            status: l.status,
            code: l.code.into(),
            shape: Some(l.shape),
        };
    }
    // A code of the wrong kind of characters is the form's own to say (`BadCode`); only a well-formed one is a code the model lacks.
    let plain = !code.is_empty()
        && code.chars().count() <= LONGEST_CODE
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    if plain {
        let ops: Vec<&str> = operations.iter().map(AsRef::as_ref).collect();
        problems.push(AddError::NotListed {
            code: code.to_owned(),
            operations: if ops.len() > 1 {
                format!("every one of {}", said(&ops))
            } else {
                said(&ops)
            },
        });
    }
    stated(None)
}

/// The words of a query parameter that lists names: separated by commas, empty entries left out, at most `most`.
fn names(query: Option<&str>, name: &str, most: usize) -> Vec<String> {
    param(query, name)
        .map(decode)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .take(most)
        .map(str::to_owned)
        .collect()
}

/// `GET /api/v1/lab/errors`.
pub(super) async fn errors(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let services: Vec<String> = names(query.as_deref(), "services", MOST_SERVICES)
        .iter()
        .filter_map(|w| resolve(&s.facts, w))
        .flatten()
        .collect();
    let operations = names(query.as_deref(), "operations", MOST_OPERATIONS);
    let answer = match offered(&s.facts, &services, &operations) {
        Ok(list) => LabErrors {
            errors: list
                .iter()
                .map(|l| LabListedError {
                    code: l.code.to_owned(),
                    status: l.status,
                    server: l.server,
                })
                .collect(),
            why: None,
        },
        Err(why) => LabErrors {
            errors: Vec::new(),
            why: Some(why.words()),
        },
    };
    json(StatusCode::OK, &answer)
}
