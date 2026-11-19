//! Who the service answers for: the account it remembers (a wake-up after a restart has no caller to say it), and which namespaces
//! its alarms watch, told to the bus.

use aws_wire::service::Caller;

use crate::core::Core;
use crate::model::{Doc, doc_key};

impl Core {
    /// After a reset: tells the bus again, from nothing, which namespaces an alarm watches.
    pub(crate) fn forget_watched(&mut self, region: &str) {
        self.told_watched = None;
        self.tell_watched(region);
    }

    /// Tells the bus which namespaces this region's metric alarms watch, when that changed, so a producer that holds its data
    /// until it is read (Lambda) sends the data an alarm needs as it makes it.
    pub(crate) fn tell_watched(&mut self, region: &str) {
        let namespaces = self.watched_namespaces();
        if self.told_watched.as_ref() == Some(&namespaces) {
            return;
        }
        if let Some(peers) = &self.peers {
            peers.watch_namespaces(region, namespaces.clone());
        }
        self.told_watched = Some(namespaces);
    }

    /// Keeps the account the service answers for, so that a wake-up with nobody calling (after a restart) can still build the ARN of
    /// an alarm for its notification.
    pub(crate) fn remember_account(&mut self, account: &str) {
        let key = doc_key("meta", "account");
        if self.docs.get(&key).is_none_or(|d| d.json != account) {
            let updated_ms = self.now_ms();
            self.docs.insert(
                key,
                Doc {
                    json: account.to_owned(),
                    updated_ms,
                    tags: Default::default(),
                },
            );
        }
    }

    /// The account the service answers for, as last remembered (empty before any call).
    pub(crate) fn account_hint(&self) -> String {
        self.docs
            .get(&doc_key("meta", "account"))
            .map(|d| d.json.clone())
            .unwrap_or_default()
    }

    /// A caller with the account filled in from what was remembered, if it has none.
    pub(crate) fn identified(&self, mut caller: Caller) -> Caller {
        if caller.account_id.is_empty() {
            caller.account_id = self.account_hint();
        }
        caller
    }
}
