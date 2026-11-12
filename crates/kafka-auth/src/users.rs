//! Who can log in, from two places: the users configured at start, and the SCRAM credentials stored at run time.
//!
//! The configured users are the **bootstrap** layer: they come from `--sasl-user`, the users file and the
//! environment, are derived once at each start, are never written down, and cannot be changed over the API. The
//! **stored** layer is what `AlterUserScramCredentials` keeps in the metadata database: an immutable snapshot
//! behind an `ArcSwap`, swapped whole after each change, so a login loads it with one atomic load and never waits.
//!
//! A name in both is the bootstrap user's, for every mechanism: the stored entries under that name are shadowed,
//! never consulted (deviation SEC-D50). PLAIN reads the bootstrap layer only, because a stored SCRAM verifier
//! cannot give a password back.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::credentials::{Credentials, CredentialsError};
use crate::principal::Mechanism;
use crate::scram::{Kind, Verifier};

/// The stored SCRAM verifiers: user, then mechanism, to the verifier. Immutable; a change makes a new one.
#[derive(Clone, Default)]
pub struct Stored {
    users: BTreeMap<String, BTreeMap<Kind, Arc<Verifier>>>,
}

impl Stored {
    /// The verifier of `user` for `kind`.
    #[must_use]
    pub fn verifier(&self, kind: Kind, user: &str) -> Option<Arc<Verifier>> {
        self.users.get(user)?.get(&kind).cloned()
    }

    /// These with `verifier` as `user`'s for its kind, replacing the one that was.
    #[must_use]
    pub fn with(&self, user: &str, verifier: Verifier) -> Self {
        let mut next = self.clone();
        next.users
            .entry(user.to_owned())
            .or_default()
            .insert(verifier.kind(), Arc::new(verifier));
        next
    }

    /// These without `user`'s verifier for `kind` (and without `user` if that was its last).
    #[must_use]
    pub fn without(&self, user: &str, kind: Kind) -> Self {
        let mut next = self.clone();
        if let Some(kinds) = next.users.get_mut(user) {
            kinds.remove(&kind);
            if kinds.is_empty() {
                next.users.remove(user);
            }
        }
        next
    }

    /// Whether there is no stored verifier.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    /// How many users have a stored verifier.
    #[must_use]
    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// Every user's verifiers, by user name and then mechanism.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arc<Verifier>)> {
        self.users
            .iter()
            .flat_map(|(user, kinds)| kinds.values().map(move |v| (user.as_str(), v)))
    }
}

impl fmt::Debug for Stored {
    /// The user names, and nothing of what they log in with.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stored")
            .field("users", &self.users.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// Both layers of users. Shared by every connection and by the handlers that change the stored layer.
pub struct Users {
    bootstrap: Credentials,
    stored: ArcSwap<Stored>,
}

impl Users {
    /// The bootstrap `credentials`, which must already hold the SCRAM verifiers of the mechanisms that are on.
    #[must_use]
    pub fn new(bootstrap: Credentials) -> Self {
        Self {
            bootstrap,
            stored: ArcSwap::from_pointee(Stored::default()),
        }
    }

    /// The bootstrap `credentials` with a verifier for every SCRAM mechanism among `mechanisms`.
    ///
    /// # Errors
    /// [`CredentialsError::NoEntropy`] if the system has no random bytes to give the salts.
    pub fn derive(
        credentials: Credentials,
        mechanisms: &[Mechanism],
    ) -> Result<Self, CredentialsError> {
        let kinds: Vec<Kind> = mechanisms.iter().filter_map(|&m| Kind::of(m)).collect();
        let derived = credentials.with_scram(&kinds)?;
        // Only PLAIN needs a password once the verifiers are made: without it none is kept (SEC-D59).
        let kept = if mechanisms.contains(&Mechanism::Plain) {
            derived
        } else {
            derived.forget_passwords()
        };
        Ok(Self::new(kept))
    }

    /// No bootstrap user: what a broker without SASL has.
    #[must_use]
    pub fn none() -> Self {
        Self::new(Credentials::default())
    }

    /// The users configured at start; PLAIN logs these in and nobody else.
    #[must_use]
    pub fn bootstrap(&self) -> &Credentials {
        &self.bootstrap
    }

    /// Whether `name` is a user configured at start.
    #[must_use]
    pub fn is_bootstrap(&self, name: &str) -> bool {
        self.bootstrap.has_user(name)
    }

    /// The SCRAM verifier of `name` for `kind`: the bootstrap user's if there is one by that name (and then no
    /// stored entry is looked at), else the stored one.
    #[must_use]
    pub fn verifier(&self, kind: Kind, name: &str) -> Option<Arc<Verifier>> {
        if self.is_bootstrap(name) {
            return self.bootstrap.verifier(kind, name);
        }
        self.stored.load().verifier(kind, name)
    }

    /// The stored verifiers now in force.
    #[must_use]
    pub fn stored(&self) -> Arc<Stored> {
        self.stored.load_full()
    }

    /// Puts `stored` in force, for the logins that begin after this. Whoever calls holds the lock that serialises
    /// changes, so two changes are never built from the same snapshot.
    pub fn replace_stored(&self, stored: Stored) {
        self.stored.store(Arc::new(stored));
    }

    /// Whether there is no user at all, configured or stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bootstrap.is_empty() && self.stored.load().is_empty()
    }

    /// How many users are configured at start.
    #[must_use]
    pub fn bootstrap_len(&self) -> usize {
        self.bootstrap.len()
    }

    /// The mechanisms `name` has a SCRAM credential for, with the iteration count of each, in mechanism order.
    /// The bootstrap user of that name if there is one, else the stored one.
    #[must_use]
    pub fn scram_of(&self, name: &str) -> Vec<(Kind, u32)> {
        if self.is_bootstrap(name) {
            return self.bootstrap.scram_of(name);
        }
        Kind::ALL
            .iter()
            .filter_map(|&k| Some((k, self.stored.load().verifier(k, name)?.iterations().get())))
            .collect()
    }

    /// Every user that has a SCRAM credential, by name, each with [`Users::scram_of`].
    #[must_use]
    pub fn scram_credentials(&self) -> Vec<(String, Vec<(Kind, u32)>)> {
        let mut names: Vec<String> = self.bootstrap.names().map(str::to_owned).collect();
        names.extend(self.stored.load().iter().map(|(user, _)| user.to_owned()));
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|name| {
                let kinds = self.scram_of(&name);
                (name, kinds)
            })
            .filter(|(_, kinds)| !kinds.is_empty())
            .collect()
    }
}

impl fmt::Debug for Users {
    /// The names of the users in each layer, and nothing of what they log in with.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Users")
            .field("bootstrap", &self.bootstrap)
            .field("stored", &*self.stored.load())
            .finish()
    }
}

#[cfg(test)]
mod tests;
