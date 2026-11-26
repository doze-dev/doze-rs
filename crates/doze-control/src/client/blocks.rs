//! The console's working blocks as typed calls (Rust daemon, additive): part of a value, a page of a tree folder, and
//! an action's form, whose refusal can name the field it is about.

use super::connection::Connection;
use crate::{ChildrenView, Error, Request, Response, ValueView};
use std::collections::BTreeMap;

impl Connection {
    /// Part of the value of `row` in `panel`, from byte `offset` (`value`). Only `wake` wakes a sleeping instance;
    /// without it the daemon says why it cannot read one (its code: [`crate::fault::ASLEEP`], `OFF`, `STARTING`,
    /// `FAILED`).
    pub fn value(
        &self,
        instance: &str,
        panel: &str,
        row: BTreeMap<String, String>,
        offset: u64,
        wake: bool,
    ) -> Result<ValueView, Error> {
        let r = self.call(Request::value(instance, panel, row, offset, wake))?;
        r.value.ok_or_else(|| Error::NoResponse("value".to_owned()))
    }

    /// One page of the children of `parent` in tree panel `panel` (`children`). Never wakes anything.
    pub fn children(
        &self,
        instance: &str,
        panel: &str,
        parent: &str,
        filter: &str,
        cursor: &str,
    ) -> Result<ChildrenView, Error> {
        let r = self.call(Request::children(instance, panel, parent, filter, cursor))?;
        Ok(r.children.unwrap_or_default())
    }

    /// Runs an action's form with every field's value (`admin` with `form`). The daemon's answer comes back whole:
    /// a refusal is in its `error`, with the field it is about in `field` when the module named one.
    pub fn admin_form(
        &self,
        instance: &str,
        action: &str,
        resource: &str,
        values: BTreeMap<String, String>,
    ) -> Result<Response, Error> {
        self.send(Request::admin_form(instance, action, resource, values))?
            .answer(self.timeout())
    }
}
