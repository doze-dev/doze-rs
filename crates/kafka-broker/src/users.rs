//! The SCRAM credentials the broker keeps for `AlterUserScramCredentials`, between the table and the logins.
//!
//! The table in `kafka-meta` is what survives a restart; [`kafka_auth::Users`] is what logins read. The one is loaded
//! into the other when the broker opens, and every change is written to the table first and swapped into the
//! snapshot second, so a login never sees a credential the table does not have.

use std::num::NonZeroU32;

use kafka_auth::Users;
use kafka_auth::scram::{Kind, Verifier};
use kafka_meta::{CredentialId, CredentialRecord, MetaStore};
use tracing::warn;

use crate::BrokerError;

/// The mechanism number Kafka's `ScramMechanism` gives SCRAM-SHA-256.
pub(crate) const SHA_256: i8 = 1;
/// The mechanism number of SCRAM-SHA-512.
pub(crate) const SHA_512: i8 = 2;

/// The hash a mechanism number stands for; `None` for 0 (Kafka's `UNKNOWN`) and every other number.
pub(crate) fn kind_of(mechanism: i8) -> Option<Kind> {
    match mechanism {
        SHA_256 => Some(Kind::Sha256),
        SHA_512 => Some(Kind::Sha512),
        _ => None,
    }
}

/// The number of `kind` on the wire.
pub(crate) fn number_of(kind: Kind) -> i8 {
    match kind {
        Kind::Sha256 => SHA_256,
        Kind::Sha512 => SHA_512,
    }
}

/// The row that keeps `verifier` as `user`'s.
pub(crate) fn row_of(user: &str, verifier: &Verifier) -> (CredentialId, CredentialRecord) {
    let (stored_key, server_key) = verifier.keys();
    (
        CredentialId {
            user: user.to_owned(),
            mechanism: number_of(verifier.kind()),
        },
        CredentialRecord {
            salt: verifier.salt().to_vec(),
            iterations: verifier.iterations().get(),
            stored_key: stored_key.to_vec(),
            server_key: server_key.to_vec(),
        },
    )
}

/// Puts the stored credentials in force. A row that this program cannot use (a mechanism it does not know, keys of the
/// wrong size, no iterations) is left in the table, unused, and said so in the log: it is not the broker's to delete.
pub(crate) fn load(meta: &MetaStore, users: &Users) -> Result<(), BrokerError> {
    let mut stored = users.stored().as_ref().clone();
    for (id, record) in meta.credentials()? {
        // A row for the console's name (from a build that did not reserve it) is never put in force (SEC-240).
        if id.user == kafka_auth::CONSOLE_USER {
            warn!("a stored SCRAM credential for the reserved user __console is ignored");
            continue;
        }
        let verifier = kind_of(id.mechanism).and_then(|kind| {
            Verifier::from_parts(
                kind,
                &record.salt,
                NonZeroU32::new(record.iterations)?,
                &record.stored_key,
                &record.server_key,
            )
        });
        match verifier {
            Some(v) => stored = stored.with(&id.user, v),
            None => warn!(
                user = %id.user,
                mechanism = id.mechanism,
                "a stored SCRAM credential cannot be used and is left alone"
            ),
        }
    }
    users.replace_stored(stored);
    Ok(())
}

#[cfg(test)]
mod tests {
    use kafka_meta::{CredentialId, CredentialRecord};
    use kafka_storage::fs::Fs;
    use kafka_storage::sim::SimFs;

    use super::*;

    fn record() -> CredentialRecord {
        CredentialRecord {
            salt: vec![1; 16],
            iterations: 4096,
            stored_key: vec![2; 32],
            server_key: vec![3; 32],
        }
    }

    #[test]
    fn a_stored_credential_for_the_consoles_name_is_never_put_in_force() {
        let fs = SimFs::new();
        fs.create_dir_all(std::path::Path::new("/data")).unwrap();
        let meta = MetaStore::open(&fs, std::path::Path::new("/data")).unwrap();
        let row = |user: &str| {
            (
                CredentialId {
                    user: user.to_owned(),
                    mechanism: SHA_256,
                },
                record(),
            )
        };
        meta.alter_credentials(&[row("alice"), row(kafka_auth::CONSOLE_USER)], &[])
            .unwrap();
        let users = Users::none();
        load(&meta, &users).unwrap();
        let stored = users.stored();
        assert!(stored.verifier(Kind::Sha256, "alice").is_some());
        assert!(
            stored
                .verifier(Kind::Sha256, kafka_auth::CONSOLE_USER)
                .is_none(),
            "a login as the console would be a super user"
        );
    }
}
