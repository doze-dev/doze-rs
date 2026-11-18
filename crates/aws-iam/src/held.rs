//! What IAM holds beside its identities, each in a table of its own (added after the first tables, so a file made before them opens
//! with them empty): virtual MFA devices, server certificates, a user's other credentials (signing certificates, SSH keys,
//! service-specific credentials), console password hashes, the credential report, last-accessed jobs, the record of which service each
//! user and role last asked for, and the account's preferences.

use std::collections::BTreeMap;

use doze_store::records::{Changes, Records};
use doze_store::{Db, StoreError};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 1;
pub(crate) const MFA: &str = "mfa";
pub(crate) const CERTS: &str = "server-certificates";
pub(crate) const CREDS: &str = "user-credentials";
pub(crate) const PASSWORDS: &str = "passwords";
pub(crate) const REPORTS: &str = "reports";
pub(crate) const JOBS: &str = "jobs";
pub(crate) const ACCESS: &str = "last-accessed";
pub(crate) const PREFS: &str = "preferences";
/// Every table here, for the database to make.
pub(crate) const TABLES: [&str; 8] = [MFA, CERTS, CREDS, PASSWORDS, REPORTS, JOBS, ACCESS, PREFS];

/// A virtual MFA device. Its key is its serial number (its ARN).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Mfa {
    pub(crate) serial: String,
    /// The TOTP secret (20 random bytes).
    pub(crate) seed: Vec<u8>,
    pub(crate) created_ms: i64,
    /// The user it is enabled for, and since when.
    pub(crate) user: Option<(String, i64)>,
    /// The last 30-second step whose code was accepted: a code is good once.
    pub(crate) last_step: i64,
    pub(crate) tags: BTreeMap<String, String>,
}

/// A server certificate. Its key is its name. The private key is checked against the certificate and not kept: IAM never gives it back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ServerCert {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) id: String,
    pub(crate) arn: String,
    pub(crate) body: String,
    pub(crate) chain: Option<String>,
    pub(crate) uploaded_ms: i64,
    pub(crate) expires_ms: i64,
    pub(crate) tags: BTreeMap<String, String>,
}

/// One of a user's credentials other than access keys and the password. Its key is its id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UserCred {
    pub(crate) id: String,
    pub(crate) user: String,
    pub(crate) active: bool,
    pub(crate) created_ms: i64,
    pub(crate) kind: CredKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum CredKind {
    /// An X.509 signing certificate, as uploaded.
    Signing { body: String },
    /// An SSH public key: the key as uploaded, and its MD5 fingerprint.
    Ssh { body: String, fingerprint: String },
    /// A service-specific credential.
    Service(Service),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Service {
    pub(crate) service: String,
    pub(crate) user_name: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) alias: Option<String>,
    pub(crate) secret: Option<String>,
    pub(crate) expires_ms: Option<i64>,
}

/// A user's console password, salted and hashed. Its key is the user's name.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Password {
    pub(crate) salt: Vec<u8>,
    pub(crate) hash: Vec<u8>,
    pub(crate) changed_ms: i64,
    pub(crate) reset_required: bool,
    /// Earlier passwords (salt, hash), newest first, as many as the policy's `PasswordReusePrevention` can ask about.
    pub(crate) previous: Vec<(Vec<u8>, Vec<u8>)>,
}

/// The credential report: when it was made, and its CSV.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Report {
    pub(crate) generated_ms: i64,
    pub(crate) csv: String,
}

/// A last-accessed job: what it was asked about, and what it found then. Its key is the job id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Job {
    pub(crate) arn: String,
    pub(crate) actions: bool,
    pub(crate) created_ms: i64,
    /// The users and roles whose use counts: the entity itself, a group's members, a policy's holders.
    pub(crate) entities: Vec<String>,
    pub(crate) services: Vec<ServiceUse>,
}

/// One service an entity's policies allow, and its use.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ServiceUse {
    pub(crate) namespace: String,
    /// Each user or role that asked, with its last use: newest first.
    pub(crate) entities: Vec<(String, Use)>,
    /// Each action asked for: its name, who asked last, and when.
    pub(crate) actions: Vec<(String, String, Use)>,
}

/// When something was last asked for, and in which region.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Use {
    pub(crate) at_ms: i64,
    pub(crate) region: String,
}

/// A user's or role's last use of one service, and of each of its actions. Its key is `<entity arn> <namespace>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Access {
    pub(crate) last: Use,
    pub(crate) actions: BTreeMap<String, Use>,
}

/// The account's preferences and facts that are not identities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Pref {
    /// `SetSecurityTokenServicePreferences`: 1 or 2.
    TokenVersion(u8),
    /// When this IAM first ran: the root's creation in the credential report.
    Created(i64),
}

/// The tables.
#[derive(Debug)]
pub(crate) struct Held {
    pub(crate) mfa: Records<Mfa>,
    pub(crate) certs: Records<ServerCert>,
    pub(crate) creds: Records<UserCred>,
    pub(crate) passwords: Records<Password>,
    pub(crate) reports: Records<Report>,
    pub(crate) jobs: Records<Job>,
    pub(crate) access: Records<Access>,
    pub(crate) prefs: Records<Pref>,
}

impl Held {
    pub(crate) fn empty() -> Self {
        Self {
            mfa: Records::new(MFA, "MFA device", VERSION),
            certs: Records::new(CERTS, "server certificate", VERSION),
            creds: Records::new(CREDS, "user credential", VERSION),
            passwords: Records::new(PASSWORDS, "password", VERSION),
            reports: Records::new(REPORTS, "report", VERSION),
            jobs: Records::new(JOBS, "job", VERSION),
            access: Records::new(ACCESS, "last access", VERSION),
            prefs: Records::new(PREFS, "preference", VERSION),
        }
    }

    pub(crate) fn load(db: &Db) -> Result<Self, StoreError> {
        let read = &db.read()?;
        Ok(Self {
            mfa: Records::load_or_new(MFA, "MFA device", VERSION, read)?,
            certs: Records::load_or_new(CERTS, "server certificate", VERSION, read)?,
            creds: Records::load_or_new(CREDS, "user credential", VERSION, read)?,
            passwords: Records::load_or_new(PASSWORDS, "password", VERSION, read)?,
            reports: Records::load_or_new(REPORTS, "report", VERSION, read)?,
            jobs: Records::load_or_new(JOBS, "job", VERSION, read)?,
            access: Records::load_or_new(ACCESS, "last access", VERSION, read)?,
            prefs: Records::load_or_new(PREFS, "preference", VERSION, read)?,
        })
    }

    pub(crate) fn stage(&mut self, out: &mut Vec<Changes>) -> Result<(), StoreError> {
        out.extend([
            self.mfa.stage()?,
            self.certs.stage()?,
            self.creds.stage()?,
            self.passwords.stage()?,
            self.reports.stage()?,
            self.jobs.stage()?,
            self.access.stage()?,
            self.prefs.stage()?,
        ]);
        Ok(())
    }

    /// How many records there are and their bytes (the preferences are not counted: the account's own).
    pub(crate) fn count(&self) -> [(u64, u64); 7] {
        use crate::storage::tally;
        [
            tally(&self.mfa),
            tally(&self.certs),
            tally(&self.creds),
            tally(&self.passwords),
            tally(&self.reports),
            tally(&self.jobs),
            tally(&self.access),
        ]
    }

    pub(crate) fn clear(&mut self) {
        use crate::storage::clear;
        clear(&mut self.mfa);
        clear(&mut self.certs);
        clear(&mut self.creds);
        clear(&mut self.passwords);
        clear(&mut self.reports);
        clear(&mut self.jobs);
        clear(&mut self.access);
        clear(&mut self.prefs);
    }
}
