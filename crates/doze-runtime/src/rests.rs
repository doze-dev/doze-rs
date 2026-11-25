//! Which lazy services exist in a process, and the way to tell one to rest now.
//!
//! Every [`Lazy`](crate::Lazy) records itself here when it is made (by name, and by key for a [`Keyed`](crate::Keyed) one: the
//! AWS region), as a weak handle: the registry owns nothing and keeps nothing alive. It is read by a person's tool (the AWS
//! console's Lab: "rest this service now"), never by the services. Resting a service is the same close its idle timer ends in:
//! the actor stops taking requests, answers what is already queued, closes the service and ends, so the next call opens it
//! again.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError, Weak};

/// A lazy service as the registry sees it, whatever it serves.
pub(crate) trait Resting: Send + Sync {
    /// Whether an actor is running.
    fn live(&self) -> bool;
    /// Tells the running actor to close as it does when idle, and resolves when it has ended; `false` when none was running.
    fn rest(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

struct Entry {
    name: &'static str,
    key: String,
    at: Weak<dyn Resting>,
}

/// A lazy service the process has made, and whether it is running now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The service's name (`sqs`).
    pub name: &'static str,
    /// Its key: the region of a [`Keyed`](crate::Keyed) service, empty for one that has none.
    pub key: String,
    /// Whether it is running (it holds its memory and files) or resting.
    pub live: bool,
}

/// What [`Rests::rest`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rested {
    /// It was running; it has closed, and the next call opens it again.
    Closed,
    /// It was already resting: nothing to close.
    AlreadyResting,
    /// No service of that name and key has been made in this process (a region nobody has called holds nothing at all).
    Unknown,
}

/// The lazy services of a process (see the module text). One lives in each [`Tasks`](crate::Tasks), which every service of a
/// host shares.
#[derive(Default)]
pub struct Rests {
    entries: Mutex<Vec<Entry>>,
}

impl fmt::Debug for Rests {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        write!(f, "Rests({n})")
    }
}

impl Rests {
    /// Records a service, dropping the ones that no longer exist and the earlier one of the same name and key.
    pub(crate) fn add(&self, name: &'static str, key: &str, at: Weak<dyn Resting>) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|e| e.at.strong_count() > 0 && !(e.name == name && e.key == key));
        entries.push(Entry {
            name,
            key: key.to_owned(),
            at,
        });
    }

    /// The service `name` of `key`, if it was made and still exists.
    fn find(&self, name: &str, key: &str) -> Option<Arc<dyn Resting>> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|e| e.name == name && e.key == key)
            .and_then(|e| e.at.upgrade())
    }

    /// Every lazy service made in this process, by name and key, with whether it is running.
    #[must_use]
    pub fn seen(&self) -> Vec<Seen> {
        let live: Vec<(&'static str, String, Arc<dyn Resting>)> = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(|e| Some((e.name, e.key.clone(), e.at.upgrade()?)))
            .collect();
        let mut out: Vec<Seen> = live
            .into_iter()
            .map(|(name, key, at)| Seen {
                name,
                key,
                live: at.live(),
            })
            .collect();
        out.sort_by(|a, b| (a.name, &a.key).cmp(&(b.name, &b.key)));
        out
    }

    /// Whether the service `name` of `key` is running; `None` when none was made.
    #[must_use]
    pub fn is_live(&self, name: &str, key: &str) -> Option<bool> {
        self.find(name, key).map(|s| s.live())
    }

    /// Tells the service `name` of `key` to rest, as its idle timer would, and waits until it has closed.
    pub async fn rest(&self, name: &str, key: &str) -> Rested {
        match self.find(name, key) {
            None => Rested::Unknown,
            Some(s) if s.rest().await => Rested::Closed,
            Some(_) => Rested::AlreadyResting,
        }
    }
}
