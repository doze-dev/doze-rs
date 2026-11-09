//! The broker's dynamic configs (what AlterConfigs sets on a broker or on all of them), kept for the next start.

use std::collections::BTreeMap;

use redb::{ReadableDatabase, ReadableTable, TableError};

use super::{BROKER_CONFIGS, MetaStore, lock};
use crate::MetaError;

impl MetaStore {
    /// Every dynamic broker config, as (scope, name) to value. The scope is the broker's id, or empty for the default of
    /// all brokers.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn broker_configs(&self) -> Result<BTreeMap<(String, String), String>, MetaError> {
        let table = match self.db.begin_read()?.open_table(BROKER_CONFIGS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(BTreeMap::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = BTreeMap::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            if let Some((scope, name)) = key.value().split_once('/') {
                out.insert(
                    (scope.to_owned(), name.to_owned()),
                    value.value().to_owned(),
                );
            }
        }
        Ok(out)
    }

    /// Makes `configs` all that is set in `scope`, and makes that durable.
    ///
    /// # Errors
    /// A failed write; the configs are then as they were.
    pub fn set_broker_configs(
        &self,
        scope: &str,
        configs: &BTreeMap<String, String>,
    ) -> Result<(), MetaError> {
        let _one_at_a_time = lock(&self.write);
        let txn = self.begin()?;
        {
            let mut table = txn.open_table(BROKER_CONFIGS)?;
            let prefix = format!("{scope}/");
            let old: Vec<String> = table
                .iter()?
                .filter_map(Result::ok)
                .map(|(k, _)| k.value().to_owned())
                .filter(|k| k.starts_with(&prefix))
                .collect();
            for key in old {
                table.remove(key.as_str())?;
            }
            for (name, value) in configs {
                table.insert(format!("{prefix}{name}").as_str(), value.as_str())?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}
