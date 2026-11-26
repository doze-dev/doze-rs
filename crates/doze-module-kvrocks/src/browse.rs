//! The console's key browser on kvrocks (protocol minor 2): the keys the module's own connection sees (the default
//! namespace, under the password) as a tree split on `:`, a key's value by its type (kvrocks's JSON values too), and
//! the form that sets a key there. doze-resp does the work, shared with valkey.

use crate::{Kvrocks, KvrocksSpec, admin};
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
            format!("kvrocks has no panel {panel:?} of keys"),
        ))
    }
}

impl Values for Kvrocks {
    async fn value(
        &self,
        cx: &Cx<KvrocksSpec>,
        panel: &str,
        row: &BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> Result<ValueAnswer> {
        only_keys(panel)?;
        let mut c = admin::client(cx, cx.spec.password()).await?;
        doze_resp::value::value(&mut c, row, offset, max).await
    }
}

impl Tree for Kvrocks {
    async fn children(
        &self,
        cx: &Cx<KvrocksSpec>,
        panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<&str>,
    ) -> Result<Children> {
        only_keys(panel)?;
        let mut c = admin::client(cx, cx.spec.password()).await?;
        browse::children(&mut c, parent, filter, cursor).await
    }
}

/// Runs the form `key-set`; any other action runs as it always has.
pub(crate) async fn run_form(
    cx: &Cx<KvrocksSpec>,
    action: &str,
    resource: &str,
    input: Option<&str>,
    values: &BTreeMap<String, String>,
) -> Result<String> {
    if action == "key-set" {
        let mut c = admin::client(cx, cx.spec.password()).await?;
        return browse::set_key(&mut c, values, input).await;
    }
    admin::run_action(cx, action, resource).await
}
