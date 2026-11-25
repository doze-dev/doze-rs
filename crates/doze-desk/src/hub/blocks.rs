//! The console's working blocks, asked of the daemon as a person looks (Rust daemon, additive): part of a value, a
//! page of a tree folder, an action's form. Each on a short-lived connection of its own and blocking, so the UI calls
//! them from a background task. Nothing here wakes an instance unless the person pressed to open a value (`wake`).

use super::Hub;
use crate::error::Error;
use doze_control::{ChildrenView, ValueView};
use doze_projects::StackKey;
use std::collections::BTreeMap;

/// What a form came to: what the module said, or its refusal and the field it is about (empty when it is about the
/// form as a whole).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormOutcome {
    Done(String),
    Refused { message: String, field: String },
}

/// What a block request is: an instance's panel and what in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockAsk {
    pub instance: String,
    pub panel: String,
}

impl Hub {
    /// Part of the value of `row` (a table's selected row's cells, or `{"id": leaf}`) from byte `offset`.
    pub fn value(
        &self,
        key: &StackKey,
        at: &BlockAsk,
        row: BTreeMap<String, String>,
        offset: u64,
        wake: bool,
    ) -> Result<ValueView, Error> {
        Ok(self
            .live_client(key)?
            .dial()?
            .value(&at.instance, &at.panel, row, offset, wake)?)
    }

    /// One page of the children of `parent` ("" for the top), filtered, from `cursor`.
    pub fn children(
        &self,
        key: &StackKey,
        at: &BlockAsk,
        parent: &str,
        filter: &str,
        cursor: &str,
    ) -> Result<ChildrenView, Error> {
        Ok(self.live_client(key)?.dial()?.children(
            &at.instance,
            &at.panel,
            parent,
            filter,
            cursor,
        )?)
    }

    /// Runs an action's form with every field's value, blanks included.
    pub fn admin_form(
        &self,
        key: &StackKey,
        inst: &str,
        (action, resource): (&str, &str),
        values: BTreeMap<String, String>,
    ) -> Result<FormOutcome, Error> {
        let r = self
            .live_client(key)?
            .dial()?
            .admin_form(inst, action, resource, values)?;
        Ok(if r.error.is_empty() {
            FormOutcome::Done(r.result)
        } else {
            FormOutcome::Refused {
                message: r.error,
                field: r.field,
            }
        })
    }
}
