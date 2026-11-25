//! The console's key browser on valkey (protocol minor 2): the keys of database 0 as a tree split on `:`, a key's
//! value by its type, and the form that sets a key. doze-resp does the work, shared with kvrocks.

use crate::{Valkey, ValkeySpec, admin};
use doze_module::caps::{Tree, Values};
use doze_module::proto::msg::blocks::{Children, ValueAnswer};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result};
use doze_resp::browse::{self, PANEL};
use std::collections::BTreeMap;

fn only_keys(panel: &str) -> Result<()> {
    if panel == PANEL {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::NotFound,
            format!("valkey has no panel {panel:?} of keys"),
        ))
    }
}

impl Values for Valkey {
    async fn value(
        &self,
        cx: &Cx<ValkeySpec>,
        panel: &str,
        row: &BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> Result<ValueAnswer> {
        only_keys(panel)?;
        doze_resp::value::value(&mut admin::client(cx).await?, row, offset, max).await
    }
}

impl Tree for Valkey {
    async fn children(
        &self,
        cx: &Cx<ValkeySpec>,
        panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<&str>,
    ) -> Result<Children> {
        only_keys(panel)?;
        browse::children(&mut admin::client(cx).await?, parent, filter, cursor).await
    }
}

/// Runs the form `key-set`; any other action runs as it always has.
pub(crate) async fn run_form(
    cx: &Cx<ValkeySpec>,
    action: &str,
    input: Option<&str>,
    values: &BTreeMap<String, String>,
) -> Result<String> {
    if action == "key-set" {
        return browse::set_key(&mut admin::client(cx).await?, values, input).await;
    }
    admin::run_action(cx, action).await
}
