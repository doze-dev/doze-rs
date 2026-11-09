//! The broker-wide defaults as they are now: what the command line said, as AlterConfigs on the broker has changed
//! them. One copy, shared by everything that reads them, so a change is seen by whatever looks next.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock};

use super::broker;
use crate::Defaults;

/// What has been set dynamically: for all brokers (the cluster scope) and for this one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Dynamic {
    pub(crate) cluster: BTreeMap<String, String>,
    pub(crate) own: BTreeMap<String, String>,
}

#[derive(Debug)]
struct State {
    /// What the command line gave.
    started_with: Defaults,
    dynamic: Dynamic,
    /// The two laid together: this broker's own over the cluster's over the command line's.
    now: Defaults,
}

/// A shared handle to the defaults in force.
#[derive(Debug, Clone)]
pub(crate) struct Live(Arc<RwLock<State>>);

impl Live {
    pub(crate) fn new(started_with: Defaults, dynamic: Dynamic) -> Self {
        let now = broker::overlay(started_with, &dynamic);
        Self(Arc::new(RwLock::new(State {
            started_with,
            dynamic,
            now,
        })))
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// The defaults in force.
    pub(crate) fn get(&self) -> Defaults {
        self.read().now
    }

    /// What the command line gave, before any dynamic change.
    pub(crate) fn started_with(&self) -> Defaults {
        self.read().started_with
    }

    /// What has been set dynamically.
    pub(crate) fn dynamic(&self) -> Dynamic {
        self.read().dynamic.clone()
    }

    /// Takes the dynamic configs as they are to be.
    pub(crate) fn set(&self, dynamic: Dynamic) {
        let mut state = self.0.write().unwrap_or_else(PoisonError::into_inner);
        state.now = broker::overlay(state.started_with, &dynamic);
        state.dynamic = dynamic;
    }
}
